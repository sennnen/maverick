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
use mav_model::time::{ClockMap, DeviceTime, WallTime};
use std::collections::{HashSet, VecDeque};

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
    /// The device timestamp was implausible and a clock correction covered it. Inter-sample
    /// deltas are preserved: the whole segment shifts by one offset.
    Corrected,
    /// The device timestamp was implausible; the capture time was used and the sample flagged.
    /// The caller logs this with code TIMELINE_IMPLAUSIBLE_TIMESTAMP.
    CaptureFallback,
}

/// The grid a stale device clock is snapped to when an anchor is learned. Snapping means the same
/// record re-synced later lands on the same corrected wall time, which is what keeps the store's
/// natural key working across sessions.
pub const ANCHOR_GRID_NANOS: i64 = 300 * 1_000_000_000;

/// Learn a correction from one implausible device time observed at a known wall time. The anchor is
/// snapped to a five-minute grid so a re-sync of the same record corrects to the same instant.
pub fn anchor_from(device: DeviceTime, capture: WallTime) -> ClockMap {
    let offset = capture.as_nanos().saturating_sub(device.as_nanos());
    let snapped = offset.div_euclid(ANCHOR_GRID_NANOS) * ANCHOR_GRID_NANOS;
    let wall_at_start = WallTime::from_nanos(device.as_nanos().saturating_add(snapped));
    ClockMap::anchored(device, wall_at_start)
}

type DedupKey = (StreamKind, i64, u64, u16);

/// How many dedup keys one timeline remembers. A session is one Timeline, and a multi-day backfill
/// pushes every banked sample through it, so the memory has to be bounded by something. 65,536 keys
/// is roughly a day of dense realtime data — far more than any single re-sync burst repeats within.
pub const DEFAULT_DEDUP_WINDOW: usize = 65_536;

/// One device's canonical series under construction. The engine holds one per device per stream
/// window, feeds it scored samples, and drains it in order for storage.
pub struct Timeline {
    seen: HashSet<DedupKey>,
    order: VecDeque<DedupKey>,
    window: usize,
    samples: Vec<Sample<RawValue>>,
}

impl Default for Timeline {
    fn default() -> Self {
        Self::with_window(DEFAULT_DEDUP_WINDOW)
    }
}

impl Timeline {
    pub fn new() -> Self {
        Self::default()
    }

