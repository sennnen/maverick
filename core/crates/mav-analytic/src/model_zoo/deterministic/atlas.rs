//! `atlas 2.1.0` — body composition from a bioimpedance measurement.
//!
//! Twelve linear regressions, six per sex, over five features: age, weight, height, the
//! height-squared-over-impedance term that is the standard bioimpedance predictor, and skin
//! temperature. Total body water, fat-free mass, skeletal muscle and bone mineral content come
//! out of the first four; the last two — the *targets* for muscle and bone — are read from
//! height alone, because a target is what the wearer's frame implies rather than what their
//! current composition is.
//!
//! It lives here rather than in a converted artefact for the same reason as the zero-parameter
//! archives: sixty coefficients across twelve dot products of at most five terms is not work
//! for an accelerator, and putting it in an `.mlpackage` would cost an artefact hash, an FFI
//! round trip and a place neither platform can golden-vector test it.
//!
//! **No supported device can produce its input.** The archive wants two 500-sample rows of
//! in-phase and quadrature response at one excitation frequency — a front end that injects a
//! known current and measures the complex voltage back. `docs/protocol/whoop-raw-afe.md`
//! enumerates the AFE from firmware strings: red, IR and green LEDs plus ambient, and a
//! single-lead ECG electrode, which senses voltage and does not excite. The port exists and is
//! tested; what it is waiting for is hardware, and `mav_analytic`'s capability negotiation is
//! where that is expressed.

/// Excitation frequencies the archive is configured for.
const FREQUENCIES: usize = 1;

/// Samples per row of the raw sweep.
pub const SWEEP_SAMPLES: usize = 500;

/// The sweep's sampling rate, in hertz.
const SAMPLING_HZ: f32 = 25.0;

/// How much of the tail of the sweep the settled value is taken over, in seconds.
const SETTLE_WINDOW_SECONDS: f32 = 3.0;

/// Width of the mode-finding smoothing kernel, in raw counts.
const MODE_BIN_WIDTH: f32 = 30.0;

/// Analogue-to-digital full scale, in counts.
const ADC_FULL_SCALE: f32 = 524_288.0;

/// The front end's gain and excitation current, in microamps RMS.
const GAIN: f32 = 5.0;
const RMS_CURRENT_UA: f32 = 32.0;

/// Basal metabolic rate from fat-free mass: the Katch-McArdle coefficients.
const BMR_PER_KG_FFM: f32 = 21.6;
const BMR_INTERCEPT: f32 = 370.0;

/// How fast historical agreement decays, in days.
const HISTORY_TAU: f32 = 21.0;

/// How much a past estimate's own confidence weighs.
const HISTORY_GAMMA: f32 = 1.0;

/// Expected drift in fat-free mass per thirty days, in kilograms.
const DRIFT_PER_MONTH: f32 = 2.0;

/// The model's own coefficient of variation.
const MODEL_CV: f32 = 0.017;

/// Confidence reported when there is no history to check against.
const NO_HISTORY_CONFIDENCE: f32 = 0.5;

/// Plausible percentage-body-fat range per sex, and how sharply it is enforced.
const PBF_RANGE_MALE: (f32, f32) = (3.0, 50.0);
const PBF_RANGE_FEMALE: (f32, f32) = (10.0, 60.0);
const PLAUSIBILITY_STEEPNESS: f32 = 2.0;

/// The male regression bank: four five-feature models then two height-only ones.
const MALE: [(&[f32], f32); 6] = [
    (
        &[-0.1268309, 0.20697017, 0.21843141, 0.6570256, -0.22805724],
        -14.72174,
    ),
    (
        &[-0.18187429, 0.28503355, 0.29152453, 0.88172436, -0.307702],
        -18.580856,
    ),
    (
        &[-0.13245097, 0.16773617, 0.14709812, 0.564559, -0.1747347],
        -8.379025,
    ),
    (
        &[
            0.0010377106,
            0.013802863,
            0.024538333,
            0.0029606535,
            0.0036291012,
        ],
        -2.0204966,
    ),
    (&[-0.052477997, 0.0012502134], 2.9322422),
    (&[-0.0033819359, 0.00011385879], 0.31166956),
];

