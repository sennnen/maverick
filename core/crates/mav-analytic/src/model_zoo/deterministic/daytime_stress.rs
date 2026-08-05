//! Daytime stress sensing, ported from `stress_daytime_sensing` 1.1.0.
//!
//! One daytime HRV reading against the wearer's own baselines becomes a stress intensity, the
//! thresholds either side of the neutral zone, and a scaled level in `-1.0 ..= 1.0`.
//!
//! The two saturation tables are step functions of the *night* HRV baseline: someone whose
//! nights sit at 20 ms and someone at 100 ms do not get the same millisecond drop scored the
//! same way. Both tables are reproduced exactly, including that each returns the value for the
//! first limit the baseline falls *below*, and a fixed value when it falls below none.
//!
//! `-1.0` and `1.0` are saturation, not certainty. A scaled level says where this reading sits
//! between its own thresholds and its own saturation point; it carries no claim about how much
//! stress the wearer experienced.

use mav_model::version::Version;

pub const ALGORITHM: &str = "daytime_stress_sensing";
pub const VERSION: Version = Version::new(1, 1, 0);

/// Below this magnitude the scaled level is stretched linearly; above it, compressed. Both
/// constants are the archive's own.
const SCALED_LEVEL_LIMIT: f64 = 0.4;
const TARGET_LEVEL_LIMIT: f64 = 0.5;

/// `(limit, magnitude)`: the stress saturation is the negative of the first magnitude whose
/// limit the night baseline falls below.
const STRESS_SATURATION: [(f64, f64); 16] = [
    (10.0, 12.0),
    (15.0, 13.0),
    (20.0, 15.0),
    (25.0, 16.0),
    (30.0, 18.0),
    (35.0, 20.0),
    (40.0, 22.0),
    (45.0, 24.0),
    (50.0, 25.0),
    (55.0, 27.0),
    (60.0, 28.0),
    (65.0, 29.0),
    (70.0, 30.0),
    (75.0, 31.0),
    (90.0, 33.0),
    (120.0, 34.0),
];
const STRESS_SATURATION_FLOOR: f64 = -35.0;

