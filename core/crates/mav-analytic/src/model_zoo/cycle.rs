//! The popsicle front-end: a cycle history into the two tensors its eight heads take.
//!
//! All eight popsicle models — ovulation detection, ovulation prediction, period prediction and
//! the minimum-follicular head, each in a current and a previous generation — take the same pair
//! of tensors: `time_series (1, 40, 3)` and `scalars (1, 40, 4)`. Nothing in the converted graphs
//! says where those seven columns come from, and for a while this build said the front-end could
//! not be recovered. It could: the archive's own `PassThroughPreprocessor` is a scripted module
//! that still runs, and every constant it closes over is readable.
//!
//! What it does, confirmed by running it rather than by reading it (see
//! `tools/ml/popsicle_vectors.py`):
//!
//! - **Series columns**, in this order: `highest_temperature` (°C), `average_breath_rate`
//!   (breaths per minute), `average_heart_rate` (bpm). One row per cycle day.
//! - **Rejection, not scaling.** A value outside its valid range becomes `NaN` and then zero.
//!   Values *inside* the range pass through untouched — the archive carries `scaled_min` and
//!   `scaled_max` attributes and this version's path never applies them, which is the kind of
//!   thing only running the module can settle.
//! - **Missing and rejected agree.** A `NaN` day and an out-of-range day both end up as zeros,
//!   because imputation is disabled in the shipped configuration.
//! - **Scalar columns**, repeated per day: `age`, `typical_cycle_length`, `typical_luteal_length`,
//!   and `cycle_day` — which is one-based and counts up across the history.
//! - **The luteal column is not the wearer's.** Whatever is passed in, the shipped preprocessor
//!   puts the population default of 13 days in the scalar block and keeps the wearer's own figure
//!   only in an `original_typical_luteal_length` output the model never reads. This holds for
//!   every value tried and for both settings of `complete_cycle`. Reading the source would have
//!   suggested the opposite; running it is what settled it, and the vectors pin it.
//!
//! Maverick has every one of these. Temperature is the nightly skin-temperature high, breath rate
//! is [`crate::respiratory_rate`], heart rate is the day's average, and the three cycle figures
//! come from the wearer's own logged cycles. That is why these eight models are runnable here and
//! the activity family is not: the activity features are Oura ring firmware outputs, and these
//! are ordinary physiology.
//!
//! This is awareness only. Nothing here is contraception, fertility prediction, or a diagnosis.

use super::health::{InputHealth, Substitution};
use mav_model::error::{codes, MavError, Result};

/// The cycle heads this front-end can feed.
///
/// The two `min_follicular` heads are deliberately absent: they take a nine-value `features`
/// tensor, not the day-sequence pair built here, and their front-end is not ported. Listing them
/// would queue them with tensors of the wrong shape, which `validate_request` would refuse — but
/// only after the caller believed the model was running.
pub const CYCLE_MODELS: &[super::ModelId] = &[
    super::ModelId::PopsicleOvulationDetection,
    super::ModelId::PopsicleOvulationDetectionV16,
    super::ModelId::PopsicleOvulationPrediction,
    super::ModelId::PopsicleOvulationPredictionV16,
    super::ModelId::PopsiclePeriodPrediction,
    super::ModelId::PopsiclePeriodPredictionV16,
];

/// Days of history the converted cores were built at.
///
/// The archive's own preprocessor allows 67; the exported graphs fix 40, so the window is the
/// most recent 40 cycle days. Padding goes at the end, which is where `pad_packed_sequence`
/// puts it.
pub const CYCLE_DAYS: usize = 40;

/// Series columns per day, in the order the model reads them.
pub const SERIES_COLUMNS: usize = 3;

/// Scalar columns per day.
pub const SCALAR_COLUMNS: usize = 4;

/// Valid range per series column, from the archive's `sequence_preprocessor.valid_min/max`.
/// Outside these a reading is not clamped, it is discarded.
const SERIES_VALID: [(f32, f32); SERIES_COLUMNS] = [(35.5, 37.5), (8.0, 24.0), (30.0, 120.0)];

/// What the archive substitutes when the wearer has not told us, from `scalar_defaults_dict`.
pub const DEFAULT_AGE_YEARS: f32 = 35.0;
pub const DEFAULT_CYCLE_LENGTH_DAYS: f32 = 28.0;
pub const DEFAULT_LUTEAL_LENGTH_DAYS: f32 = 13.0;

