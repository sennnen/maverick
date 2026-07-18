//! Resting heart rate (WHOOP-P6, `[WRS]`): the WHOOP-style floor — the lowest sustained 5-minute
//! in-bed level — not the night mean. `session_resting_hr` is the per-session floor;
//! `daily_resting_hr` folds the day's sessions to their minimum. Plain HR samples in, bpm out.
//! Absent signal returns `None`. Wellness estimate, never medical.

const WINDOW_SECONDS: i64 = 5 * 60;

/// One HR reading: unix-second `ts` and integer `bpm`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HrSample {
    pub ts: i64,
    pub bpm: i32,
}

impl HrSample {
    pub fn new(ts: i64, bpm: i32) -> Self {
        Self { ts, bpm }
    }
}

/// Round to nearest, ties toward positive infinity (matches the upstream integer rounding).
fn round_half_up(x: f64) -> i32 {
    (x + 0.5).floor() as i32
}

/// The session floor: the lowest 5-min tumbling-window mean bpm over `[start, end]`. Falls back to
/// the whole-segment mean when no window holds a sample, and to `None` when the segment is empty.
pub fn session_resting_hr(start: i64, end: i64, hr: &[HrSample]) -> Option<i32> {
    let seg: Vec<&HrSample> = hr.iter().filter(|s| s.ts >= start && s.ts <= end).collect();
    if seg.is_empty() {
        return None;
    }
    let mut means: Vec<f64> = Vec::new();
    let mut t = start;
    while t < end {
        let win: Vec<&&HrSample> = seg
            .iter()
            .filter(|s| s.ts >= t && s.ts < t + WINDOW_SECONDS)
            .collect();
        if !win.is_empty() {
            let sum: i64 = win.iter().map(|s| i64::from(s.bpm)).sum();
            means.push(sum as f64 / win.len() as f64);
        }
        t += WINDOW_SECONDS;
    }
    if let Some(m) = means.into_iter().reduce(f64::min) {
        return Some(round_half_up(m));
    }
    let all: i64 = seg.iter().map(|s| i64::from(s.bpm)).sum();
    Some(round_half_up(all as f64 / seg.len() as f64))
}

/// The day's resting HR: the minimum session floor across the day's matched sessions, or `None`.
pub fn daily_resting_hr(session_floors: &[Option<i32>]) -> Option<i32> {
    session_floors.iter().filter_map(|f| *f).min()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_is_the_lowest_sustained_window_not_the_mean() {
        // First 5 min at 60 bpm, second 5 min at 50: floor is 50, the night mean would be 55.
        let mut hr = Vec::new();
        for t in 0..300 {
            hr.push(HrSample::new(t, 60));
        }
        for t in 300..600 {
            hr.push(HrSample::new(t, 50));
        }
        assert_eq!(session_resting_hr(0, 600, &hr), Some(50));
    }

    #[test]
    fn empty_segment_is_none_and_daily_folds_to_minimum() {
        assert_eq!(session_resting_hr(0, 600, &[]), None);
        assert_eq!(daily_resting_hr(&[Some(52), None, Some(48)]), Some(48));
        assert_eq!(daily_resting_hr(&[None, None]), None);
    }

    #[test]
    fn rounding_is_half_up() {
        // A single 5-min window whose mean lands exactly on .5 rounds up.
        let hr = [HrSample::new(0, 50), HrSample::new(1, 51)];
        assert_eq!(session_resting_hr(0, 300, &hr), Some(51));
    }
}
