//! `cva_calibrator 1.3.0` — the offset that makes cardiovascular age comparable over time.
//!
//! The predictor produces a cardiovascular age from one day's pulse morphology. That number
//! is not stable enough to show on its own: it moves with how the ring sat, and it jumps when
//! the hardware changes, because a different unit measures the same wrist slightly
//! differently. This archive holds the offset that absorbs the jump, plus the cubic that maps
//! a calibrated age to a pulse-wave velocity.
//!
//! The offset is derived once, from the *previous* hardware's smoothed reading: the wearer's
//! age did not change when their ring did, so whatever the new unit reads today should be
//! moved to meet what the old one was reading. After enough days have accumulated the offset
//! is frozen at the median of what it has been, and from then on it is reused rather than
//! recomputed — an offset that keeps drifting is not an offset.
//!
//! Three windows govern all of it, and they are not the same window:
//!
//!   * **thirty days** of readings on the current hardware are what smoothing looks at;
//!   * **fourteen readings** are the minimum before a smoothed value exists at all;
//!   * **fourteen non-zero offsets** on the current hardware are what freezes it.
//!
//! Arithmetic is `f32` throughout, as the archive's is.

use super::torch_median;

/// Cubic coefficients mapping calibrated cardiovascular age to pulse-wave velocity, as
/// `a·x + b·x² + c·x³ + d`. The curves differ by sex and the archive carries both.
const MALE_CURVE: [f32; 4] = [0.161_263_63, -0.002_029_558_2, 1.440_364e-5, 3.0];
const FEMALE_CURVE: [f32; 4] = [0.169_376_78, -0.002_773_610_9, 2.159_522_4e-5, 3.0];

/// How far back smoothing and the hardware-change test look, in seconds.
const LOOKBACK_SECONDS: f32 = 2_592_000.0;

/// How many readings the rolling median over the previous hardware needs before it counts.
const DAYS_REQUIRED_FOR_OFFSET: usize = 14;

/// The most readings the smoothed value is taken over, newest first.
const SMOOTHING_WINDOW: usize = 30;

/// The fewest readings before a smoothed value exists at all.
const MIN_READINGS_TO_SMOOTH: usize = 14;

/// Which curve to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sex {
    /// The male curve.
    Male,
    /// The female curve.
    Female,
    /// Unspecified, which the archive routes to the female curve.
    Unspecified,
}

impl Sex {
    fn curve(self) -> [f32; 4] {
        // The archive tests only for equality with its male sentinel, so everything else —
        // including the unspecified value — takes the female curve. Reproduced rather than
        // tidied, because a wearer who did not answer would otherwise get a different number
        // here than the archive gives them.
        match self {
            Self::Male => MALE_CURVE,
            Self::Female | Self::Unspecified => FEMALE_CURVE,
        }
    }
}

/// Why the archive refused the input, with the code it refuses under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibratorError {
    /// 130 — the daily value is absent or missing.
    DailyValueMissing,
    /// 131 — the hardware-change timestamp is absent or missing.
    HardwareChangeMissing,
    /// 132 — a history timestamp is missing.
    TimestampMissing,
    /// 133 — the history timestamps are not in order.
    TimestampsNotSorted,
    /// 134 — a historical calibrated value is missing.
    HistoryMissing,
    /// 135 — an offset is missing.
    OffsetMissing,
    /// 136 — a freeze flag is missing.
    FreezeFlagMissing,
    /// 137 — a freeze flag is neither zero nor one.
    FreezeFlagNotBoolean,
    /// 140 — the four history columns are different lengths.
    HistoryLengthMismatch,
}

impl CalibratorError {
    /// The archive's own code for this refusal.
    pub fn code(self) -> u16 {
        match self {
            Self::DailyValueMissing => 130,
            Self::HardwareChangeMissing => 131,
            Self::TimestampMissing => 132,
            Self::TimestampsNotSorted => 133,
            Self::HistoryMissing => 134,
            Self::OffsetMissing => 135,
            Self::FreezeFlagMissing => 136,
            Self::FreezeFlagNotBoolean => 137,
            Self::HistoryLengthMismatch => 140,
        }
    }
}

/// One day's history row.
#[derive(Debug, Clone, Copy)]
pub struct HistoryRow {
    /// When the reading was taken, in Unix seconds.
    pub timestamp: f32,
    /// The calibrated cardiovascular age recorded that day.
    pub calibrated_value: f32,
    /// The offset in force that day.
    pub offset: f32,
    /// Whether that day's offset was frozen.
    pub offset_frozen: bool,
}

