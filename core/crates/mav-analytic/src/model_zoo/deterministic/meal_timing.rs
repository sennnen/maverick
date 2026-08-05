//! `meal_timing 0.1.0` — the hours a wearer usually eats in, and whether they keep to them.
//!
//! Logged meal times go into 48 half-hour bins of *local* time, each meal rounded to the
//! nearest bin. Bins the wearer uses often become windows; windows close enough together
//! become one; and a window has to carry a real share of the meals to survive at all.
//!
//! The one structural trick is the extension. A window running from 23:00 to 00:30 is two
//! runs at opposite ends of a 48-bin array and one window in a person's day. So the array is
//! extended by twelve bins — the first six hours appended after the last — and clustering runs
//! over the 60-bin strip, where the late window is contiguous. Anything that comes back round
//! to the same window is then dropped, so it is not counted twice.
//!
//! The consistency flag is deliberately coarse: one if at least 70% of logged meals fall
//! inside the windows *and* no window is longer than three hours, zero if not, and absent
//! below ten logged meals, where the question does not mean anything yet.

/// Half-hour bins in a day.
const BINS_PER_DAY: usize = 48;

/// How many bins are appended so a window across midnight stays contiguous.
const EXTENSION_BINS: usize = 12;

/// Minutes per bin.
const MINUTES_PER_BIN: i64 = 30;

/// A bin has to reach this share of the busiest bin to be part of a window.
const MIN_SCALED_FREQUENCY: f64 = 0.2;

/// Windows separated by more than this many bins stay separate.
const MAX_GAP_BINS: i64 = 2;

/// A window has to hold at least this many meals.
const MIN_MEALS_PER_WINDOW: f64 = 2.0;

/// …and at least this share of all of them.
const MIN_MEAL_SHARE_PER_WINDOW: f64 = 0.1;

/// Below this many logged meals, consistency is not scored.
const MIN_MEALS_TO_SCORE: f64 = 10.0;

/// At least this share of meals must fall inside the windows to be consistent.
const CONSISTENT_MEAL_SHARE: f64 = 0.7;

/// No window may be longer than this to be consistent, in minutes.
const CONSISTENT_MAX_WINDOW_MINUTES: i64 = 180;

/// Why the archive refused the input, with the code it refuses under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MealTimingError {
    /// 1 — no timestamps.
    NoTimestamps,
    /// 3 — no timezone offsets.
    NoTimezones,
    /// 5 — the two are different lengths.
    LengthMismatch,
}

impl MealTimingError {
    /// The archive's own code for this refusal.
    pub fn code(self) -> u8 {
        match self {
            Self::NoTimestamps => 1,
            Self::NoTimezones => 3,
            Self::LengthMismatch => 5,
        }
    }
}

/// One window the wearer habitually eats in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MealWindow {
    /// Where it starts, in minutes past local midnight.
    pub start_minutes: i64,
    /// How long it runs, in minutes.
    pub duration_minutes: i64,
}

/// The windows, and whether the wearer keeps to them.
#[derive(Debug, Clone, PartialEq)]
pub struct MealTiming {
    /// The habitual windows, in order.
    pub windows: Vec<MealWindow>,
    /// One for consistent, zero for not, absent below ten logged meals.
    pub consistent: Option<bool>,
}

/// Which half-hour bin a local timestamp rounds into.
///
/// Rounding is to the nearest bin, and it wraps: 23:50 rounds forward to midnight rather than
/// back to 23:30, which is what puts a late meal at the start of the day rather than the end.
fn bin_of(local_seconds: i64) -> usize {
    let minutes = local_seconds.rem_euclid(86_400) / 60;
    let rounded = (minutes + MINUTES_PER_BIN / 2).div_euclid(MINUTES_PER_BIN) * MINUTES_PER_BIN;
    (rounded.rem_euclid(1440) / MINUTES_PER_BIN) as usize
}

/// Meals per bin, extended so a window across midnight is contiguous.
fn extended_histogram(timestamps: &[i64], timezones: &[i64]) -> Vec<f64> {
    let mut bins = vec![0.0; BINS_PER_DAY];
    for (timestamp, offset) in timestamps.iter().zip(timezones) {
        bins[bin_of(timestamp + offset)] += 1.0;
    }
    let head: Vec<f64> = bins[..EXTENSION_BINS].to_vec();
    bins.extend(head);
    bins
}