/// The female bank, in the same order.
const FEMALE: [(&[f32], f32); 6] = [
    (
        &[-0.0675222, 0.079188116, 0.1775746, 1.0081433, -0.20549859],
        -11.049779,
    ),
    (
        &[-0.08512197, 0.11367532, 0.24364762, 1.343802, -0.2793285],
        -15.283245,
    ),
    (
        &[-0.05292452, 0.066639654, 0.12575132, 0.8383583, -0.16792741],
        -8.440548,
    ),
    (
        &[
            0.0042354055,
            0.0049207984,
            0.026888171,
            0.047214612,
            0.009611832,
        ],
        -3.307086,
    ),
    (&[-0.028127102, 0.0010695131], 0.28953597),
    (&[-0.010178265, 0.0001252225], 0.79298097),
];

/// The calibration the front end was characterised with, per frequency.
#[derive(Debug, Clone, Copy)]
pub struct Calibration {
    /// In-phase zero offset, in raw counts.
    pub in_phase_offset: f32,
    /// Quadrature zero offset, in raw counts.
    pub quadrature_offset: f32,
    /// Magnitude scale factor.
    pub magnitude_coefficient: f32,
    /// Phase offset, in degrees.
    pub phase_coefficient: f32,
}

/// Who the wearer is.
#[derive(Debug, Clone, Copy)]
pub struct Demographics {
    /// One for male, zero for female — the archive's own encoding.
    pub sex: f32,
    /// Years.
    pub age: f32,
    /// Kilograms.
    pub weight_kg: f32,
    /// Centimetres.
    pub height_cm: f32,
}

/// One earlier estimate, for the consistency check.
#[derive(Debug, Clone, Copy)]
pub struct PriorEstimate {
    /// The fat-free mass estimated then, in kilograms.
    pub fat_free_mass_kg: f32,
    /// How many days ago it was taken.
    pub days_ago: f32,
    /// The confidence it carried.
    pub confidence: f32,
}

/// What the archive returns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BodyComposition {
    /// Total body water, in kilograms.
    pub total_body_water_kg: f32,
    /// Fat-free mass, in kilograms.
    pub fat_free_mass_kg: f32,
    /// Skeletal muscle mass, in kilograms.
    pub skeletal_muscle_mass_kg: f32,
    /// Bone mineral content, in kilograms.
    pub bone_mineral_content_kg: f32,
    /// Fat mass, in kilograms.
    pub fat_mass_kg: f32,
    /// Percentage body fat.
    pub percent_body_fat: f32,
    /// Basal metabolic rate, in kilocalories per day.
    pub basal_metabolic_rate: f32,
    /// The skeletal muscle mass this frame implies.
    pub skeletal_muscle_target_kg: f32,
    /// The bone mineral content this frame implies.
    pub bone_mineral_target_kg: f32,
    /// How much the estimate should be believed, from zero to one.
    pub confidence: f32,
    /// The calibrated impedance magnitude, in ohms.
    pub impedance_ohms: f32,
    /// The calibrated phase, in degrees.
    pub phase_degrees: f32,
}

/// Why the archive refused the input, with the code it refuses under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtlasError {
    /// 103 — the sweep is not two rows of 500 samples.
    SweepWrongShape,
    /// 503 — the calibration does not cover every frequency.
    CalibrationWrongShape,
}

impl AtlasError {
    /// The archive's own code for this refusal.
    pub fn code(self) -> u16 {
        match self {
            Self::SweepWrongShape => 103,
            Self::CalibrationWrongShape => 503,
        }
    }
}