/// Bounds the archive's input validator accepts for the scalar block. There is no luteal-length
/// bound here because no wearer-supplied luteal length ever reaches the model — see
/// [`CycleProfile::resolved`].
const AGE_RANGE: (f32, f32) = (1.0, 140.0);
const CYCLE_LENGTH_RANGE: (f32, f32) = (12.0, 90.0);

/// One cycle day's physiology, as the wearer's own history holds it.
///
/// `None` is a day that was not recorded, which the front-end treats identically to a day whose
/// reading fell outside the plausible range — both become zero, and the model was trained that
/// way.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct CycleDay {
    /// Highest skin temperature that night, in degrees Celsius.
    pub highest_temperature_c: Option<f32>,
    /// Mean respiratory rate that night, in breaths per minute.
    pub average_breath_rate: Option<f32>,
    /// Mean heart rate that day, in beats per minute.
    pub average_heart_rate_bpm: Option<f32>,
}

/// What the wearer has told us about themselves and their cycles.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CycleProfile {
    pub age_years: Option<f32>,
    pub typical_cycle_length_days: Option<f32>,
    pub typical_luteal_length_days: Option<f32>,
}

impl CycleProfile {
    /// The three scalars the model's block carries.
    ///
    /// Age and cycle length are the wearer's own, defaulted when absent or implausible — the
    /// validator's behaviour, and the alternative would let one mistyped number withhold a whole
    /// surface. The luteal length is *not* the wearer's: the shipped preprocessor substitutes the
    /// population default unconditionally, so passing theirs through here would feed the model a
    /// column it was never given in training.
    fn resolved(&self) -> (f32, f32, f32) {
        (
            admit(self.age_years, AGE_RANGE).unwrap_or(DEFAULT_AGE_YEARS),
            admit(self.typical_cycle_length_days, CYCLE_LENGTH_RANGE)
                .unwrap_or(DEFAULT_CYCLE_LENGTH_DAYS),
            DEFAULT_LUTEAL_LENGTH_DAYS,
        )
    }
}

/// The two tensors the popsicle heads take.
#[derive(Clone, Debug, PartialEq)]
pub struct CycleInput {
    /// `(1, 40, 3)` row-major: forty days of three columns.
    pub time_series: Vec<f32>,
    /// `(1, 40, 4)` row-major: forty days of four columns.
    pub scalars: Vec<f32>,
    /// How many of the forty rows are real history rather than padding. Carried so a caller can
    /// refuse to read a prediction that is mostly padding.
    pub days: usize,
    /// What the series was actually made of: real readings, days the archive rejected as
    /// out-of-range, and days that were never recorded.
    ///
    /// The counted positions are the series cells, not the scalar ones — the scalars are the
    /// wearer's profile repeated per day and are always present, so including them would dilute
    /// the fraction with values that cannot be missing and make an empty history look
    /// three-sevenths real.
    pub health: InputHealth,
}

/// A reading the archive's validator accepts for its column, or `None`.
///
/// One predicate for the series cells and the scalar block both, because they have to agree: the
/// archive discards an implausible value to `NaN` and then fills every `NaN` with zero, so
/// "37.6 °C" and "no reading" reach the model as the same number. The callers differ only in what
/// they substitute — zero for a series cell, the population default for a scalar.
fn admit(value: Option<f32>, range: (f32, f32)) -> Option<f32> {
    value.filter(|value| value.is_finite() && *value >= range.0 && *value <= range.1)
}