    /// A timeline remembering at most `max_keys` dedup keys, oldest evicted first. This is the fast
    /// path only: the store's natural key is the durable dedup layer, so a duplicate that outlives
    /// the window is still rejected there. A window of zero is meaningless and becomes one.
    pub fn with_window(max_keys: usize) -> Self {
        Self {
            seen: HashSet::new(),
            order: VecDeque::new(),
            window: max_keys.max(1),
            samples: Vec::new(),
        }
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
            self.order.push_back(key);
            while self.order.len() > self.window {
                if let Some(evicted) = self.order.pop_front() {
                    self.seen.remove(&evicted);
                }
            }
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
    place_on_wall_with(&ClockMap::default(), sample, capture)
}

/// Place one sample using a learned clock correction, without touching its raw device timestamp.
///
/// A plausible device time is trusted as-is. An implausible one goes through the map, which shifts
/// the whole segment by a single offset and therefore preserves the gaps between samples — the
/// thing the capture fallback destroys by collapsing every sample in a burst onto one instant.
/// Only when the map covers nothing does the capture time apply. A corrected or fallen-back sample
/// keeps `RejectReason::ImplausibleTimestamp`, so the correction is visible to the inspector.
pub fn place_on_wall_with(
    map: &ClockMap,
    sample: &mut Sample<RawValue>,
    capture: WallTime,
) -> Placement {
    let as_wall = WallTime::from_nanos(sample.device_time.as_nanos());
    if as_wall.is_plausible() {
        sample.wall_time = Some(as_wall);
        return Placement::DeviceClock;
    }
    sample.quality.reason = Some(RejectReason::ImplausibleTimestamp);
    match map.to_wall(sample.device_time) {
        Some(wall) => {
            sample.wall_time = Some(wall);
            Placement::Corrected
        }
        None => {
            sample.wall_time = Some(capture);
            Placement::CaptureFallback
        }
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

    /// The dedup memory is bounded, so a multi-day backfill cannot grow it without limit. Eviction
    /// is oldest-first, and a key that falls out of the window is accepted again here — the store's
    /// natural key is what still rejects it durably.
    #[test]
    fn the_dedup_window_evicts_its_oldest_key_first() {
        let mut timeline = Timeline::with_window(3);
        for seq in 0..3 {
            assert_eq!(timeline.insert(rr(T0, 812, seq)), InsertOutcome::Inserted);
        }
        assert_eq!(timeline.insert(rr(T0, 812, 0)), InsertOutcome::Duplicate);

        // A fourth key evicts the oldest, seq 0.
        assert_eq!(timeline.insert(rr(T0, 812, 3)), InsertOutcome::Inserted);
        assert_eq!(
            timeline.insert(rr(T0, 812, 0)),
            InsertOutcome::Inserted,
            "the evicted key is no longer remembered"
        );
        // That re-insert evicted seq 1 in turn; the window now holds 2, 3, 0.
        assert_eq!(
            timeline.insert(rr(T0, 812, 2)),
            InsertOutcome::Duplicate,
            "a key still inside the window is still remembered"
        );
    }

    #[test]
    fn the_dedup_memory_never_exceeds_its_window() {
        let mut timeline = Timeline::with_window(8);
        for seq in 0..500 {
            timeline.insert(rr(T0, 812, seq));
        }
        assert_eq!(timeline.seen.len(), 8);
        assert_eq!(timeline.order.len(), 8);
        assert_eq!(timeline.len(), 500, "no sample is dropped by eviction");
    }

    #[test]
    fn a_zero_window_still_remembers_one_key() {
        let mut timeline = Timeline::with_window(0);
        assert_eq!(timeline.insert(rr(T0, 812, 0)), InsertOutcome::Inserted);
        assert_eq!(timeline.insert(rr(T0, 812, 0)), InsertOutcome::Duplicate);
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

    /// The load-bearing one. Two samples ten seconds apart under a 1970-era RTC must still be ten
    /// seconds apart on the wall. Collapsing both onto the capture instant destroys every
    /// inter-sample interval, which is the input to every variability metric downstream.
    #[test]
    fn a_stale_clock_preserves_the_gaps_between_samples() {
        let capture = WallTime::from_unix_seconds(1_752_600_123);
        let map = anchor_from(DeviceTime::from_nanos(0), capture);

        let mut first = rr(0, 812, 0);
        let mut second = rr(10 * 1_000_000_000, 812, 1);
        assert_eq!(
            place_on_wall_with(&map, &mut first, capture),
            Placement::Corrected
        );
        assert_eq!(
            place_on_wall_with(&map, &mut second, capture),
            Placement::Corrected
        );

        let gap = second.wall_time.expect("second").as_nanos()
            - first.wall_time.expect("first").as_nanos();
        assert_eq!(gap, 10 * 1_000_000_000, "ten seconds apart, still");
        // Both are flagged, because neither timestamp came from a trustworthy clock.
        assert_eq!(
            first.quality.reason,
            Some(RejectReason::ImplausibleTimestamp)
        );
    }

    /// The anchor snaps to a five-minute grid so the same record re-syncing later corrects to the
    /// same wall time, and the store's natural key still recognises it.
    #[test]
    fn the_anchor_snaps_to_the_five_minute_grid() {
        let device = DeviceTime::from_nanos(0);
        let first = anchor_from(device, WallTime::from_unix_seconds(1_752_600_123));
        let second = anchor_from(device, WallTime::from_unix_seconds(1_752_600_223));
        assert_eq!(
            first.to_wall(device),
            second.to_wall(device),
            "captures 100 seconds apart must land on the same grid step"
        );
        let placed = first.to_wall(device).expect("anchored").as_unix_seconds();
        assert_eq!(placed % 300, 0);
    }

    /// A sample arriving before any correction has been learned falls back to the capture time and
    /// says so, rather than being placed by a map that does not cover it.
    #[test]
    fn a_sample_before_the_first_anchor_falls_back_to_capture() {
        let capture = WallTime::from_unix_seconds(1_752_600_123);
        let map = ClockMap::anchored(DeviceTime::from_nanos(1_000), capture);
        let mut sample = rr(7, 812, 0);
        assert_eq!(
            place_on_wall_with(&map, &mut sample, capture),
            Placement::CaptureFallback
        );
        assert_eq!(sample.wall_time, Some(capture));
        assert_eq!(
            sample.quality.reason,
            Some(RejectReason::ImplausibleTimestamp)
        );
    }

    /// A plausible device clock is never routed through the map, however many corrections it holds.
    #[test]
    fn a_plausible_clock_ignores_the_correction() {
        let capture = WallTime::from_unix_seconds(1_752_600_123);
        let map = anchor_from(DeviceTime::from_nanos(0), capture);
        let mut sample = rr(T0, 812, 0);
        assert_eq!(
            place_on_wall_with(&map, &mut sample, capture),
            Placement::DeviceClock
        );
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