/// The mode of the sweep's tail, which is where the response has settled.
///
/// A mean would be dragged by the transient at the start of the window and a median by any
/// sustained drift, so the archive histograms the tail at a tenth of the smoothing width,
/// smooths the counts with a boxcar, and takes the bin edge under the peak.
fn settled_value(row: &[f32]) -> f32 {
    let window = (SAMPLING_HZ * SETTLE_WINDOW_SECONDS) as usize;
    let segment = &row[row.len().saturating_sub(window)..];
    let bin = MODE_BIN_WIDTH / 10.0;
    let low = segment.iter().copied().fold(f32::INFINITY, f32::min) - MODE_BIN_WIDTH;
    let high = segment.iter().copied().fold(f32::NEG_INFINITY, f32::max) + MODE_BIN_WIDTH;
    // `arange` computes each edge as `low + index * bin` rather than accumulating, and over
    // several hundred bins the two drift apart by enough to move the peak by one.
    let edge_count = (((high + bin) - low) / bin).ceil().max(0.0) as usize;
    let edges: Vec<f32> = (0..edge_count)
        .map(|index| low + index as f32 * bin)
        .collect();
    let bins = edges.len().saturating_sub(1);
    if bins == 0 {
        return low;
    }
    let mut counts = vec![0.0f32; bins];
    for value in segment {
        // The histogram's last bin is closed at the top, as `torch.histc`'s is.
        let position = (value - low) / (high - low) * bins as f32;
        let index = (position.floor() as isize).clamp(0, bins as isize - 1) as usize;
        counts[index] += 1.0;
    }
    let kernel = ((MODE_BIN_WIDTH / bin) as usize).max(1);
    let pad = (kernel - 1) / 2;
    let mut padded = vec![0.0f32; pad];
    padded.extend_from_slice(&counts);
    padded.extend(core::iter::repeat_n(0.0f32, pad));
    let smoothed: Vec<f32> = padded
        .windows(kernel)
        .map(|window| window.iter().sum())
        .collect();
    // The *first* maximum, as `argmax` reports it. A boxcar over a histogram plateaus often,
    // and taking the last one instead moves the settled value by most of a kernel width.
    let mut peak = 0;
    for (index, value) in smoothed.iter().enumerate() {
        if *value > smoothed[peak] {
            peak = index;
        }
    }
    edges.get(peak).copied().unwrap_or(low)
}

/// Raw counts to ohms and degrees.
fn calibrate(in_phase: f32, quadrature: f32, calibration: &Calibration) -> (f32, f32) {
    let corrected_i = in_phase - calibration.in_phase_offset;
    let corrected_q = quadrature - calibration.quadrature_offset;
    // The front end's transfer function: full scale, gain, the two from the differential
    // pair, the RMS-to-peak factor, and the excitation current in amps.
    let counts_per_ohm = core::f32::consts::PI.recip()
        * (ADC_FULL_SCALE * GAIN * 2.0)
        * core::f32::consts::SQRT_2
        * RMS_CURRENT_UA
        * 1e-6;
    let ohms_i = corrected_i / counts_per_ohm;
    let ohms_q = corrected_q / counts_per_ohm;
    let magnitude = (ohms_i * ohms_i + ohms_q * ohms_q).sqrt();
    let phase = ohms_q.atan2(ohms_i) * 180.0 / core::f32::consts::PI;
    (
        magnitude / calibration.magnitude_coefficient,
        phase - calibration.phase_coefficient,
    )
}

fn predict(bank: &[(&[f32], f32); 6], features: &[f32], normative: &[f32]) -> [f32; 6] {
    let mut out = [0.0f32; 6];
    for (slot, (weights, bias)) in bank.iter().enumerate() {
        // The last two models read the *normative* features — height and height squared —
        // because a target is what the frame implies, not what the body currently is.
        let inputs = if weights.len() == normative.len() {
            normative
        } else {
            features
        };
        out[slot] = weights
            .iter()
            .zip(inputs)
            .map(|(weight, value)| weight * value)
            .sum::<f32>()
            + bias;
    }
    out
}