/// Build the popsicle input from a cycle history, oldest day first.
///
/// Histories longer than [`CYCLE_DAYS`] keep their most recent forty days; shorter ones are
/// zero-padded at the end, which is where the training pipeline's `pad_packed_sequence` puts
/// padding.
pub fn cycle_input(history: &[CycleDay], profile: &CycleProfile) -> Result<CycleInput> {
    if history.is_empty() {
        return Err(MavError::new(
            codes::ML_MODEL_TENSOR_INVALID,
            "a cycle prediction needs at least one logged day",
        ));
    }
    let window = if history.len() > CYCLE_DAYS {
        &history[history.len() - CYCLE_DAYS..]
    } else {
        history
    };
    let (age, cycle_length, luteal_length) = profile.resolved();

    let mut time_series = vec![0.0; CYCLE_DAYS * SERIES_COLUMNS];
    let mut scalars = vec![0.0; CYCLE_DAYS * SCALAR_COLUMNS];
    let mut real = 0usize;
    let mut rejected = false;
    let mut missing = window.len() < CYCLE_DAYS;
    for (index, day) in window.iter().enumerate() {
        let base = index * SERIES_COLUMNS;
        let cells = [
            (day.highest_temperature_c, SERIES_VALID[0]),
            (day.average_breath_rate, SERIES_VALID[1]),
            (day.average_heart_rate_bpm, SERIES_VALID[2]),
        ];
        for (offset, (value, range)) in cells.iter().enumerate() {
            match admit(*value, *range) {
                Some(admitted) => {
                    time_series[base + offset] = admitted;
                    real += 1;
                }
                // A reading that exists and was discarded is a different fact from one that was
                // never taken, and the wearer is owed the difference. A non-finite value counts as
                // discarded: something was recorded, and it was not usable.
                None if value.is_some() => rejected = true,
                None => missing = true,
            }
        }

        let base = index * SCALAR_COLUMNS;
        scalars[base] = age;
        scalars[base + 1] = cycle_length;
        scalars[base + 2] = luteal_length;
        // One-based, counting up across the window — the archive's own numbering.
        scalars[base + 3] = (index + 1) as f32;
    }
    let mut health = InputHealth::of(real, CYCLE_DAYS * SERIES_COLUMNS);
    if rejected {
        health = health.substituting(Substitution::OutOfRange);
    }
    if missing {
        health = health.substituting(Substitution::Missing);
    }
    Ok(CycleInput {
        time_series,
        scalars,
        days: window.len(),
        health,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn vectors() -> Value {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../artifacts/models/vectors/popsicle_1_8_1.json"
        );
        serde_json::from_str(&std::fs::read_to_string(path).expect("popsicle vectors"))
            .expect("popsicle vectors parse")
    }

    /// The port against the archive that produced the contract.
    ///
    /// `tools/ml/popsicle_vectors.py` runs `popsicle_1_8_1.pt`'s own preprocessor; this holds the
    /// Rust to what came back, for every case including the out-of-range and missing ones. It is
    /// the only thing standing between a plausible column order and the right one.
    #[test]
    fn matches_the_archive_on_generated_vectors() {
        let vectors = vectors();
        for case in vectors["cases"].as_array().expect("cases") {
            let name = case["name"].as_str().expect("name");
            let inputs = &case["inputs"];
            let rows = inputs["series"].as_array().expect("series");
            let history: Vec<CycleDay> = rows
                .iter()
                .map(|row| {
                    let row = row.as_array().expect("row");
                    let value = |index: usize| row[index].as_f64().map(|value| value as f32);
                    CycleDay {
                        highest_temperature_c: value(0),
                        average_breath_rate: value(1),
                        average_heart_rate_bpm: value(2),
                    }
                })
                .collect();
            let profile = CycleProfile {
                age_years: inputs["age"].as_f64().map(|value| value as f32),
                typical_cycle_length_days: inputs["typical_cycle_length"]
                    .as_f64()
                    .map(|value| value as f32),
                typical_luteal_length_days: inputs["typical_luteal_length"]
                    .as_f64()
                    .map(|value| value as f32),
            };
            let built = cycle_input(&history, &profile).expect("build");

            let expected_series: Vec<f32> = case["sequences"]
                .as_array()
                .expect("sequences")
                .iter()
                .map(|value| value.as_f64().unwrap_or(0.0) as f32)
                .collect();
            let expected_scalars: Vec<f32> = case["statics"]
                .as_array()
                .expect("statics")
                .iter()
                .map(|value| value.as_f64().unwrap_or(0.0) as f32)
                .collect();

            // The archive emits exactly as many rows as the history has; the converted core takes
            // forty. Compare the real rows and require the rest to be padding.
            assert_eq!(
                &built.time_series[..expected_series.len()],
                &expected_series[..],
                "{name} series"
            );
            assert_eq!(
                &built.scalars[..expected_scalars.len()],
                &expected_scalars[..],
                "{name} scalars"
            );
            assert!(
                built.time_series[expected_series.len()..]
                    .iter()
                    .all(|v| *v == 0.0),
                "{name}: padding must be zero"
            );
            assert!(
                built.scalars[expected_scalars.len()..]
                    .iter()
                    .all(|v| *v == 0.0),
                "{name}: padding must be zero"
            );
        }
    }

    #[test]
    fn the_tensors_are_the_shapes_the_contract_declares() {
        let built = cycle_input(&[CycleDay::default()], &CycleProfile::default()).expect("build");
        assert_eq!(built.time_series.len(), CYCLE_DAYS * SERIES_COLUMNS);
        assert_eq!(built.scalars.len(), CYCLE_DAYS * SCALAR_COLUMNS);
    }

    #[test]
    fn a_history_longer_than_the_window_keeps_its_most_recent_days() {
        let mut history = vec![CycleDay::default(); CYCLE_DAYS + 5];
        history[CYCLE_DAYS + 4].average_heart_rate_bpm = Some(61.0);
        let built = cycle_input(&history, &CycleProfile::default()).expect("build");
        assert_eq!(built.days, CYCLE_DAYS);
        // The marked day is the newest, so it lands on the last real row.
        let last = (CYCLE_DAYS - 1) * SERIES_COLUMNS + 2;
        assert_eq!(built.time_series[last], 61.0);
    }

    #[test]
    fn an_empty_history_is_refused_rather_than_padded_into_a_prediction() {
        let error = cycle_input(&[], &CycleProfile::default()).expect_err("empty history");
        assert_eq!(error.code, codes::ML_MODEL_TENSOR_INVALID);
    }

    #[test]
    fn an_implausible_typed_figure_falls_back_rather_than_withholding_the_surface() {
        let profile = CycleProfile {
            age_years: Some(400.0),
            typical_cycle_length_days: Some(3.0),
            typical_luteal_length_days: Some(99.0),
        };
        let built = cycle_input(&[CycleDay::default()], &profile).expect("build");
        assert_eq!(built.scalars[0], DEFAULT_AGE_YEARS);
        assert_eq!(built.scalars[1], DEFAULT_CYCLE_LENGTH_DAYS);
        assert_eq!(built.scalars[2], DEFAULT_LUTEAL_LENGTH_DAYS);
    }

    /// A perfectly plausible luteal length still does not reach the model.
    ///
    /// The vectors prove the archive does this; this states it in one place so nobody "fixes" it
    /// back to passing the wearer's figure through.
    #[test]
    fn a_valid_luteal_length_is_still_replaced_by_the_population_default() {
        for entered in [8.0, 12.0, 14.0, 19.0] {
            let profile = CycleProfile {
                age_years: Some(30.0),
                typical_cycle_length_days: Some(28.0),
                typical_luteal_length_days: Some(entered),
            };
            let built = cycle_input(&[CycleDay::default()], &profile).expect("build");
            assert_eq!(
                built.scalars[2], DEFAULT_LUTEAL_LENGTH_DAYS,
                "entered {entered} reached the model"
            );
        }
    }
}

#[cfg(test)]
mod health_tests {
    use super::*;
    use crate::model_zoo::health::{Applicability, Substitution};

    fn day(temperature: f32) -> CycleDay {
        CycleDay {
            highest_temperature_c: Some(temperature),
            average_breath_rate: Some(15.0),
            average_heart_rate_bpm: Some(58.0),
        }
    }

    #[test]
    fn a_full_history_of_readings_is_sound() {
        let history = vec![day(36.5); CYCLE_DAYS];
        let built = cycle_input(&history, &CycleProfile::default()).expect("build");
        assert_eq!(built.health.real_fraction, 1.0);
        assert_eq!(built.health.applicability(), Applicability::Sound);
    }

    /// The case this whole module exists for.
    ///
    /// A wearer whose skin temperature sits outside `[35.5, 37.5]` — a different wear site reads
    /// cooler, and the archive's band was fitted on a finger — has every day rejected. The tensor
    /// is all zeros, the model still returns an ovulation probability, and without this the
    /// number is indistinguishable from one computed from a real month.
    #[test]
    fn a_history_the_archive_rejects_entirely_is_unfounded_not_merely_empty() {
        let history = vec![day(34.9); CYCLE_DAYS];
        let built = cycle_input(&history, &CycleProfile::default()).expect("build");
        // Only the temperature column is out of range; the other two survive untouched. That
        // asymmetry is the point — a rejection is per-column, so a wearer can lose one signal
        // and keep the rest, and the fraction has to reflect that rather than rounding to "no
        // data".
        assert_eq!(built.time_series[0], 0.0, "rejected temperature");
        assert_eq!(built.time_series[1], 15.0, "breath rate survives");
        assert_eq!(built.time_series[2], 58.0, "heart rate survives");
        assert!(
            (built.health.real_fraction - 2.0 / 3.0).abs() < 1e-6,
            "two of three columns are real, got {}",
            built.health.real_fraction
        );
        // Two of three columns still survive, so this is not zero overall — but the verdict has
        // to reflect that a third of every row is a discarded reading.
        assert!(built
            .health
            .substitutions
            .contains(&Substitution::OutOfRange));
        assert_ne!(built.health.applicability(), Applicability::Sound);
    }

    /// Every column rejected, which is the total case.
    #[test]
    fn a_history_rejected_on_every_column_is_unfounded() {
        let history = vec![
            CycleDay {
                highest_temperature_c: Some(34.0),
                average_breath_rate: Some(40.0),
                average_heart_rate_bpm: Some(200.0),
            };
            CYCLE_DAYS
        ];
        let built = cycle_input(&history, &CycleProfile::default()).expect("build");
        assert_eq!(built.health.real_fraction, 0.0);
        assert_eq!(built.health.applicability(), Applicability::Unfounded);
        assert!(
            !built.health.presentable(),
            "a prediction from an all-zero series must not reach a surface"
        );
    }

    /// A short history is padded, and the padding is counted honestly rather than being allowed
    /// to look like data.
    #[test]
    fn a_short_history_reports_the_padding_it_needed() {
        let history = vec![day(36.5); 4];
        let built = cycle_input(&history, &CycleProfile::default()).expect("build");
        assert_eq!(built.days, 4);
        assert!((built.health.real_fraction - 4.0 / CYCLE_DAYS as f32).abs() < 1e-6);
        assert_eq!(built.health.applicability(), Applicability::Unfounded);
        assert!(built.health.substitutions.contains(&Substitution::Missing));
    }

    /// Rejected and unrecorded days are reported apart, because the wearer can act on one and
    /// not the other.
    #[test]
    fn a_rejected_day_and_an_unrecorded_day_are_named_separately() {
        let mut history = vec![day(36.5); 20];
        history[0].highest_temperature_c = Some(39.0);
        history[1].average_breath_rate = None;
        let built = cycle_input(&history, &CycleProfile::default()).expect("build");
        assert!(built
            .health
            .substitutions
            .contains(&Substitution::OutOfRange));
        assert!(built.health.substitutions.contains(&Substitution::Missing));
    }
}

#[cfg(test)]
mod model_list_tests {
    use super::*;
    use crate::model_zoo::pipeline::{pipeline_of, FrontEnd};

    /// Every model this front-end claims to feed must actually take the tensors it builds.
    ///
    /// This is the guard against the mistake that was already made once here: all eight popsicle
    /// heads were marked as fed by `cycle_input`, and two of them take a nine-value vector this
    /// module does not produce.
    #[test]
    fn every_listed_model_takes_the_pair_this_module_builds() {
        for model in CYCLE_MODELS {
            let contract = model.contract();
            let series = contract
                .input("time_series")
                .unwrap_or_else(|| panic!("{} has no time_series input", contract.slug));
            let scalars = contract
                .input("scalars")
                .unwrap_or_else(|| panic!("{} has no scalars input", contract.slug));
            assert_eq!(series.element_count(), CYCLE_DAYS * SERIES_COLUMNS);
            assert_eq!(scalars.element_count(), CYCLE_DAYS * SCALAR_COLUMNS);
            assert_eq!(contract.inputs.len(), 2, "{}", contract.slug);
        }
    }

    /// And the pipeline table has to agree with this list in both directions.
    #[test]
    fn the_pipeline_marks_exactly_these_as_fed_by_this_front_end() {
        let declared: Vec<&str> = crate::model_zoo::pipeline::PIPELINE
            .iter()
            .filter(|entry| matches!(entry.front_end, FrontEnd::Ported("cycle::cycle_input")))
            .map(|entry| entry.model.contract().slug)
            .collect();
        let listed: Vec<&str> = CYCLE_MODELS
            .iter()
            .map(|model| model.contract().slug)
            .collect();
        assert_eq!(declared, listed);
        for model in CYCLE_MODELS {
            let entry = pipeline_of(*model).expect("every model has a pipeline row");
            assert!(matches!(
                entry.front_end,
                FrontEnd::Ported("cycle::cycle_input")
            ));
        }
    }

    /// The two heads this module cannot feed must not be in the list, and must still say why.
    #[test]
    fn the_min_follicular_heads_are_excluded_and_explain_themselves() {
        use crate::model_zoo::ModelId;
        for model in [
            ModelId::PopsicleMinFollicular,
            ModelId::PopsicleMinFollicularV16,
        ] {
            assert!(
                !CYCLE_MODELS.contains(&model),
                "{} takes a nine-value vector this module does not build",
                model.contract().slug
            );
            let entry = pipeline_of(model).expect("every model has a pipeline row");
            assert!(matches!(entry.front_end, FrontEnd::NotPorted(_, _)));
        }
    }
}
