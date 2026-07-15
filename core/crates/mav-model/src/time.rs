//! Two clocks and the mapping between them.
//!
//! `DeviceTime` is the strap's own clock, expressed in nanoseconds once the connector has
//! normalised whatever raw units the wire uses (seconds, 1/32768, milliseconds, and so on — the
//! subsecond unit differs by field and by device, so it is the connector's job to settle it, not
//! this crate's). `WallTime` is UTC. A strap's clock drifts and is sometimes wrong by years, so we
//! never assume the two are equal; a `ClockMap` holds the corrections we have actually learned and
//! refuses to place a sample it has no correction for. Nothing here invents a timestamp.

use serde::{Deserialize, Serialize};

/// 2000-01-01T00:00:00Z. Anything earlier is treated as an implausible device clock.
const MIN_PLAUSIBLE_UNIX_SECONDS: i64 = 946_684_800;
/// 2100-01-01T00:00:00Z. Anything later is treated as an implausible device clock.
const MAX_PLAUSIBLE_UNIX_SECONDS: i64 = 4_102_444_800;

const NANOS_PER_SECOND: i64 = 1_000_000_000;
const NANOS_PER_MILLI: i64 = 1_000_000;

/// The strap's own clock, in nanoseconds. Unit normalisation is the connector's responsibility.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct DeviceTime(i64);

impl DeviceTime {
    pub const fn from_nanos(nanos: i64) -> Self {
        Self(nanos)
    }

    pub const fn as_nanos(self) -> i64 {
        self.0
    }
}

/// UTC time, in nanoseconds since the Unix epoch.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct WallTime(i64);

impl WallTime {
    pub const fn from_nanos(nanos: i64) -> Self {
        Self(nanos)
    }

    pub const fn from_unix_seconds(seconds: i64) -> Self {
        Self(seconds.saturating_mul(NANOS_PER_SECOND))
    }

    pub const fn from_unix_millis(millis: i64) -> Self {
        Self(millis.saturating_mul(NANOS_PER_MILLI))
    }

    pub const fn as_nanos(self) -> i64 {
        self.0
    }

    pub const fn as_unix_seconds(self) -> i64 {
        self.0.div_euclid(NANOS_PER_SECOND)
    }

    /// True when the time falls inside the window a real wearable capture could plausibly carry.
    /// A strap fresh off the shelf, or one whose clock has glitched, reports times well outside it,
    /// and the timeline uses this to decide whether to trust the device clock or fall back to the
    /// time the phone received the frame.
    pub const fn is_plausible(self) -> bool {
        let seconds = self.as_unix_seconds();
        seconds >= MIN_PLAUSIBLE_UNIX_SECONDS && seconds <= MAX_PLAUSIBLE_UNIX_SECONDS
    }
}

/// One learned correction: at `start_device`, the device clock lined up with `wall_at_start`, and
/// within the segment the device clock is taken to advance one-for-one with wall time.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct ClockSegment {
    pub start_device: DeviceTime,
    pub wall_at_start: WallTime,
}

/// A piecewise mapping from device time to wall time, built from the corrections observed during a
/// session. Segments are held sorted by `start_device`. A device time before the first segment maps
/// to nothing, because we have no correction for it and will not guess one.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ClockMap {
    segments: Vec<ClockSegment>,
}

impl ClockMap {
    pub fn new(mut segments: Vec<ClockSegment>) -> Self {
        segments.sort_by_key(|s| s.start_device);
        Self { segments }
    }

    /// A map with a single anchor: everywhere from `start_device` onward, device time equals wall
    /// time plus a fixed offset.
    pub fn anchored(start_device: DeviceTime, wall_at_start: WallTime) -> Self {
        Self::new(vec![ClockSegment {
            start_device,
            wall_at_start,
        }])
    }

    pub fn segments(&self) -> &[ClockSegment] {
        &self.segments
    }

    /// Place a device time on the wall clock using the applicable segment, or `None` if the time is
    /// before every correction we hold or the arithmetic would overflow.
    pub fn to_wall(&self, device: DeviceTime) -> Option<WallTime> {
        let segment = self
            .segments
            .iter()
            .rev()
            .find(|s| s.start_device <= device)?;
        let delta = device
            .as_nanos()
            .checked_sub(segment.start_device.as_nanos())?;
        let wall = segment.wall_at_start.as_nanos().checked_add(delta)?;
        Some(WallTime::from_nanos(wall))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_seconds_roundtrip() {
        let t = WallTime::from_unix_seconds(1_752_600_000);
        assert_eq!(t.as_unix_seconds(), 1_752_600_000);
    }

    #[test]
    fn plausibility_brackets_the_window() {
        assert!(!WallTime::from_unix_seconds(0).is_plausible());
        assert!(!WallTime::from_unix_seconds(MIN_PLAUSIBLE_UNIX_SECONDS - 1).is_plausible());
        assert!(WallTime::from_unix_seconds(MIN_PLAUSIBLE_UNIX_SECONDS).is_plausible());
        assert!(WallTime::from_unix_seconds(1_752_600_000).is_plausible());
        assert!(WallTime::from_unix_seconds(MAX_PLAUSIBLE_UNIX_SECONDS).is_plausible());
        assert!(!WallTime::from_unix_seconds(MAX_PLAUSIBLE_UNIX_SECONDS + 1).is_plausible());
    }

    #[test]
    fn to_wall_uses_the_latest_applicable_segment() {
        let map = ClockMap::new(vec![
            ClockSegment {
                start_device: DeviceTime::from_nanos(0),
                wall_at_start: WallTime::from_unix_seconds(1_000),
            },
            ClockSegment {
                start_device: DeviceTime::from_nanos(500 * NANOS_PER_SECOND),
                wall_at_start: WallTime::from_unix_seconds(9_000),
            },
        ]);

        // Inside the first segment: 100 s of device time past the anchor at wall 1000.
        assert_eq!(
            map.to_wall(DeviceTime::from_nanos(100 * NANOS_PER_SECOND)),
            Some(WallTime::from_unix_seconds(1_100))
        );
        // Past the second anchor: the later segment wins and the earlier one is not applied.
        assert_eq!(
            map.to_wall(DeviceTime::from_nanos(510 * NANOS_PER_SECOND)),
            Some(WallTime::from_unix_seconds(9_010))
        );
    }

    #[test]
    fn device_time_before_first_segment_maps_to_nothing() {
        let map = ClockMap::anchored(
            DeviceTime::from_nanos(1_000),
            WallTime::from_unix_seconds(1_752_600_000),
        );
        assert_eq!(map.to_wall(DeviceTime::from_nanos(999)), None);
    }

    #[test]
    fn empty_map_maps_to_nothing() {
        let map = ClockMap::default();
        assert_eq!(map.to_wall(DeviceTime::from_nanos(0)), None);
    }
}
