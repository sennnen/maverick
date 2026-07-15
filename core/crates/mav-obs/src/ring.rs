//! The always-on ring log: a fixed-capacity in-memory record of recent pipeline events, including
//! every rejection with its code. It answers "what just happened" and is what the report bundle
//! snapshots; its durable sibling, the SQLite error journal, lands with mav-store.

use crate::stage::Stage;
use crate::tap::{Tap, TapEvent};
use mav_model::error::Severity;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct RingEntry {
    /// Monotonic within one log; later events have larger values, so gaps after eviction are
    /// visible.
    pub seq: u64,
    pub stage: Stage,
    pub kind: RingEntryKind,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RingEntryKind {
    Produced {
        count: usize,
        summary: Option<String>,
    },
    Rejected {
        code: u16,
        severity: Severity,
        message: String,
        context: Vec<String>,
    },
    Transition {
        from: String,
        to: String,
    },
}

struct Inner {
    entries: VecDeque<RingEntry>,
    next_seq: u64,
}

pub struct RingLog {
    capacity: usize,
    inner: Mutex<Inner>,
}

impl RingLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            inner: Mutex::new(Inner {
                entries: VecDeque::new(),
                next_seq: 0,
            }),
        }
    }

    pub fn push(&self, stage: Stage, kind: RingEntryKind) {
        let mut inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let seq = inner.next_seq;
        inner.next_seq += 1;
        if inner.entries.len() == self.capacity {
            inner.entries.pop_front();
        }
        inner.entries.push_back(RingEntry { seq, stage, kind });
    }

    /// The most recent entries, oldest first, at most `limit`.
    pub fn recent(&self, limit: usize) -> Vec<RingEntry> {
        let inner = match self.inner.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let skip = inner.entries.len().saturating_sub(limit);
        inner.entries.iter().skip(skip).cloned().collect()
    }

    pub fn len(&self) -> usize {
        match self.inner.lock() {
            Ok(guard) => guard.entries.len(),
            Err(poisoned) => poisoned.into_inner().entries.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// The tap that feeds the ring log from stage boundaries.
pub struct RingLogTap(pub Arc<RingLog>);

impl Tap for RingLogTap {
    fn on_stage(&self, stage: Stage, event: TapEvent) {
        let kind = match event {
            TapEvent::Produced { count, summary, .. } => RingEntryKind::Produced { count, summary },
            TapEvent::Rejected { error, .. } => RingEntryKind::Rejected {
                code: error.code,
                severity: error.severity,
                message: error.message,
                context: error.context,
            },
            TapEvent::Transition { from, to, .. } => RingEntryKind::Transition {
                from: from.to_owned(),
                to: to.to_owned(),
            },
        };
        self.0.push(stage, kind);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tap::Ids;
    use mav_model::error::{codes, MavError};

    #[test]
    fn ring_log_retains_recent_and_evicts_oldest() {
        let log = RingLog::new(3);
        for i in 0..5u64 {
            log.push(
                Stage::Frames,
                RingEntryKind::Produced {
                    count: i as usize,
                    summary: None,
                },
            );
        }
        let recent = log.recent(10);
        assert_eq!(log.len(), 3);
        assert_eq!(recent.len(), 3);
        assert_eq!(
            recent.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![2, 3, 4],
            "oldest entries are evicted and seq shows the gap"
        );
    }

    #[test]
    fn recent_respects_the_limit_and_orders_oldest_first() {
        let log = RingLog::new(10);
        for i in 0..4usize {
            log.push(
                Stage::Sqi,
                RingEntryKind::Produced {
                    count: i,
                    summary: None,
                },
            );
        }
        let last_two = log.recent(2);
        assert_eq!(last_two.len(), 2);
        assert!(last_two[0].seq < last_two[1].seq);
        assert_eq!(last_two[1].seq, 3);
    }

    #[test]
    fn rejection_is_logged_with_code_and_reason() {
        let log = Arc::new(RingLog::new(8));
        let tap = RingLogTap(log.clone());
        tap.on_stage(
            Stage::Frames,
            TapEvent::Rejected {
                error: MavError::new(codes::FRAME_HEADER_CRC_MISMATCH, "header crc mismatch")
                    .context("frame 12"),
                ids: Ids::default(),
            },
        );

        let entries = log.recent(1);
        match &entries[0].kind {
            RingEntryKind::Rejected {
                code,
                message,
                context,
                ..
            } => {
                assert_eq!(*code, codes::FRAME_HEADER_CRC_MISMATCH);
                assert!(!message.is_empty());
                assert_eq!(context, &vec!["frame 12".to_owned()]);
            }
            other => panic!("expected a rejection entry, got {other:?}"),
        }
    }

    #[test]
    fn entries_serialise_for_the_report_bundle() {
        let entry = RingEntry {
            seq: 7,
            stage: Stage::Timeline,
            kind: RingEntryKind::Transition {
                from: "idle".into(),
                to: "streaming".into(),
            },
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert_eq!(serde_json::from_str::<RingEntry>(&json).unwrap(), entry);
    }
}