/// How well this estimate agrees with the wearer's own recent history.
fn historical_consistency(fat_free_mass: f32, history: &[PriorEstimate]) -> f32 {
    let usable: Vec<&PriorEstimate> = history
        .iter()
        .filter(|entry| {
            !entry.fat_free_mass_kg.is_nan()
                && !entry.days_ago.is_nan()
                && !entry.confidence.is_nan()
        })
        .collect();
    if usable.is_empty() {
        return NO_HISTORY_CONFIDENCE;
    }
    // Each past estimate counts for its own confidence, decayed by how long ago it was.
    let weights: Vec<f32> = usable
        .iter()
        .map(|entry| entry.confidence.powf(HISTORY_GAMMA) * (-entry.days_ago / HISTORY_TAU).exp())
        .collect();
    let weight_sum: f32 = weights.iter().sum();
    let predicted: f32 = usable
        .iter()
        .zip(&weights)
        .map(|(entry, weight)| weight * entry.fat_free_mass_kg)
        .sum::<f32>()
        / weight_sum;
    let history_variance = if usable.len() >= 2 {
        usable
            .iter()
            .zip(&weights)
            .map(|(entry, weight)| weight * (entry.fat_free_mass_kg - predicted).powi(2))
            .sum::<f32>()
            / weight_sum
    } else {
        0.0
    };
    let effective_days: f32 = usable
        .iter()
        .zip(&weights)
        .map(|(entry, weight)| weight * entry.days_ago)
        .sum::<f32>()
        / weight_sum;
    let drift = effective_days * DRIFT_PER_MONTH / 30.0;
    // With one or two prior estimates the model's own error is doubled, because there is not
    // enough history to have measured the wearer's spread.
    let model_sigma = fat_free_mass * MODEL_CV * if usable.len() <= 2 { 2.0 } else { 1.0 };
    let sigma = (history_variance + drift * drift + model_sigma * model_sigma).sqrt();
    let z = (fat_free_mass - predicted) / sigma;
    (-0.5 * z * z).exp()
}

/// Whether the percentage body fat is physiologically plausible for this wearer.
fn plausibility(percent_body_fat: f32, sex: f32) -> f32 {
    let (low, high) = (
        sex * PBF_RANGE_MALE.0 + (1.0 - sex) * PBF_RANGE_FEMALE.0,
        sex * PBF_RANGE_MALE.1 + (1.0 - sex) * PBF_RANGE_FEMALE.1,
    );
    let sigmoid = |x: f32| 1.0 / (1.0 + (-x).exp());
    sigmoid((percent_body_fat - low) * PLAUSIBILITY_STEEPNESS)
        * sigmoid((percent_body_fat - high) * -PLAUSIBILITY_STEEPNESS)
}

