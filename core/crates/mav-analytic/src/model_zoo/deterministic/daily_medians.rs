//! `daily_medians 1.1.0` — the three daily medians the stress models read.
//!
//! Heart-rate variability, minimum heart rate and skin temperature each get one median for
//! the day, taken over the samples that were measured under quiet conditions. "Quiet" is
//! three exclusions, and the whole archive is those three exclusions plus a median:
//!
//!   * the sample's own HRV accuracy is below 20;
//!   * it falls within a minute *after* a MET reading above 1.8, so the wearer was moving;
//!   * it falls inside a sleep period.
//!
//! Skin temperature is sampled on its own clock, so its accuracy exclusion is indirect: a
//! skin-temperature sample is dropped when it lands within a minute after any *HRV* sample
//! whose accuracy was poor. That is not the same as excluding poor skin-temperature samples,
//! and the difference is visible whenever the two series are sampled at different rates.
//!
//! Every window is `[t, t + 60]`, closed at both ends, and forward only — a sample a minute
//! *before* the movement is kept. That asymmetry is the archive's, and it matters at the
//! boundaries.

use super::numpy_median;

/// Below this HRV accuracy the sample is not trusted.
const MIN_HRV_ACCURACY: f64 = 20.0;

/// Above this metabolic equivalent the wearer is moving.
const MOVING_MET: f64 = 1.8;

/// How long after a disqualifying event samples keep being dropped, in seconds.
const EXCLUSION_SECONDS: f64 = 60.0;

/// Why the archive refused an input, with the code it refuses under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DailyMediansError {
    /// 1 — the MET series is empty or holds a missing value.
    MetMissing,
    /// 2 — every MET reading is at or above the movement threshold, so nothing is quiet.
    MetAllMoving,
    /// 3 — a sleep timestamp is missing.
    SleepMissing,
    /// 4 — the sleep timestamps do not pair up into periods.
    SleepUnpaired,
    /// 5 — a median came out undefined because every candidate sample was excluded.
    MedianUndefined,
    /// 6 — no sample of one of the two series survived the exclusions.
    NoQualityMeasurements,
}

impl DailyMediansError {
    /// The archive's own code for this refusal.
    pub fn code(self) -> u8 {
        match self {
            Self::MetMissing => 1,
            Self::MetAllMoving => 2,
            Self::SleepMissing => 3,
            Self::SleepUnpaired => 4,
            Self::MedianUndefined => 5,
            Self::NoQualityMeasurements => 6,
        }
    }
}

/// The three medians, one per series.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DailyMedians {
    /// Median heart-rate variability over the quiet samples.
    pub hrv: f64,
    /// Median minimum heart rate over the same samples.
    pub hr_min: f64,
    /// Median skin temperature over its own quiet samples.
    pub skin_temperature: f64,
}

/// One series and the timestamps it was sampled at.
#[derive(Debug, Clone, Copy)]
pub struct Series<'a> {
    /// The values.
    pub values: &'a [f64],
    /// Unix seconds, one per value.
    pub timestamps: &'a [f64],
}

/// Everything the archive reads for one day.
#[derive(Debug, Clone, Copy)]
pub struct DailyMediansInput<'a> {
    /// Heart-rate variability samples.
    pub hrv: Series<'a>,
    /// The accuracy reported alongside each HRV sample.
    pub hrv_accuracy: &'a [f64],
    /// Minimum heart rate, sampled on the HRV clock.
    pub hr_min: &'a [f64],
    /// Skin temperature, on its own clock.
    pub skin_temperature: Series<'a>,
    /// Metabolic equivalent samples.
    pub met: Series<'a>,
    /// Sleep-period boundaries, alternating start and end.
    pub sleep_timestamps: &'a [f64],
}

/// True where `timestamp` falls in `[event, event + 60]` for any event.
fn shadowed(timestamp: f64, events: &[f64]) -> bool {
    events
        .iter()
        .any(|&event| timestamp >= event && timestamp <= event + EXCLUSION_SECONDS)
}

/// True where `timestamp` falls inside any sleep period.
fn asleep(timestamp: f64, periods: &[(f64, f64)]) -> bool {
    periods
        .iter()
        .any(|&(start, end)| timestamp >= start && timestamp <= end)
}