/// What the calibrator returns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Calibration {
    /// Today's cardiovascular age with the offset applied.
    pub calibrated_value: f32,
    /// The offset that was applied.
    pub offset: f32,
    /// Whether that offset is now frozen.
    pub offset_frozen: bool,
    /// Pulse-wave velocity from the calibrated value.
    pub pulse_wave_velocity: f32,
    /// The smoothed cardiovascular age over recent history, absent when there is too little.
    pub smoothed_value: Option<f32>,
    /// Pulse-wave velocity from the smoothed value, absent for the same reason.
    pub smoothed_pulse_wave_velocity: Option<f32>,
}

/// The cubic mapping a cardiovascular age to a pulse-wave velocity.
fn pulse_wave_velocity(value: f32, sex: Sex) -> f32 {
    let [a, b, c, d] = sex.curve();
    a * value + b * value * value + c * value * value * value + d
}

fn validate(
    daily_value: f32,
    hardware_change: f32,
    history: &[HistoryRow],
) -> Result<(), CalibratorError> {
    if daily_value.is_nan() {
        return Err(CalibratorError::DailyValueMissing);
    }
    if hardware_change.is_nan() {
        return Err(CalibratorError::HardwareChangeMissing);
    }
    if history.iter().any(|row| row.timestamp.is_nan()) {
        return Err(CalibratorError::TimestampMissing);
    }
    if history
        .windows(2)
        .any(|pair| pair[1].timestamp < pair[0].timestamp)
    {
        return Err(CalibratorError::TimestampsNotSorted);
    }
    if history.iter().any(|row| row.calibrated_value.is_nan()) {
        return Err(CalibratorError::HistoryMissing);
    }
    if history.iter().any(|row| row.offset.is_nan()) {
        return Err(CalibratorError::OffsetMissing);
    }
    Ok(())
}

/// The last rolling median over the previous hardware's readings, if any window qualifies.
///
/// Each reading gets a thirty-day window ending on itself; a window holding at least fourteen
/// readings contributes its median, and the *last* such median is the one carried forward.
fn previous_hardware_smoothed(history: &[HistoryRow], hardware_change: f32) -> Option<f32> {
    let previous: Vec<&HistoryRow> = history
        .iter()
        .filter(|row| row.timestamp < hardware_change)
        .collect();
    let mut latest = None;
    for anchor in &previous {
        let window: Vec<f64> = previous
            .iter()
            .filter(|row| {
                row.timestamp >= anchor.timestamp - LOOKBACK_SECONDS
                    && row.timestamp <= anchor.timestamp
            })
            .map(|row| f64::from(row.calibrated_value))
            .collect();
        if window.len() >= DAYS_REQUIRED_FOR_OFFSET {
            latest = torch_median(&window).map(|value| value as f32);
        }
    }
    latest
}