/// Estimate body composition from one bioimpedance sweep.
pub fn body_composition(
    sweep: &[[f32; SWEEP_SAMPLES]],
    skin_temperature: f32,
    demographics: &Demographics,
    calibration: &[Calibration],
    history: &[PriorEstimate],
) -> Result<BodyComposition, AtlasError> {
    if sweep.len() != FREQUENCIES * 2 {
        return Err(AtlasError::SweepWrongShape);
    }
    if calibration.len() != FREQUENCIES {
        return Err(AtlasError::CalibrationWrongShape);
    }

    let in_phase = settled_value(&sweep[0]);
    let quadrature = settled_value(&sweep[1]);
    let (impedance, phase) = calibrate(in_phase, quadrature, &calibration[0]);

    let height_squared = demographics.height_cm * demographics.height_cm;
    let features = [
        demographics.age,
        demographics.weight_kg,
        demographics.height_cm,
        height_squared / impedance,
        skin_temperature,
    ];
    let normative = [demographics.height_cm, height_squared];
    let bank = if demographics.sex == 1.0 {
        &MALE
    } else {
        &FEMALE
    };
    let predictions = predict(bank, &features, &normative);

    let fat_free_mass = predictions[1];
    let fat_mass = demographics.weight_kg - fat_free_mass;
    let percent_body_fat = fat_mass / demographics.weight_kg * 100.0;
    Ok(BodyComposition {
        total_body_water_kg: predictions[0],
        fat_free_mass_kg: fat_free_mass,
        skeletal_muscle_mass_kg: predictions[2],
        bone_mineral_content_kg: predictions[3],
        fat_mass_kg: fat_mass,
        percent_body_fat,
        basal_metabolic_rate: fat_free_mass * BMR_PER_KG_FFM + BMR_INTERCEPT,
        skeletal_muscle_target_kg: predictions[4],
        bone_mineral_target_kg: predictions[5],
        // Both halves must hold: an estimate that agrees with history but is physiologically
        // impossible is not a confident one, and neither is the reverse.
        confidence: historical_consistency(fat_free_mass, history)
            * plausibility(percent_body_fat, demographics.sex),
        impedance_ohms: impedance,
        phase_degrees: phase,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// float32 through decimal. Tight on purpose: the confidence figure divides by a sigma
    /// of order one kilogram, so a fat-free mass loose by even a tenth of a percent shows up
    /// there — which is how the settled value's tie-breaking bug was found.
    const TOLERANCE: f32 = 1e-4;

    fn calibration() -> Vec<Calibration> {
        vec![Calibration {
            in_phase_offset: 500.0,
            quadrature_offset: -200.0,
            magnitude_coefficient: 1.05,
            phase_coefficient: 3.0,
        }]
    }

    #[test]
    fn the_settled_value_is_the_mode_of_the_tail_not_its_mean() {
        // The first half drifts; the tail settles. A mean over the whole row would land
        // between the two, and the mode of the tail should not.
        let mut row = [0.0f32; SWEEP_SAMPLES];
        for (index, slot) in row.iter_mut().enumerate() {
            *slot = if index < SWEEP_SAMPLES - 75 {
                200_000.0
            } else {
                120_000.0
            };
        }
        let settled = settled_value(&row);
        assert!(
            (settled - 120_000.0).abs() < MODE_BIN_WIDTH * 2.0,
            "settled on {settled}"
        );
    }

    #[test]
    fn the_two_target_models_read_height_rather_than_the_impedance_term() {
        // Changing the impedance moves the four body models and must leave the targets alone.
        let bank = &MALE;
        let features = [34.0, 82.0, 180.0, 400.0, 33.5];
        let mut other = features;
        other[3] = 500.0;
        let normative = [180.0, 180.0 * 180.0];
        let first = predict(bank, &features, &normative);
        let second = predict(bank, &other, &normative);
        assert_ne!(first[0], second[0], "total body water should move");
        assert_eq!(first[4], second[4], "the muscle target should not");
        assert_eq!(first[5], second[5], "nor the bone target");
    }

    #[test]
    fn without_history_confidence_falls_back_to_a_half() {
        assert_eq!(historical_consistency(60.0, &[]), NO_HISTORY_CONFIDENCE);
        let missing = [PriorEstimate {
            fat_free_mass_kg: f32::NAN,
            days_ago: 3.0,
            confidence: 0.8,
        }];
        assert_eq!(
            historical_consistency(60.0, &missing),
            NO_HISTORY_CONFIDENCE
        );
    }

    #[test]
    fn an_estimate_that_agrees_with_history_scores_higher_than_one_that_does_not() {
        let history: Vec<PriorEstimate> = (0..5)
            .map(|index| PriorEstimate {
                fat_free_mass_kg: 60.0,
                days_ago: index as f32 * 3.0,
                confidence: 0.8,
            })
            .collect();
        let agreeing = historical_consistency(60.0, &history);
        let disagreeing = historical_consistency(75.0, &history);
        assert!(agreeing > disagreeing, "{agreeing} vs {disagreeing}");
        assert!(agreeing > 0.9, "an exact match should score near one");
    }

    #[test]
    fn implausible_body_fat_is_pushed_towards_zero_confidence_per_sex() {
        // 5% is plausible for a man and not for a woman.
        assert!(plausibility(5.0, 1.0) > 0.9);
        assert!(plausibility(5.0, 0.0) < 0.1);
        // 55% is the reverse.
        assert!(plausibility(55.0, 1.0) < 0.1);
        assert!(plausibility(55.0, 0.0) > 0.9);
    }

    #[test]
    fn refuses_a_sweep_or_calibration_of_the_wrong_shape() {
        let sweep = vec![[0.0f32; SWEEP_SAMPLES]];
        let demographics = Demographics {
            sex: 1.0,
            age: 34.0,
            weight_kg: 82.0,
            height_cm: 180.0,
        };
        assert_eq!(
            body_composition(&sweep, 33.5, &demographics, &calibration(), &[]),
            Err(AtlasError::SweepWrongShape)
        );
        let sweep = vec![[0.0f32; SWEEP_SAMPLES]; 2];
        assert_eq!(
            body_composition(&sweep, 33.5, &demographics, &[], &[]),
            Err(AtlasError::CalibrationWrongShape)
        );
    }

    /// Vectors generated by `tools/ml/deterministic_vectors.py atlas_2_1_0`.
    #[test]
    fn matches_the_archive_on_generated_vectors() {
        let raw = include_str!("../../../../../../artifacts/models/vectors/atlas_2_1_0.json");
        let file: serde_json::Value =
            serde_json::from_str(raw).expect("the vector file should parse");
        let mut checked = 0;
        for vector in file["vectors"]
            .as_array()
            .expect("vectors should be a list")
        {
            let inputs = &vector["inputs"];
            let rows = inputs["bioz_signals"].as_array().expect("two rows");
            let mut sweep = vec![[0.0f32; SWEEP_SAMPLES]; rows.len()];
            for (index, row) in rows.iter().enumerate() {
                for (slot, value) in row.as_array().expect("a row").iter().enumerate() {
                    sweep[index][slot] = value.as_f64().expect("a sample") as f32;
                }
            }
            let flat = |name: &str| -> Vec<f32> {
                inputs[name]
                    .as_array()
                    .expect("a list")
                    .iter()
                    .map(|value| value.as_f64().map_or(f32::NAN, |v| v as f32))
                    .collect()
            };
            let demographics = flat("demographics");
            let coefficients = inputs["calibration_coeffs"].as_array().expect("a matrix")[0]
                .as_array()
                .expect("a row")
                .iter()
                .map(|value| value.as_f64().expect("a coefficient") as f32)
                .collect::<Vec<f32>>();
            let history: Vec<PriorEstimate> = inputs["historical_data"]
                .as_array()
                .expect("a matrix")
                .iter()
                .map(|row| {
                    let row = row.as_array().expect("a row");
                    let read =
                        |index: usize| row[index].as_f64().map_or(f32::NAN, |value| value as f32);
                    PriorEstimate {
                        fat_free_mass_kg: read(0),
                        days_ago: read(1),
                        confidence: read(2),
                    }
                })
                .collect();
            let got = body_composition(
                &sweep,
                flat("temperature")[0],
                &Demographics {
                    sex: demographics[0],
                    age: demographics[1],
                    weight_kg: demographics[2],
                    height_cm: demographics[3],
                },
                &[Calibration {
                    in_phase_offset: coefficients[0],
                    quadrature_offset: coefficients[1],
                    magnitude_coefficient: coefficients[2],
                    phase_coefficient: coefficients[3],
                }],
                &history,
            )
            .expect("the archive accepted this input");

            let want = vector["outputs"].as_array().expect("outputs are a list");
            fn scalar(value: &serde_json::Value) -> f32 {
                match value {
                    serde_json::Value::Array(items) => scalar(&items[0]),
                    serde_json::Value::Number(number) => number.as_f64().expect("a number") as f32,
                    _ => f32::NAN,
                }
            }
            let close = |name: &str, got: f32, index: usize| {
                let expected = scalar(&want[index]);
                assert!(
                    (got - expected).abs() <= TOLERANCE * expected.abs().max(1.0),
                    "{name}: {got} vs {expected}"
                );
            };
            close("total body water", got.total_body_water_kg, 0);
            close("fat-free mass", got.fat_free_mass_kg, 1);
            close("skeletal muscle", got.skeletal_muscle_mass_kg, 2);
            close("bone mineral", got.bone_mineral_content_kg, 3);
            close("fat mass", got.fat_mass_kg, 4);
            close("percent body fat", got.percent_body_fat, 5);
            close("basal metabolic rate", got.basal_metabolic_rate, 6);
            close("muscle target", got.skeletal_muscle_target_kg, 7);
            close("bone target", got.bone_mineral_target_kg, 8);
            close("confidence", got.confidence, 9);
            close("impedance", got.impedance_ohms, 10);
            close("phase", got.phase_degrees, 11);
            checked += 1;
        }
        assert_eq!(checked, 5, "every generated vector should be checked");
    }
}