const RECOVERY_SATURATION: [(f64, f64); 16] = [
    (15.0, 18.0),
    (20.0, 19.0),
    (25.0, 21.0),
    (30.0, 24.0),
    (35.0, 26.0),
    (40.0, 27.0),
    (45.0, 30.0),
    (50.0, 32.0),
    (55.0, 34.0),
    (60.0, 35.0),
    (65.0, 37.0),
    (70.0, 38.0),
    (75.0, 39.0),
    (95.0, 42.0),
    (110.0, 44.0),
    (120.0, 45.0),
];
const RECOVERY_SATURATION_CEILING: f64 = 46.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DaytimeStress {
    /// Raw difference from baseline, in the same unit as the HRV inputs.
    pub intensity: f64,
    pub stress_threshold: f64,
    pub recovery_threshold: f64,
    pub stress_saturation: f64,
    pub recovery_saturation: f64,
    /// `-1.0 ..= 1.0`, negative for stress.
    pub scaled_intensity: f64,
    pub scaled_stress_threshold: f64,
    pub scaled_recovery_threshold: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DaytimeStressError {
    NonFiniteInput,
}

/// Half-width of the neutral zone: the band around baseline that is neither stress nor recovery.
fn neutral_zone_half_width(night_hrv_baseline: f64) -> f64 {
    if night_hrv_baseline < 40.0 {
        2.0
    } else if night_hrv_baseline < 75.0 {
        3.0
    } else {
        4.0
    }
}

fn saturation(table: &[(f64, f64); 16], baseline: f64, fallback: f64, negate: bool) -> f64 {
    for (limit, magnitude) in table {
        if baseline < *limit {
            return if negate { -*magnitude } else { *magnitude };
        }
    }
    fallback
}

/// Map an intensity onto `-1.0 ..= 1.0`, clamping at each saturation point.
fn stress_scaler(intensity: f64, stress_saturation: f64, recovery_saturation: f64) -> f64 {
    if intensity < stress_saturation {
        -1.0
    } else if intensity < 0.0 {
        -intensity / stress_saturation
    } else if intensity > recovery_saturation {
        1.0
    } else {
        intensity / recovery_saturation
    }
}

/// Redistribute the scale so `SCALED_LEVEL_LIMIT` lands on `TARGET_LEVEL_LIMIT`.
///
/// Two straight lines rather than one: inside the limit the level is stretched, outside it the
/// remaining headroom is compressed into what is left. It keeps the mid-range readable without
/// letting the ends run past one.
fn equalize(level: f64) -> f64 {
    if (-SCALED_LEVEL_LIMIT..=SCALED_LEVEL_LIMIT).contains(&level) {
        level * (TARGET_LEVEL_LIMIT / SCALED_LEVEL_LIMIT)
    } else {
        let slope = TARGET_LEVEL_LIMIT / (1.0 - SCALED_LEVEL_LIMIT);
        level.signum() * (level.abs() * slope + (1.0 - slope))
    }
}

/// Score one daytime HRV reading against the wearer's own baselines.
pub fn daytime_stress(
    dhrv_value: f64,
    dhrv_baseline: f64,
    night_hrv_baseline: f64,
) -> Result<DaytimeStress, DaytimeStressError> {
    if !dhrv_value.is_finite() || !dhrv_baseline.is_finite() || !night_hrv_baseline.is_finite() {
        return Err(DaytimeStressError::NonFiniteInput);
    }

    let intensity = dhrv_value - dhrv_baseline;
    let recovery_threshold = neutral_zone_half_width(night_hrv_baseline);
    let stress_threshold = -recovery_threshold;
    let stress_saturation_value = saturation(
        &STRESS_SATURATION,
        night_hrv_baseline,
        STRESS_SATURATION_FLOOR,
        true,
    );
    let recovery_saturation_value = saturation(
        &RECOVERY_SATURATION,
        night_hrv_baseline,
        RECOVERY_SATURATION_CEILING,
        false,
    );

    let scaled_intensity = equalize(stress_scaler(
        intensity,
        stress_saturation_value,
        recovery_saturation_value,
    ));
    // The archive negates the stored negative threshold before dividing by a negative
    // saturation, which lands the scaled stress threshold back on the negative side.
    let scaled_stress_threshold = equalize(-stress_threshold / stress_saturation_value);
    let scaled_recovery_threshold = equalize(recovery_threshold / recovery_saturation_value);

    Ok(DaytimeStress {
        intensity,
        stress_threshold,
        recovery_threshold,
        stress_saturation: stress_saturation_value,
        recovery_saturation: recovery_saturation_value,
        scaled_intensity,
        scaled_stress_threshold,
        scaled_recovery_threshold,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Vectors generated by running `stress_daytime_sensing` 1.1.0 itself.
    /// `(dhrv, dhrv_baseline, night_hrv_baseline)` -> the eight outputs in contract order.
    const GOLDEN: [([f64; 3], [f64; 8]); 6] = [
        (
            [45.0, 40.0, 50.0],
            [
                5.0, -3.0, 3.0, -27.0, 34.0, 0.183_824, -0.138_889, 0.110_294,
            ],
        ),
        (
            [30.0, 40.0, 35.0],
            [
                -10.0, -2.0, 2.0, -22.0, 27.0, -0.545_455, -0.113_636, 0.092_593,
            ],
        ),
        (
            [60.0, 50.0, 80.0],
            [
                10.0, -4.0, 4.0, -33.0, 42.0, 0.297_619, -0.151_515, 0.119_048,
            ],
        ),
        (
            [20.0, 25.0, 20.0],
            [
                -5.0, -2.0, 2.0, -16.0, 21.0, -0.390_625, -0.156_25, 0.119_048,
            ],
        ),
        (
            [100.0, 60.0, 110.0],
            [
                40.0, -4.0, 4.0, -34.0, 45.0, 0.907_407, -0.147_059, 0.111_111,
            ],
        ),
        (
            [41.0, 40.0, 42.0],
            [1.0, -3.0, 3.0, -24.0, 30.0, 0.041_667, -0.156_25, 0.125],
        ),
    ];

    #[test]
    fn matches_the_reference_on_every_golden_vector() {
        for (inputs, expected) in GOLDEN {
            let got = daytime_stress(inputs[0], inputs[1], inputs[2]).expect("finite inputs");
            let actual = [
                got.intensity,
                got.stress_threshold,
                got.recovery_threshold,
                got.stress_saturation,
                got.recovery_saturation,
                got.scaled_intensity,
                got.scaled_stress_threshold,
                got.scaled_recovery_threshold,
            ];
            for (index, (a, e)) in actual.iter().zip(expected.iter()).enumerate() {
                assert!(
                    (a - e).abs() < 1e-6,
                    "input {inputs:?} output {index}: got {a}, reference {e}"
                );
            }
        }
    }

    #[test]
    fn the_neutral_zone_widens_in_three_steps() {
        assert_eq!(neutral_zone_half_width(39.999), 2.0);
        assert_eq!(neutral_zone_half_width(40.0), 3.0);
        assert_eq!(neutral_zone_half_width(74.999), 3.0);
        assert_eq!(neutral_zone_half_width(75.0), 4.0);
    }

    #[test]
    fn saturation_uses_the_first_limit_the_baseline_falls_below() {
        // 50 is below 55, not below 50, so it takes the 55 row.
        assert_eq!(
            saturation(&STRESS_SATURATION, 50.0, STRESS_SATURATION_FLOOR, true),
            -27.0
        );
        // Past every limit, the fixed floor applies rather than the last row.
        assert_eq!(
            saturation(&STRESS_SATURATION, 200.0, STRESS_SATURATION_FLOOR, true),
            STRESS_SATURATION_FLOOR
        );
        assert_eq!(
            saturation(
                &RECOVERY_SATURATION,
                200.0,
                RECOVERY_SATURATION_CEILING,
                false
            ),
            RECOVERY_SATURATION_CEILING
        );
    }

    #[test]
    fn the_scaled_level_saturates_and_never_leaves_the_unit_range() {
        // Far past stress saturation and far past recovery saturation.
        let stressed = daytime_stress(0.0, 500.0, 50.0).expect("finite");
        let recovered = daytime_stress(500.0, 0.0, 50.0).expect("finite");
        assert!(stressed.scaled_intensity >= -1.0 && stressed.scaled_intensity < 0.0);
        assert!(recovered.scaled_intensity <= 1.0 && recovered.scaled_intensity > 0.0);
        assert!((stressed.scaled_intensity + 1.0).abs() < 1e-12);
        assert!((recovered.scaled_intensity - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_reading_on_baseline_scores_zero() {
        let flat = daytime_stress(45.0, 45.0, 50.0).expect("finite");
        assert_eq!(flat.intensity, 0.0);
        assert_eq!(flat.scaled_intensity, 0.0);
    }

    #[test]
    fn a_non_finite_input_is_an_error_not_a_nan_output() {
        assert_eq!(
            daytime_stress(f64::NAN, 40.0, 50.0),
            Err(DaytimeStressError::NonFiniteInput)
        );
        assert_eq!(
            daytime_stress(45.0, 40.0, f64::INFINITY),
            Err(DaytimeStressError::NonFiniteInput)
        );
    }

    /// The same archive, run through the shared generator into a file, so this port is
    /// covered by the regenerable path every other deterministic port uses rather than only
    /// by the transcribed table above.
    ///
    /// The sleep-window and MET gates live in the wrapper, not in this function, so the
    /// vectors the archive refuses are checked for having refused rather than replayed.
    #[test]
    fn matches_the_generated_vectors() {
        let raw = include_str!(
            "../../../../../../artifacts/models/vectors/stress_daytime_sensing_1_1_0.json"
        );
        let file: serde_json::Value =
            serde_json::from_str(raw).expect("the vector file should parse");
        let mut checked = 0;
        for vector in file["vectors"]
            .as_array()
            .expect("vectors should be a list")
        {
            if vector.get("error").is_some() {
                continue;
            }
            let inputs = &vector["inputs"];
            let scalar = |name: &str| -> f64 {
                inputs[name].as_array().expect("a list")[0]
                    .as_f64()
                    .expect("a number")
            };
            let got = daytime_stress(
                scalar("dhrv_value"),
                scalar("dhrv_baseline"),
                scalar("night_hrv_baseline"),
            )
            .expect("the archive produced a result");
            let want = vector["outputs"].as_array().expect("outputs are a list");
            let expected = |index: usize| -> f64 {
                let mut value = &want[index];
                while let Some(items) = value.as_array() {
                    value = &items[0];
                }
                value.as_f64().expect("a number")
            };
            let fields = [
                ("intensity", got.intensity),
                ("stress threshold", got.stress_threshold),
                ("recovery threshold", got.recovery_threshold),
                ("stress saturation", got.stress_saturation),
                ("recovery saturation", got.recovery_saturation),
                ("scaled intensity", got.scaled_intensity),
                ("scaled stress threshold", got.scaled_stress_threshold),
                ("scaled recovery threshold", got.scaled_recovery_threshold),
            ];
            for (index, (name, value)) in fields.iter().enumerate() {
                let target = expected(index);
                assert!(
                    (value - target).abs() <= 1e-4 * target.abs().max(1.0),
                    "{name}: {value} vs {target}"
                );
            }
            checked += 1;
        }
        assert_eq!(
            checked, 4,
            "every vector the archive accepted should be checked"
        );
    }
}
