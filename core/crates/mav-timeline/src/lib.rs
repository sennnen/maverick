//! The canonical timeline: ordering, deduplication, and clock placement. Two rules hold
//! absolutely and are restated from docs/pipeline.md because this is the crate that enforces
//! them: the timeline never interpolates, and it never mutates a raw device timestamp. A clock
//! correction is a stored mapping and a flag, not an edit.
//!
//! The dedup key carries the lesson this lineage learned the hard way: two equal RR intervals in
//! the same second are two distinct heartbeats. The key is (kind, device_time, value bits, seq),
//! and without the `seq` tiebreaker the second of two equal intervals vanishes, a zero-difference
//! beat disappears, and RMSSD and every HRV figure built on it bias high at rest and in sleep.
#![forbid(unsafe_code)]

use mav_model::raw::RawValue;
use mav_model::stream::{RejectReason, Sample, StreamKind};
use mav_model::time::WallTime;
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InsertOutcome {
    Inserted,
    /// A sample identical in every key field (kind, device time, value, seq) was already present.
    /// Normal during re-sync; the caller counts these rather than logging each one.
    Duplicate,
}

/// Where a sample's wall-clock placement came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Placement {
    /// The device timestamp was plausible and was used directly.
    DeviceClock,
    /// The device timestamp was implausible; the capture time was used and the sample flagged.
    /// The caller logs this with code TIMELINE_IMPLAUSIBLE_TIMESTAMP.
    CaptureFallback,
}

type DedupKey = (StreamKind, i64, u64, u16);

/// One device's canonical series under construction. The engine holds one per device per stream
/// window, feeds it scored samples, and drains it in order for storage.
#[derive(Default)]
pub struct Timeline {
    seen: HashSet<DedupKey>,
    samples: Vec<Sample<RawValue>>,
}

impl Timeline {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert one sample, deduplicating on (kind, device_time, value bits, seq). Equal values at
    /// the same instant with different `seq` are distinct on purpose; see the crate doc.
    pub fn insert(&mut self, sample: Sample<RawValue>) -> InsertOutcome {
        let key: DedupKey = (
            sample.kind,
            sample.device_time.as_nanos(),
            sample.value.key_bits(),
            sample.seq,
        );
        if self.seen.insert(key) {
            self.samples.push(sample);
            InsertOutcome::Inserted
        } else {
            InsertOutcome::Duplicate
        }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// The canonical series: ordered by device time, then seq, then kind and value bits so the
    /// order is total and identical regardless of insertion order. The dedup memory is kept, so a
    /// timeline can keep receiving a re-sync after a drain.
    pub fn drain_ordered(&mut self) -> Vec<Sample<RawValue>> {
        let mut out = std::mem::take(&mut self.samples);
        out.sort_by_key(|s| {
            (
                s.device_time.as_nanos(),
                s.seq,
                s.kind as u8,
                s.value.key_bits(),
            )
        });
        out
    }
}

/// Place one sample on the wall clock without touching its raw device timestamp. A plausible
/// device time is trusted as-is; an implausible one falls back to the time the phone captured
/// the frame, and the sample carries `RejectReason::ImplausibleTimestamp` so the fallback is
/// visible all the way to the inspector.
pub fn place_on_wall(sample: &mut Sample<RawValue>, capture: WallTime) -> Placement {
    let as_wall = WallTime::from_nanos(sample.device_time.as_nanos());
    if as_wall.is_plausible() {
        sample.wall_time = Some(as_wall);
        Placement::DeviceClock
    } else {
        sample.wall_time = Some(capture);
        sample.quality.reason = Some(RejectReason::ImplausibleTimestamp);
        Placement::CaptureFallback
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_model::ids::MetadataId;
    use mav_model::stream::Quality;
    use mav_model::time::DeviceTime;

    fn rr(device_nanos: i64, ms: u16, seq: u16) -> Sample<RawValue> {
        Sample {
            kind: StreamKind::RrInterval,
            device_time: DeviceTime::from_nanos(device_nanos),
            wall_time: None,
            seq,
            value: RawValue::U16(ms),
            quality: Quality::scored(1.0),
            provenance: MetadataId::new(0),
        }
    }

    const T0: i64 = 1_752_600_000 * 1_000_000_000;

    #[test]
    fn equal_rr_in_same_second_are_kept_distinct() {
        let mut timeline = Timeline::new();
        assert_eq!(timeline.insert(rr(T0, 812, 0)), InsertOutcome::Inserted);
        assert_eq!(timeline.insert(rr(T0, 812, 1)), InsertOutcome::Inserted);
        assert_eq!(
            timeline.len(),
            2,
            "two equal intervals are two beats, not one"
        );
    }

    #[test]
    fn exact_duplicate_is_deduped() {
        let mut timeline = Timeline::new();
        assert_eq!(timeline.insert(rr(T0, 812, 0)), InsertOutcome::Inserted);
        assert_eq!(timeline.insert(rr(T0, 812, 0)), InsertOutcome::Duplicate);
        assert_eq!(timeline.len(), 1);
    }

    #[test]
    fn dedup_memory_survives_a_drain_for_resync() {
        let mut timeline = Timeline::new();
        timeline.insert(rr(T0, 812, 0));
        let drained = timeline.drain_ordered();
        assert_eq!(drained.len(), 1);
        assert_eq!(timeline.insert(rr(T0, 812, 0)), InsertOutcome::Duplicate);
        assert!(timeline.is_empty());
    }

    #[test]
    fn implausible_timestamp_falls_back_and_flags() {
        let capture = WallTime::from_unix_seconds(1_752_600_123);
        let mut sample = rr(0, 812, 0);
        let placement = place_on_wall(&mut sample, capture);
        assert_eq!(placement, Placement::CaptureFallback);
        assert_eq!(sample.wall_time, Some(capture));
        assert_eq!(
            sample.quality.reason,
            Some(RejectReason::ImplausibleTimestamp)
        );
    }

    #[test]
    fn plausible_timestamp_uses_the_device_clock() {
        let capture = WallTime::from_unix_seconds(1_752_600_123);
        let mut sample = rr(T0, 812, 0);
        let placement = place_on_wall(&mut sample, capture);
        assert_eq!(placement, Placement::DeviceClock);
        assert_eq!(sample.wall_time, Some(WallTime::from_nanos(T0)));
        assert_eq!(sample.quality.reason, None);
    }

    #[test]
    fn raw_timestamp_never_mutated() {
        let capture = WallTime::from_unix_seconds(1_752_600_123);
        let mut implausible = rr(7, 812, 0);
        let mut plausible = rr(T0, 812, 0);
        place_on_wall(&mut implausible, capture);
        place_on_wall(&mut plausible, capture);
        assert_eq!(implausible.device_time, DeviceTime::from_nanos(7));
        assert_eq!(plausible.device_time, DeviceTime::from_nanos(T0));
    }
}