/// The timestamps of MET readings that put the wearer above the movement threshold.
fn moving_events(met: Series<'_>) -> Vec<f64> {
    met.values
        .iter()
        .zip(met.timestamps)
        .filter(|(value, _)| **value > MOVING_MET)
        .map(|(_, timestamp)| *timestamp)
        .collect()
}

/// Sleep boundaries read as consecutive pairs, ignoring a trailing unpaired start.
fn sleep_periods(timestamps: &[f64]) -> Vec<(f64, f64)> {
    timestamps
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect()
}

fn validate(input: &DailyMediansInput<'_>) -> Result<(), DailyMediansError> {
    if input.met.values.is_empty() || input.met.values.iter().any(|value| value.is_nan()) {
        return Err(DailyMediansError::MetMissing);
    }
    // Not "any quiet sample exists" but "any MET reading is below the threshold" — the
    // archive checks the series, not the samples that survive the other exclusions.
    if !input.met.values.iter().any(|value| *value < MOVING_MET) {
        return Err(DailyMediansError::MetAllMoving);
    }
    if input.sleep_timestamps.iter().any(|value| value.is_nan()) {
        return Err(DailyMediansError::SleepMissing);
    }
    if !input.sleep_timestamps.is_empty() && !input.sleep_timestamps.len().is_multiple_of(2) {
        return Err(DailyMediansError::SleepUnpaired);
    }
    Ok(())
}