/// Runs of busy bins, as inclusive `(first, last)` index pairs into the extended strip.
fn cluster(bins: &[f64]) -> Vec<(i64, i64)> {
    let busiest = bins.iter().copied().fold(0.0f64, f64::max);
    if busiest <= 0.0 {
        return Vec::new();
    }
    let busy: Vec<i64> = bins
        .iter()
        .enumerate()
        .filter(|(_, count)| *count / busiest >= MIN_SCALED_FREQUENCY)
        .map(|(index, _)| index as i64)
        .collect();
    if busy.is_empty() {
        return Vec::new();
    }
    let mut clusters = Vec::new();
    let mut start = busy[0];
    let mut end = busy[0];
    for pair in busy.windows(2) {
        if pair[1] - pair[0] > MAX_GAP_BINS {
            clusters.push((start, end));
            start = pair[1];
        }
        end = pair[1];
    }
    clusters.push((start, end));
    clusters
}

/// The windows the wearer habitually eats in, and whether they keep to them.
pub fn meal_timing(timestamps: &[i64], timezones: &[i64]) -> Result<MealTiming, MealTimingError> {
    if timestamps.is_empty() {
        return Err(MealTimingError::NoTimestamps);
    }
    if timezones.is_empty() {
        return Err(MealTimingError::NoTimezones);
    }
    if timestamps.len() != timezones.len() {
        return Err(MealTimingError::LengthMismatch);
    }

    let bins = extended_histogram(timestamps, timezones);
    let total: f64 = bins.iter().sum();

    // Halves, because a single-bin window is widened by half a bin either side: one bin is a
    // point in the clustering and half an hour of a person's evening.
    let mut kept: Vec<(f64, f64)> = Vec::new();
    for (start, end) in cluster(&bins) {
        let inside: f64 = bins[start as usize..=end as usize].iter().sum();
        if inside < MIN_MEAL_SHARE_PER_WINDOW * total || inside < MIN_MEALS_PER_WINDOW {
            continue;
        }
        let (start, end) = if start == end {
            (start as f64 - 0.5, end as f64 + 0.5)
        } else {
            (start as f64, end as f64)
        };
        kept.push((start, end));
        // Once a window runs past the end of the real day, everything after it is the
        // extension repeating what has already been counted.
        if end >= BINS_PER_DAY as f64 {
            break;
        }
    }

    // A window that wrapped past midnight now covers the same hours as the first window; the
    // extension has already represented it, so the first copy goes.
    if kept.len() >= 2 {
        let wrapped = kept[kept.len() - 1].1 - BINS_PER_DAY as f64;
        if wrapped >= kept[0].0 && wrapped <= kept[0].1 {
            kept.remove(0);
        }
    }

    let windows: Vec<MealWindow> = kept
        .iter()
        .map(|(start, end)| MealWindow {
            start_minutes: (start * MINUTES_PER_BIN as f64) as i64,
            duration_minutes: ((end - start + 1.0) * MINUTES_PER_BIN as f64) as i64,
        })
        .collect();

    let consistent = if windows.is_empty() || total < MIN_MEALS_TO_SCORE {
        None
    } else {
        let inside: f64 = kept
            .iter()
            .map(|(start, end)| {
                // The bounds may be half-bins after widening; the archive truncates both.
                let (first, last) = (*start as usize, *end as usize);
                bins[first..=last.min(bins.len() - 1)].iter().sum::<f64>()
            })
            .sum();
        let longest = windows
            .iter()
            .map(|window| window.duration_minutes)
            .max()
            .unwrap_or(0);
        Some(inside / total >= CONSISTENT_MEAL_SHARE && longest <= CONSISTENT_MAX_WINDOW_MINUTES)
    };

    Ok(MealTiming {
        windows,
        consistent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    const BASE: i64 = 1_700_000_000;
    const OFFSET: i64 = 3600;

    fn meals(hours: &[f64], days: usize) -> (Vec<i64>, Vec<i64>) {
        let mut stamps = Vec::new();
        for day in 0..days {
            for hour in hours {
                stamps.push(BASE + day as i64 * DAY + (hour * 3600.0) as i64);
            }
        }
        let offsets = vec![OFFSET; stamps.len()];
        (stamps, offsets)
    }

    #[test]
    fn rounding_goes_to_the_nearest_bin_and_wraps_at_midnight() {
        // Rounding is at the half-bin, so 23:45 is the first minute that wraps to midnight.
        assert_eq!(bin_of(23 * 3600 + 45 * 60), 0);
        assert_eq!(bin_of(23 * 3600 + 50 * 60), 0);
        assert_eq!(bin_of(23 * 3600 + 44 * 60), 47);
        assert_eq!(bin_of(23 * 3600 + 20 * 60), 47);
        assert_eq!(bin_of(0), 0);
        assert_eq!(bin_of(30 * 60), 1);
    }

    #[test]
    fn three_regular_meals_become_three_windows() {
        let (stamps, offsets) = meals(&[7.5, 12.5, 19.0], 14);
        let got = meal_timing(&stamps, &offsets).expect("valid");
        assert_eq!(got.windows.len(), 3);
        assert_eq!(got.consistent, Some(true));
        assert!(got.windows.iter().all(|w| w.duration_minutes == 60));
    }

    #[test]
    fn a_grazing_window_longer_than_three_hours_is_not_consistent() {
        let (stamps, offsets) = meals(&[13.0, 13.5, 14.0, 14.5, 15.0, 15.5, 16.0], 10);
        let got = meal_timing(&stamps, &offsets).expect("valid");
        assert_eq!(got.windows.len(), 1);
        assert!(got.windows[0].duration_minutes > CONSISTENT_MAX_WINDOW_MINUTES);
        assert_eq!(got.consistent, Some(false));
    }

    #[test]
    fn too_few_meals_leaves_consistency_unanswered() {
        let (stamps, offsets) = meals(&[8.0], 6);
        let got = meal_timing(&stamps, &offsets).expect("valid");
        assert_eq!(got.consistent, None, "six meals is not enough to judge");
    }

    #[test]
    fn a_single_meal_forms_no_window_at_all() {
        let (stamps, offsets) = meals(&[8.0], 1);
        let got = meal_timing(&stamps, &offsets).expect("valid");
        assert!(
            got.windows.is_empty(),
            "one meal is below the two-meal minimum"
        );
        assert_eq!(got.consistent, None);
    }

    #[test]
    fn a_single_bin_window_is_widened_to_half_a_bin_either_side() {
        let (stamps, offsets) = meals(&[8.0], 6);
        let got = meal_timing(&stamps, &offsets).expect("valid");
        assert_eq!(got.windows.len(), 1);
        // A bin is thirty minutes; widened it runs an hour, starting fifteen minutes early.
        assert_eq!(got.windows[0].duration_minutes, 60);
        assert_eq!(got.windows[0].start_minutes % MINUTES_PER_BIN, 15);
    }

    #[test]
    fn refuses_the_inputs_the_archive_refuses() {
        assert_eq!(
            meal_timing(&[], &[OFFSET]),
            Err(MealTimingError::NoTimestamps)
        );
        assert_eq!(meal_timing(&[BASE], &[]), Err(MealTimingError::NoTimezones));
        assert_eq!(
            meal_timing(&[BASE, BASE], &[OFFSET]),
            Err(MealTimingError::LengthMismatch)
        );
    }

    /// Vectors generated by `tools/ml/deterministic_vectors.py meal_timing_0_1_0`.
    #[test]
    fn matches_the_archive_on_generated_vectors() {
        let raw = include_str!("../../../../../../artifacts/models/vectors/meal_timing_0_1_0.json");
        let file: serde_json::Value =
            serde_json::from_str(raw).expect("the vector file should parse");
        let mut checked = 0;
        for vector in file["vectors"]
            .as_array()
            .expect("vectors should be a list")
        {
            let read = |name: &str| -> Vec<i64> {
                vector["inputs"][name]
                    .as_array()
                    .expect("a list")
                    .iter()
                    .map(|value| value.as_f64().expect("a number") as i64)
                    .collect()
            };
            let got = meal_timing(&read("unix_timestamps"), &read("unix_timezones"));
            match vector.get("error").and_then(|e| e.as_str()) {
                Some(message) => {
                    let error = got.expect_err("the archive refused this input");
                    assert!(
                        message.starts_with(&format!("builtins.Exception: {}", error.code())),
                        "expected code {} for {message}",
                        error.code()
                    );
                }
                None => {
                    let got = got.expect("the archive produced windows");
                    let want = vector["outputs"].as_array().expect("outputs are a list");
                    let windows = want[0].as_array().expect("windows are a list");
                    assert_eq!(got.windows.len(), windows.len(), "window count");
                    for (index, window) in got.windows.iter().enumerate() {
                        let row = windows[index].as_array().expect("a window");
                        assert_eq!(
                            window.start_minutes,
                            row[0].as_f64().expect("a start") as i64,
                            "window {index} start"
                        );
                        assert_eq!(
                            window.duration_minutes,
                            row[1].as_f64().expect("a duration") as i64,
                            "window {index} duration"
                        );
                    }
                    let flag = want[1].as_array().expect("a matrix")[0]
                        .as_array()
                        .expect("a row")[0]
                        .as_f64();
                    assert_eq!(
                        got.consistent,
                        flag.map(|value| value == 1.0),
                        "consistency flag"
                    );
                }
            }
            checked += 1;
        }
        assert_eq!(checked, 7, "every generated vector should be checked");
    }
}