/// Calibrate today's cardiovascular age against the wearer's history.
pub fn calibrate(
    daily_value: f32,
    hardware_change: f32,
    history: &[HistoryRow],
    sex: Sex,
    freeze_offset_days: usize,
    reset_baseline: bool,
) -> Result<Calibration, CalibratorError> {
    validate(daily_value, hardware_change, history)?;

    let uncalibrated_velocity = pulse_wave_velocity(daily_value, sex);
    if history.is_empty() {
        return Ok(Calibration {
            calibrated_value: daily_value,
            offset: 0.0,
            offset_frozen: false,
            pulse_wave_velocity: uncalibrated_velocity,
            smoothed_value: None,
            smoothed_pulse_wave_velocity: None,
        });
    }

    let now = history[history.len() - 1].timestamp;
    let on_current_hardware = |row: &HistoryRow| row.timestamp >= hardware_change;

    // Smoothing looks only at the current hardware's last thirty days, and only once there
    // are fourteen readings to smooth.
    let recent: Vec<&HistoryRow> = history
        .iter()
        .filter(|row| on_current_hardware(row) && row.timestamp >= now - LOOKBACK_SECONDS)
        .collect();
    let smoothed_value = if recent.len() >= MIN_READINGS_TO_SMOOTH {
        let tail: Vec<f64> = recent[recent.len().saturating_sub(SMOOTHING_WINDOW)..]
            .iter()
            .map(|row| f64::from(row.calibrated_value))
            .collect();
        torch_median(&tail).map(|value| value as f32)
    } else {
        None
    };
    let smoothed_velocity = smoothed_value.map(|value| pulse_wave_velocity(value, sex));

    if reset_baseline {
        return Ok(Calibration {
            calibrated_value: daily_value,
            offset: 0.0,
            offset_frozen: false,
            pulse_wave_velocity: uncalibrated_velocity,
            smoothed_value,
            smoothed_pulse_wave_velocity: smoothed_velocity,
        });
    }

    // An offset can only be derived while the previous hardware is still inside the lookback;
    // once it falls out there is nothing left to align to and the offset stays where it is.
    let carried = if hardware_change >= now - LOOKBACK_SECONDS {
        previous_hardware_smoothed(history, hardware_change)
    } else {
        None
    };

    // The first reading on new hardware cannot be frozen: there is nothing behind it to have
    // frozen it. The archive clears the flag in place, which matters because the next branch
    // reads it back.
    let current_count = history
        .iter()
        .filter(|row| on_current_hardware(row))
        .count();
    let last_frozen = history[history.len() - 1].offset_frozen && current_count != 1;

    let (offset, offset_frozen) = if last_frozen {
        (history[history.len() - 1].offset, true)
    } else if let Some(previous) = carried {
        let current_offsets: Vec<f32> = history
            .iter()
            .filter(|row| on_current_hardware(row))
            .map(|row| row.offset)
            .filter(|offset| *offset != 0.0)
            .collect();
        if freeze_offset_days <= current_offsets.len() {
            // Enough days have agreed on an offset. Freeze it at the median of the first
            // `freeze_offset_days` of them, so later drift cannot move it again.
            let head: Vec<f64> = current_offsets[..freeze_offset_days]
                .iter()
                .map(|offset| f64::from(*offset))
                .collect();
            (
                torch_median(&head).map(|value| value as f32).unwrap_or(0.0),
                true,
            )
        } else {
            (previous - daily_value, false)
        }
    } else {
        (0.0, false)
    };

    let calibrated_value = daily_value + offset;
    Ok(Calibration {
        calibrated_value,
        offset,
        offset_frozen,
        pulse_wave_velocity: pulse_wave_velocity(calibrated_value, sex),
        smoothed_value,
        smoothed_pulse_wave_velocity: smoothed_velocity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: f32 = 86_400.0;
    const BASE: f32 = 1_700_000_000.0;
    /// The archive computes in float32 and the vectors are written through decimal.
    const TOLERANCE: f32 = 2e-4;

    fn history(count: usize, offset: f32, frozen_last: bool) -> Vec<HistoryRow> {
        (0..count)
            .map(|index| HistoryRow {
                timestamp: BASE + index as f32 * DAY,
                calibrated_value: 45.0 + (index % 5) as f32 * 0.3,
                offset,
                offset_frozen: frozen_last && index + 1 == count,
            })
            .collect()
    }

    #[test]
    fn without_history_the_daily_value_passes_through_uncalibrated() {
        let got = calibrate(45.0, BASE - DAY, &[], Sex::Male, 14, false).expect("valid");
        assert_eq!(got.offset, 0.0);
        assert!(!got.offset_frozen);
        assert_eq!(got.calibrated_value, 45.0);
        assert!(got.smoothed_value.is_none());
    }

    #[test]
    fn smoothing_needs_fourteen_readings() {
        let short = history(13, 0.0, false);
        assert!(calibrate(45.0, BASE - DAY, &short, Sex::Male, 14, false)
            .expect("valid")
            .smoothed_value
            .is_none());
        let long = history(14, 0.0, false);
        assert!(calibrate(45.0, BASE - DAY, &long, Sex::Male, 14, false)
            .expect("valid")
            .smoothed_value
            .is_some());
    }

    #[test]
    fn the_two_sexes_take_different_curves_and_unspecified_takes_the_female_one() {
        let male = pulse_wave_velocity(45.0, Sex::Male);
        let female = pulse_wave_velocity(45.0, Sex::Female);
        assert!((male - female).abs() > 0.1, "the curves should differ");
        assert_eq!(
            pulse_wave_velocity(45.0, Sex::Unspecified),
            female,
            "unspecified follows the archive's else-branch"
        );
    }

    #[test]
    fn a_frozen_offset_is_reused_rather_than_recomputed() {
        let rows = history(40, 2.5, true);
        let got = calibrate(45.0, BASE, &rows, Sex::Male, 14, false).expect("valid");
        assert_eq!(got.offset, 2.5);
        assert!(got.offset_frozen);
        assert_eq!(got.calibrated_value, 47.5);

        // The one reading that cannot be frozen is the first on new hardware: there is
        // nothing behind it that could have frozen anything.
        let single = vec![rows[rows.len() - 1]];
        let got =
            calibrate(45.0, single[0].timestamp, &single, Sex::Male, 14, false).expect("valid");
        assert!(!got.offset_frozen);
        assert_eq!(got.offset, 0.0);
    }

    #[test]
    fn a_baseline_reset_discards_the_offset() {
        let rows = history(30, 2.5, true);
        let got = calibrate(45.0, BASE, &rows, Sex::Male, 14, true).expect("valid");
        assert_eq!(got.offset, 0.0);
        assert!(!got.offset_frozen);
        // The smoothed value survives a reset: it describes the history, not the offset.
        assert!(got.smoothed_value.is_some());
    }

    #[test]
    fn refuses_the_inputs_the_archive_refuses() {
        assert_eq!(
            calibrate(f32::NAN, BASE, &[], Sex::Male, 14, false),
            Err(CalibratorError::DailyValueMissing)
        );
        assert_eq!(
            calibrate(45.0, f32::NAN, &[], Sex::Male, 14, false),
            Err(CalibratorError::HardwareChangeMissing)
        );
        let mut rows = history(4, 0.0, false);
        rows[2].timestamp = BASE - 10.0 * DAY;
        assert_eq!(
            calibrate(45.0, BASE, &rows, Sex::Male, 14, false),
            Err(CalibratorError::TimestampsNotSorted)
        );
        let mut rows = history(4, 0.0, false);
        rows[1].calibrated_value = f32::NAN;
        assert_eq!(
            calibrate(45.0, BASE, &rows, Sex::Male, 14, false),
            Err(CalibratorError::HistoryMissing)
        );
    }

    /// Vectors generated by `tools/ml/deterministic_vectors.py cva_calibrator_1_3_0`.
    #[test]
    fn matches_the_archive_on_generated_vectors() {
        let raw =
            include_str!("../../../../../../artifacts/models/vectors/cva_calibrator_1_3_0.json");
        let file: serde_json::Value =
            serde_json::from_str(raw).expect("the vector file should parse");
        let mut checked = 0;
        for vector in file["vectors"]
            .as_array()
            .expect("vectors should be a list")
        {
            let inputs = &vector["inputs"];
            // Every history column arrives as a column vector, one value per row.
            let column = |name: &str| -> Vec<f32> {
                inputs[name]
                    .as_array()
                    .expect("column should be a list")
                    .iter()
                    .map(|row| {
                        row.as_array().expect("a row")[0]
                            .as_f64()
                            .expect("a number") as f32
                    })
                    .collect()
            };
            let single = |name: &str| -> f64 {
                inputs[name].as_array().expect("a matrix")[0]
                    .as_array()
                    .expect("a row")[0]
                    .as_f64()
                    .unwrap_or(0.0)
            };
            let (values, offsets, frozen, timestamps) = (
                column("calibrated_cva_values"),
                column("offsets"),
                column("is_offset_frozen"),
                column("timestamps"),
            );
            let history: Vec<HistoryRow> = (0..values.len())
                .map(|index| HistoryRow {
                    timestamp: timestamps[index],
                    calibrated_value: values[index],
                    offset: offsets[index],
                    offset_frozen: frozen[index] == 1.0,
                })
                .collect();
            let sex = match single("sex_at_birth") as i32 {
                1 => Sex::Male,
                -1 => Sex::Female,
                _ => Sex::Unspecified,
            };
            let reset = inputs["reset_baseline"].as_array().expect("a matrix")[0]
                .as_array()
                .expect("a row")[0]
                .as_bool()
                .unwrap_or(false);
            let freeze = inputs["freeze_offset_days"].as_array().expect("a matrix")[0]
                .as_array()
                .expect("a row")[0]
                .as_i64()
                .expect("an integer") as usize;
            let got = calibrate(
                single("daily_cva_value") as f32,
                single("hw_serial_change") as f32,
                &history,
                sex,
                freeze,
                reset,
            )
            .expect("the archive accepted this input");

            let want = vector["outputs"].as_array().expect("outputs are a list");
            // Each output is nested to its own depth; this reaches the single scalar inside.
            fn scalar(value: &serde_json::Value) -> Option<f32> {
                match value {
                    serde_json::Value::Array(items) => items.first().and_then(scalar),
                    serde_json::Value::Number(number) => number.as_f64().map(|v| v as f32),
                    _ => None,
                }
            }
            let close = |name: &str, got: f32, index: usize| {
                let expected = scalar(&want[index]).expect("a number");
                assert!(
                    (got - expected).abs() <= TOLERANCE * expected.abs().max(1.0),
                    "{name}: {got} vs {expected}"
                );
            };
            close("calibrated", got.calibrated_value, 0);
            close("offset", got.offset, 1);
            close("frozen", f32::from(u8::from(got.offset_frozen)), 2);
            close("pwv", got.pulse_wave_velocity, 3);
            match scalar(&want[4]) {
                None => assert!(got.smoothed_value.is_none(), "smoothed should be absent"),
                Some(expected) => {
                    let value = got.smoothed_value.expect("smoothed should be present");
                    assert!(
                        (value - expected).abs() <= TOLERANCE * expected.abs().max(1.0),
                        "smoothed: {value} vs {expected}"
                    );
                }
            }
            checked += 1;
        }
        assert_eq!(checked, 8, "every generated vector should be checked");
    }
}