/// The three daily medians, or the code the archive would have refused under.
pub fn daily_medians(input: &DailyMediansInput<'_>) -> Result<DailyMedians, DailyMediansError> {
    validate(input)?;

    let moving = moving_events(input.met);
    let periods = sleep_periods(input.sleep_timestamps);

    // The HRV clock: a sample is kept when its own accuracy is good, it is not in a movement
    // shadow, and it is not inside sleep.
    let mut hrv_values = Vec::new();
    let mut hr_min_values = Vec::new();
    let mut poor_accuracy_times = Vec::new();
    for (index, &timestamp) in input.hrv.timestamps.iter().enumerate() {
        let accuracy = input.hrv_accuracy.get(index).copied().unwrap_or(f64::NAN);
        if accuracy < MIN_HRV_ACCURACY {
            poor_accuracy_times.push(timestamp);
            continue;
        }
        if shadowed(timestamp, &moving) || asleep(timestamp, &periods) {
            continue;
        }
        if let Some(value) = input.hrv.values.get(index) {
            hrv_values.push(*value);
        }
        if let Some(value) = input.hr_min.get(index) {
            hr_min_values.push(*value);
        }
    }

    // The skin-temperature clock: same movement and sleep exclusions, but the accuracy one
    // arrives as a shadow cast by the poor HRV samples rather than as a per-sample test.
    let mut skin_values = Vec::new();
    for (index, &timestamp) in input.skin_temperature.timestamps.iter().enumerate() {
        if shadowed(timestamp, &moving)
            || asleep(timestamp, &periods)
            || shadowed(timestamp, &poor_accuracy_times)
        {
            continue;
        }
        if let Some(value) = input.skin_temperature.values.get(index) {
            skin_values.push(*value);
        }
    }

    if hrv_values.is_empty() || skin_values.is_empty() {
        return Err(DailyMediansError::NoQualityMeasurements);
    }

    let hrv = numpy_median(&hrv_values).ok_or(DailyMediansError::MedianUndefined)?;
    let hr_min = numpy_median(&hr_min_values).ok_or(DailyMediansError::MedianUndefined)?;
    let skin_temperature = numpy_median(&skin_values).ok_or(DailyMediansError::MedianUndefined)?;
    if hrv.is_nan() || hr_min.is_nan() || skin_temperature.is_nan() {
        return Err(DailyMediansError::MedianUndefined);
    }
    Ok(DailyMedians {
        hrv,
        hr_min,
        skin_temperature,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: f64 = 1_700_000_000.0;

    /// The archive is exact arithmetic over f64, so the port should be too; this leaves room
    /// only for the decimal round trip through the vector file.
    const TOLERANCE: f64 = 1e-9;

    struct Day {
        hrv: Vec<f64>,
        accuracy: Vec<f64>,
        hr_min: Vec<f64>,
        skin: Vec<f64>,
        skin_times: Vec<f64>,
        met: Vec<f64>,
        met_times: Vec<f64>,
        sleep: Vec<f64>,
        hrv_times: Vec<f64>,
    }

    impl Day {
        fn input(&self) -> DailyMediansInput<'_> {
            DailyMediansInput {
                hrv: Series {
                    values: &self.hrv,
                    timestamps: &self.hrv_times,
                },
                hrv_accuracy: &self.accuracy,
                hr_min: &self.hr_min,
                skin_temperature: Series {
                    values: &self.skin,
                    timestamps: &self.skin_times,
                },
                met: Series {
                    values: &self.met,
                    timestamps: &self.met_times,
                },
                sleep_timestamps: &self.sleep,
            }
        }
    }

    /// A day with one moving stretch, one poor-accuracy sample and one sleep period, laid
    /// out so each exclusion removes a different, identifiable sample.
    fn day() -> Day {
        let hrv_times: Vec<f64> = (0..10).map(|i| START + f64::from(i) * 300.0).collect();
        Day {
            hrv: vec![40.0, 41.0, 42.0, 43.0, 44.0, 45.0, 46.0, 47.0, 48.0, 49.0],
            // Sample 3 is below the accuracy threshold; every other sample clears it.
            accuracy: vec![50.0, 50.0, 50.0, 10.0, 50.0, 50.0, 50.0, 50.0, 50.0, 50.0],
            hr_min: vec![50.0, 51.0, 52.0, 53.0, 54.0, 55.0, 56.0, 57.0, 58.0, 59.0],
            skin: vec![34.0, 34.1, 34.2, 34.3, 34.4, 34.5],
            skin_times: (0..6).map(|i| START + f64::from(i) * 500.0).collect(),
            // One reading above the threshold, at the timestamp of HRV sample 1.
            met: vec![1.0, 2.5, 1.2, 1.1],
            met_times: vec![START, START + 300.0, START + 600.0, START + 900.0],
            // Sleep covers HRV samples 8 and 9.
            sleep: vec![START + 2400.0, START + 2700.0],
            hrv_times,
        }
    }

    #[test]
    fn rejects_a_met_series_that_never_falls_below_the_movement_threshold() {
        let mut day = day();
        day.met = vec![2.0, 2.5, 3.0, 4.0];
        assert_eq!(
            daily_medians(&day.input()),
            Err(DailyMediansError::MetAllMoving)
        );
        assert_eq!(DailyMediansError::MetAllMoving.code(), 2);
    }

    #[test]
    fn rejects_sleep_timestamps_that_do_not_pair_up() {
        let mut day = day();
        day.sleep = vec![START + 2400.0, START + 2700.0, START + 3000.0];
        assert_eq!(
            daily_medians(&day.input()),
            Err(DailyMediansError::SleepUnpaired)
        );
        assert_eq!(DailyMediansError::SleepUnpaired.code(), 4);
    }

    #[test]
    fn rejects_an_empty_met_series() {
        let mut day = day();
        day.met = vec![];
        day.met_times = vec![];
        assert_eq!(
            daily_medians(&day.input()),
            Err(DailyMediansError::MetMissing)
        );
    }

    #[test]
    fn drops_the_sample_under_poor_accuracy_and_the_two_inside_sleep() {
        let day = day();
        let got = daily_medians(&day.input()).expect("the day should produce medians");
        // Samples 0,2,4,5,6,7 survive: 1 is in the movement shadow, 3 fails accuracy,
        // 8 and 9 are asleep. Their HRV values are 40,42,44,45,46,47 — median 44.5.
        assert!((got.hrv - 44.5).abs() < TOLERANCE, "hrv was {}", got.hrv);
        // The same six positions in hr_min: 50,52,54,55,56,57 — median 54.5.
        assert!(
            (got.hr_min - 54.5).abs() < TOLERANCE,
            "hr_min was {}",
            got.hr_min
        );
    }

    #[test]
    fn the_exclusion_window_runs_forward_only() {
        // A sample one second *before* the movement is kept; one second after is dropped.
        let events = [START + 300.0];
        assert!(!shadowed(START + 299.0, &events));
        assert!(shadowed(START + 301.0, &events));
        assert!(
            shadowed(START + 360.0, &events),
            "the window is closed at 60"
        );
        assert!(!shadowed(START + 361.0, &events));
    }

    #[test]
    fn skin_temperature_is_excluded_by_the_hrv_accuracy_shadow_not_its_own() {
        // HRV sample 3 is at START + 900 with poor accuracy, so skin samples in
        // [900, 960] go. START + 500 * 2 = START + 1000 is outside it and stays.
        let day = day();
        let got = daily_medians(&day.input()).expect("the day should produce medians");
        // Skin samples at 0, 500, 1000, 1500, 2000, 2500. The one at 0 is in the movement
        // shadow of nothing (the first MET reading is 1.0), 2500 is inside sleep.
        // Surviving: 34.0, 34.1, 34.2, 34.3, 34.4 — median 34.2.
        assert!(
            (got.skin_temperature - 34.2).abs() < TOLERANCE,
            "skin temperature was {}",
            got.skin_temperature
        );
    }

    #[test]
    fn refuses_when_every_sample_of_a_series_is_excluded() {
        let mut day = day();
        // Sleep over the whole day removes every HRV sample.
        day.sleep = vec![START - 1.0, START + 100_000.0];
        assert_eq!(
            daily_medians(&day.input()),
            Err(DailyMediansError::NoQualityMeasurements)
        );
    }

    /// Vectors generated by `tools/ml/deterministic_vectors.py daily_medians_1_1_0`, which
    /// runs the archive itself. Regenerate rather than edit; see docs/testing.md.
    #[test]
    fn matches_the_archive_on_generated_vectors() {
        let raw =
            include_str!("../../../../../../artifacts/models/vectors/daily_medians_1_1_0.json");
        let file: serde_json::Value =
            serde_json::from_str(raw).expect("the vector file should parse");
        let vectors = file["vectors"]
            .as_array()
            .expect("vectors should be a list");
        let mut checked = 0;
        for vector in vectors {
            let inputs = &vector["inputs"];
            let read = |name: &str| -> Vec<f64> {
                inputs[name]
                    .as_array()
                    .expect("input should be a list")
                    .iter()
                    .map(|v| v.as_f64().expect("input should be a number"))
                    .collect()
            };
            let (hrv, accuracy, hrv_times, hr_min) = (
                read("hrv"),
                read("hrv_accuracy"),
                read("hrv_timestamps"),
                read("hr_min"),
            );
            let (skin, skin_times) = (read("skin_temp"), read("skin_temp_timestamps"));
            let (met, met_times) = (read("met"), read("met_timestamps"));
            let sleep = read("sleep_timestamps");
            let input = DailyMediansInput {
                hrv: Series {
                    values: &hrv,
                    timestamps: &hrv_times,
                },
                hrv_accuracy: &accuracy,
                hr_min: &hr_min,
                skin_temperature: Series {
                    values: &skin,
                    timestamps: &skin_times,
                },
                met: Series {
                    values: &met,
                    timestamps: &met_times,
                },
                sleep_timestamps: &sleep,
            };
            let got = daily_medians(&input);
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
                    let got = got.expect("the archive produced medians for this input");
                    let want = vector["outputs"]
                        .as_array()
                        .expect("outputs should be a list");
                    let scalar = |index: usize| -> f64 {
                        want[index][0][0]
                            .as_f64()
                            .expect("output should be a number")
                    };
                    assert!(
                        (got.hrv - scalar(0)).abs() < TOLERANCE,
                        "hrv {} vs {}",
                        got.hrv,
                        scalar(0)
                    );
                    assert!(
                        (got.hr_min - scalar(1)).abs() < TOLERANCE,
                        "hr_min {} vs {}",
                        got.hr_min,
                        scalar(1)
                    );
                    assert!(
                        (got.skin_temperature - scalar(2)).abs() < TOLERANCE,
                        "skin temperature {} vs {}",
                        got.skin_temperature,
                        scalar(2)
                    );
                }
            }
            checked += 1;
        }
        assert_eq!(checked, 6, "every generated vector should be checked");
    }
}
