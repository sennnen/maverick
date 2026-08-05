//! Short-term stress baselines, ported from `daily_short_term_baselines` 1.1.0.
//!
//! Turns a short history of daily medians into the three baselines the daytime-stress path reads,
//! plus a night HRV baseline drawn from whole nights that passed a plausibility filter.
//!
//! The three daily baselines are Gaussian-weighted averages, so the most recent days and the
//! oldest days both count less than the middle of the window. The night baseline is a plain
//! median over nights that survive filtering — a different estimator on purpose, because one
//! implausible night should be discarded rather than down-weighted.

use super::torch_median;
use mav_model::version::Version;

pub const ALGORITHM: &str = "short_term_stress_baselines";
pub const VERSION: Version = Version::new(1, 1, 0);

/// The archive rejects a history shorter than this.
pub const MINIMUM_DAYS: usize = 5;

/// Window over which a night is considered a night at all: four hours, in seconds.
const MINIMUM_SLEEP_SECONDS: f64 = 14_400.0;
const LOWEST_HEART_RATE_RANGE: (f64, f64) = (30.0, 200.0);
const HIGHEST_TEMPERATURE_RANGE: (f64, f64) = (28.0, 40.0);
const AVERAGE_HRV_RANGE: (f64, f64) = (5.0, 150.0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShortTermBaselines {
    pub dhrv_baseline: f64,
    pub skin_temperature_baseline: f64,
    pub minimum_heart_rate_baseline: f64,
    /// `None` when no night in the history passed the plausibility filter.
    pub night_hrv_baseline: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortTermBaselineError {
    /// Fewer than [`MINIMUM_DAYS`] daily medians.
    HistoryTooShort(usize),
    /// The three median series must describe the same days.
    MismatchedSeries,
    /// The four nightly series must describe the same nights.
    MismatchedNights,
    NonFiniteInput,
    /// Every value in one of the median series was missing.
    NoUsableHistory,
}

/// One night's summary, as the plausibility filter reads it.
#[derive(Clone, Copy, Debug)]
pub struct NightSummary {
    pub total_sleep_seconds: f64,
    pub lowest_heart_rate: f64,
    pub highest_temperature: f64,
    pub average_hrv: f64,
}

impl NightSummary {
    /// True when every field is inside the range the archive requires.
    ///
    /// All four are checked together and the night is kept or dropped whole: a night with an
    /// implausible temperature is a night whose HRV is not trustworthy either.
    fn is_plausible(&self) -> bool {
        let in_range = |value: f64, (low, high): (f64, f64)| (low..=high).contains(&value);
        self.total_sleep_seconds >= MINIMUM_SLEEP_SECONDS
            && in_range(self.lowest_heart_rate, LOWEST_HEART_RATE_RANGE)
            && in_range(self.highest_temperature, HIGHEST_TEMPERATURE_RANGE)
            && in_range(self.average_hrv, AVERAGE_HRV_RANGE)
    }
}

/// Gaussian weights over a window, centred on its middle.
///
/// Computed in `f64`, which is deliberately *more* precise than the archive. The archive casts
/// its inputs and weights to `f32` and reduces in `f32`, so a baseline of exactly 46 comes back
/// from it as 46.000004. Reproducing that would mean matching torch's reduction order as well as
/// its precision — fragile, and it would only make this port less accurate. The golden tests
/// therefore compare within the archive's own single-precision noise rather than bit for bit.
fn gaussian_weights(window_length: usize) -> Vec<f64> {
    let length = window_length as f64;
    let standard_deviation = length / 2.5;
    let centre = (length - 1.0) / 2.0;
    (0..window_length)
        .map(|index| {
            let offset = index as f64 - centre;
            (-(offset * offset) / (2.0 * standard_deviation * standard_deviation)).exp()
        })
        .collect()
}

/// Gaussian-weighted average over a window of daily medians.
///
/// A missing day is not skipped. The archive multiplies it through, lands a NaN in the baseline
/// and raises; this returns `None` so the caller can raise the same way. Quietly dropping the day
/// and renormalising would be a different estimator from the one the thresholds downstream were
/// chosen against, which is not a change a port gets to make.
fn weighted_average(medians: &[f64]) -> Option<f64> {
    let weights = gaussian_weights(medians.len());
    let mut weighted_sum = 0.0;
    let mut weight_total = 0.0;
    for (value, weight) in medians.iter().zip(weights.iter()) {
        weighted_sum += value * weight;
        weight_total += weight;
    }
    let average = weighted_sum / weight_total;
    average.is_finite().then_some(average)
}

/// Derive the short-term baselines from a history of daily medians and nightly summaries.
pub fn short_term_baselines(
    dhrv_medians: &[f64],
    skin_temperature_medians: &[f64],
    minimum_heart_rate_medians: &[f64],
    nights: &[NightSummary],
) -> Result<ShortTermBaselines, ShortTermBaselineError> {
    if dhrv_medians.len() < MINIMUM_DAYS {
        return Err(ShortTermBaselineError::HistoryTooShort(dhrv_medians.len()));
    }
    if skin_temperature_medians.len() != dhrv_medians.len()
        || minimum_heart_rate_medians.len() != dhrv_medians.len()
    {
        return Err(ShortTermBaselineError::MismatchedSeries);
    }
    for series in [
        dhrv_medians,
        skin_temperature_medians,
        minimum_heart_rate_medians,
    ] {
        if series.iter().any(|value| value.is_infinite()) {
            return Err(ShortTermBaselineError::NonFiniteInput);
        }
    }
    for night in nights {
        if !night.total_sleep_seconds.is_finite()
            || !night.lowest_heart_rate.is_finite()
            || !night.highest_temperature.is_finite()
            || !night.average_hrv.is_finite()
        {
            return Err(ShortTermBaselineError::MismatchedNights);
        }
    }

    let dhrv_baseline =
        weighted_average(dhrv_medians).ok_or(ShortTermBaselineError::NoUsableHistory)?;
    let skin_temperature_baseline = weighted_average(skin_temperature_medians)
        .ok_or(ShortTermBaselineError::NoUsableHistory)?;
    let minimum_heart_rate_baseline = weighted_average(minimum_heart_rate_medians)
        .ok_or(ShortTermBaselineError::NoUsableHistory)?;

    let plausible: Vec<f64> = nights
        .iter()
        .filter(|night| night.is_plausible())
        .map(|night| night.average_hrv)
        .collect();

    Ok(ShortTermBaselines {
        dhrv_baseline,
        skin_temperature_baseline,
        minimum_heart_rate_baseline,
        night_hrv_baseline: torch_median(&plausible),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nights(
        durations: &[f64],
        heart_rates: &[f64],
        temperatures: &[f64],
        hrvs: &[f64],
    ) -> Vec<NightSummary> {
        (0..durations.len())
            .map(|index| NightSummary {
                total_sleep_seconds: durations[index],
                lowest_heart_rate: heart_rates[index],
                highest_temperature: temperatures[index],
                average_hrv: hrvs[index],
            })
            .collect()
    }

    /// How far a value may sit from the reference.
    ///
    /// The archive reduces in single precision, so its own answers carry rounding of order 1e-6 at
    /// these magnitudes — 46.0 comes back from it as 46.000004. This port is `f64`, so the gap is
    /// the *archive's* error rather than this code's, and the bar is set to admit it.
    const REFERENCE_TOLERANCE: f64 = 1e-4;

    /// Generated by running `daily_short_term_baselines` 1.1.0 itself.
    #[test]
    fn matches_the_reference_on_a_five_day_history() {
        let got = short_term_baselines(
            &[42.0, 44.0, 46.0, 48.0, 50.0],
            &[33.0, 33.2, 33.4, 33.1, 33.3],
            &[52.0, 51.0, 50.0, 49.0, 48.0],
            &nights(
                &[21600.0, 25200.0, 28800.0, 26000.0, 27000.0],
                &[48.0, 49.0, 47.0, 50.0, 46.0],
                &[36.5, 36.6, 36.4, 36.7, 36.3],
                &[45.0, 47.0, 44.0, 46.0, 48.0],
            ),
        )
        .expect("valid history");
        assert!(
            (got.dhrv_baseline - 46.000_003_81).abs() < REFERENCE_TOLERANCE,
            "{got:?}"
        );
        assert!(
            (got.skin_temperature_baseline - 33.212_844_85).abs() < REFERENCE_TOLERANCE,
            "{got:?}"
        );
        assert!(
            (got.minimum_heart_rate_baseline - 50.000_003_81).abs() < REFERENCE_TOLERANCE,
            "{got:?}"
        );
        assert_eq!(got.night_hrv_baseline, Some(46.0));
    }

    #[test]
    fn matches_the_reference_on_a_seven_day_history() {
        let got = short_term_baselines(
            &[55.0, 60.0, 58.0, 62.0, 59.0, 61.0, 57.0],
            &[34.0, 34.1, 33.9, 34.2, 34.0, 34.1, 33.8],
            &[45.0, 44.0, 46.0, 43.0, 45.0, 44.0, 46.0],
            &nights(
                &[
                    28800.0, 29000.0, 27000.0, 30000.0, 26000.0, 28000.0, 29500.0,
                ],
                &[44.0, 45.0, 43.0, 46.0, 44.0, 45.0, 43.0],
                &[36.8, 36.9, 36.7, 37.0, 36.8, 36.9, 36.6],
                &[55.0, 58.0, 54.0, 60.0, 56.0, 57.0, 53.0],
            ),
        )
        .expect("valid history");
        assert!(
            (got.dhrv_baseline - 59.181_285_86).abs() < REFERENCE_TOLERANCE,
            "{got:?}"
        );
        assert!(
            (got.skin_temperature_baseline - 34.026_741_03).abs() < REFERENCE_TOLERANCE,
            "{got:?}"
        );
        assert!(
            (got.minimum_heart_rate_baseline - 44.631_134_03).abs() < REFERENCE_TOLERANCE,
            "{got:?}"
        );
        assert_eq!(got.night_hrv_baseline, Some(56.0));
    }

    #[test]
    fn a_history_shorter_than_five_days_is_refused() {
        let short = short_term_baselines(
            &[30.0, 35.0, 40.0],
            &[32.0, 32.5, 33.0],
            &[60.0, 58.0, 56.0],
            &[],
        );
        assert_eq!(short, Err(ShortTermBaselineError::HistoryTooShort(3)));
    }

    #[test]
    fn series_of_different_lengths_are_refused_rather_than_zipped_short() {
        let mismatched = short_term_baselines(
            &[42.0, 44.0, 46.0, 48.0, 50.0],
            &[33.0, 33.2],
            &[52.0, 51.0, 50.0, 49.0, 48.0],
            &[],
        );
        assert_eq!(mismatched, Err(ShortTermBaselineError::MismatchedSeries));
    }

    #[test]
    fn the_middle_of_the_window_outweighs_its_ends() {
        // Same days, one with the high value in the middle and one with it at the edge. The
        // Gaussian window has to score the middle placement higher.
        let middle =
            short_term_baselines(&[40.0, 40.0, 60.0, 40.0, 40.0], &[33.0; 5], &[50.0; 5], &[])
                .expect("valid");
        let edge =
            short_term_baselines(&[60.0, 40.0, 40.0, 40.0, 40.0], &[33.0; 5], &[50.0; 5], &[])
                .expect("valid");
        assert!(
            middle.dhrv_baseline > edge.dhrv_baseline,
            "middle {} should outweigh edge {}",
            middle.dhrv_baseline,
            edge.dhrv_baseline
        );
    }

    #[test]
    fn a_missing_day_makes_the_baseline_unusable_rather_than_being_dropped() {
        // The archive propagates the NaN into the baseline and raises. Renormalising over the
        // days that are present would be a quieter but materially different estimator.
        let with_gap = short_term_baselines(
            &[46.0, 46.0, f64::NAN, 46.0, 46.0],
            &[33.0; 5],
            &[50.0; 5],
            &[],
        );
        assert_eq!(with_gap, Err(ShortTermBaselineError::NoUsableHistory));
    }

    #[test]
    fn an_implausible_night_is_dropped_from_the_night_baseline() {
        // The middle night slept twenty minutes; its HRV must not reach the median.
        let got = short_term_baselines(
            &[46.0; 5],
            &[33.0; 5],
            &[50.0; 5],
            &nights(
                &[28800.0, 28800.0, 1200.0, 28800.0, 28800.0],
                &[45.0, 45.0, 45.0, 45.0, 45.0],
                &[36.5, 36.5, 36.5, 36.5, 36.5],
                &[50.0, 52.0, 999.0, 54.0, 56.0],
            ),
        )
        .expect("valid");
        assert_eq!(got.night_hrv_baseline, Some(52.0));
    }

    #[test]
    fn no_plausible_night_means_no_night_baseline() {
        let got = short_term_baselines(
            &[46.0; 5],
            &[33.0; 5],
            &[50.0; 5],
            &nights(&[600.0; 5], &[45.0; 5], &[36.5; 5], &[50.0; 5]),
        )
        .expect("valid");
        assert_eq!(got.night_hrv_baseline, None);
    }

    #[test]
    fn a_history_of_only_missing_days_is_an_error() {
        let got = short_term_baselines(&[f64::NAN; 5], &[33.0; 5], &[50.0; 5], &[]);
        assert_eq!(got, Err(ShortTermBaselineError::NoUsableHistory));
    }

    /// The same archive through the shared generator, so this port is covered by the
    /// regenerable path every other deterministic port uses.
    #[test]
    fn matches_the_generated_vectors() {
        let raw = include_str!(
            "../../../../../../artifacts/models/vectors/daily_short_term_baselines_1_1_0.json"
        );
        let file: serde_json::Value =
            serde_json::from_str(raw).expect("the vector file should parse");
        let mut produced = 0;
        let mut refused = 0;
        for vector in file["vectors"]
            .as_array()
            .expect("vectors should be a list")
        {
            let inputs = &vector["inputs"];
            let series = |name: &str| -> Vec<f64> {
                inputs[name]
                    .as_array()
                    .expect("a series")
                    .iter()
                    .map(|value| value.as_f64().expect("a number"))
                    .collect()
            };
            let (sleep, lowest, highest, hrv) = (
                series("total_sleep_durations"),
                series("lowest_heart_rates"),
                series("highest_temperatures"),
                series("average_hrvs"),
            );
            let nights: Vec<NightSummary> = (0..sleep.len())
                .map(|index| NightSummary {
                    total_sleep_seconds: sleep[index],
                    lowest_heart_rate: lowest[index],
                    highest_temperature: highest[index],
                    average_hrv: hrv[index],
                })
                .collect();
            let got = short_term_baselines(
                &series("dhrv_medians"),
                &series("skin_temp_medians"),
                &series("hr_min_medians"),
                &nights,
            );
            if vector.get("error").is_some() {
                got.expect_err("the archive refused this input");
                refused += 1;
                continue;
            }
            let got = got.expect("the archive produced baselines");
            let want = vector["outputs"].as_array().expect("outputs are a list");
            let expected = |index: usize| -> f64 {
                let mut value = &want[index];
                while let Some(items) = value.as_array() {
                    value = &items[0];
                }
                value.as_f64().expect("a number")
            };
            // The port computes in f64 where the archive casts to f32; see the module doc.
            let tolerance = 1e-4;
            let fields = [
                ("dhrv", got.dhrv_baseline, 0usize),
                ("skin temperature", got.skin_temperature_baseline, 1),
                ("minimum heart rate", got.minimum_heart_rate_baseline, 2),
            ];
            for (name, value, index) in fields {
                let target = expected(index);
                assert!(
                    (value - target).abs() <= tolerance * target.abs().max(1.0),
                    "{name}: {value} vs {target}"
                );
            }
            let night = expected(3);
            match got.night_hrv_baseline {
                Some(value) => assert!(
                    (value - night).abs() <= tolerance * night.abs().max(1.0),
                    "night hrv: {value} vs {night}"
                ),
                None => assert!(night.is_nan(), "night hrv should have been produced"),
            }
            produced += 1;
        }
        assert_eq!(
            (produced, refused),
            (4, 1),
            "four baselines and one refusal"
        );
    }
}
