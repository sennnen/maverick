//! The Tap: a passive observer invoked at every stage boundary. A tap can watch the data move but
//! has no way to change it or the flow; that separation is what lets observability be everywhere
//! without becoming a second control path.

use crate::stage::Stage;
use mav_model::error::MavError;
use mav_model::ids::{DeviceId, FrameId, SessionId, StreamId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// The ids in flight when an event fired, so nothing observed is ever orphaned from its data.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Ids {
    pub device: Option<DeviceId>,
    pub session: Option<SessionId>,
    pub stream: Option<StreamId>,
    pub frame: Option<FrameId>,
}

#[derive(Clone, Debug)]
pub enum TapEvent {
    /// The stage produced output. `summary` is a payload description present only in debug
    /// builds; use [`debug_summary`] to construct it so release builds pay nothing and leak
    /// nothing.
    Produced {
        count: usize,
        ids: Ids,
        summary: Option<String>,
    },
    /// The stage refused something, with the full typed error. This is the no-silent-drops rule
    /// made observable.
    Rejected { error: MavError, ids: Ids },
    /// A state machine moved. Used by acquisition, where every transition is logged.
    Transition {
        from: &'static str,
        to: &'static str,
        ids: Ids,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    Produced,
    Rejected,
    Transition,
}

impl TapEvent {
    pub fn kind(&self) -> EventKind {
        match self {
            TapEvent::Produced { .. } => EventKind::Produced,
            TapEvent::Rejected { .. } => EventKind::Rejected,
            TapEvent::Transition { .. } => EventKind::Transition,
        }
    }
}

pub trait Tap: Send + Sync {
    fn on_stage(&self, stage: Stage, event: TapEvent);
}

/// Build a payload summary that exists only in debug builds. Release builds drop the closure
/// unevaluated, so summaries cost nothing there and raw health bytes can never end up in a
/// production log through this path.
pub fn debug_summary<F: FnOnce() -> String>(f: F) -> Option<String> {
    if cfg!(debug_assertions) {
        Some(f())
    } else {
        let _ = f;
        None
    }
}

/// Fan-out to any number of taps. The engine holds one of these and calls it at each boundary.
#[derive(Default, Clone)]
pub struct Taps {
    taps: Vec<Arc<dyn Tap>>,
}

impl Taps {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, tap: Arc<dyn Tap>) {
        self.taps.push(tap);
    }
}

impl Tap for Taps {
    fn on_stage(&self, stage: Stage, event: TapEvent) {
        for tap in &self.taps {
            tap.on_stage(stage, event.clone());
        }
    }
}

/// Counts events per stage and kind. Cheap enough to leave on always; the numbers answer "did
/// anything flow, and did anything get refused" before a debugger comes out.
#[derive(Default)]
pub struct CountersTap {
    counts: Mutex<BTreeMap<(Stage, EventKind), u64>>,
}

impl CountersTap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> BTreeMap<(Stage, EventKind), u64> {
        match self.counts.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl Tap for CountersTap {
    fn on_stage(&self, stage: Stage, event: TapEvent) {
        let increment = match &event {
            TapEvent::Produced { count, .. } => *count as u64,
            TapEvent::Rejected { .. } | TapEvent::Transition { .. } => 1,
        };
        let mut counts = match self.counts.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *counts.entry((stage, event.kind())).or_insert(0) += increment;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_model::error::codes;

    struct RecordingTap {
        seen: Mutex<Vec<(Stage, EventKind)>>,
    }

    impl Tap for RecordingTap {
        fn on_stage(&self, stage: Stage, event: TapEvent) {
            self.seen.lock().unwrap().push((stage, event.kind()));
        }
    }

    #[test]
    fn tap_receives_stage_events_in_order() {
        let recorder = Arc::new(RecordingTap {
            seen: Mutex::new(Vec::new()),
        });
        let mut taps = Taps::new();
        taps.add(recorder.clone());

        taps.on_stage(
            Stage::Acquisition,
            TapEvent::Transition {
                from: "disconnected",
                to: "scanning",
                ids: Ids::default(),
            },
        );
        taps.on_stage(
            Stage::Frames,
            TapEvent::Produced {
                count: 3,
                ids: Ids::default(),
                summary: None,
            },
        );
        taps.on_stage(
            Stage::Frames,
            TapEvent::Rejected {
                error: MavError::new(codes::FRAME_PAYLOAD_CRC_MISMATCH, "bad crc"),
                ids: Ids::default(),
            },
        );

        assert_eq!(
            *recorder.seen.lock().unwrap(),
            vec![
                (Stage::Acquisition, EventKind::Transition),
                (Stage::Frames, EventKind::Produced),
                (Stage::Frames, EventKind::Rejected),
            ]
        );
    }

    #[test]
    fn counters_accumulate_by_stage_and_kind() {
        let counters = CountersTap::new();
        counters.on_stage(
            Stage::Decode,
            TapEvent::Produced {
                count: 5,
                ids: Ids::default(),
                summary: None,
            },
        );
        counters.on_stage(
            Stage::Decode,
            TapEvent::Produced {
                count: 2,
                ids: Ids::default(),
                summary: None,
            },
        );
        counters.on_stage(
            Stage::Decode,
            TapEvent::Rejected {
                error: MavError::new(codes::DECODE_UNKNOWN_PACKET_TYPE, "type 0x99"),
                ids: Ids::default(),
            },
        );

        let snapshot = counters.snapshot();
        assert_eq!(snapshot[&(Stage::Decode, EventKind::Produced)], 7);
        assert_eq!(snapshot[&(Stage::Decode, EventKind::Rejected)], 1);
    }

    #[test]
    fn debug_summary_matches_build_profile() {
        let summary = debug_summary(|| "3 samples".to_owned());
        if cfg!(debug_assertions) {
            assert_eq!(summary.as_deref(), Some("3 samples"));
        } else {
            assert_eq!(summary, None);
        }
    }
}
