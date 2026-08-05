//! `steps_motion_decoder 2.0.0` — unpacking the strap's quantised motion features.
//!
//! The strap does not send motion features as numbers. It sends integer codes, because a
//! 9-bit code costs a ninth of what a float costs over a link that is on a battery. This
//! archive is the other half of that encoding: a per-column range, bit depth and optional
//! transform, applied to turn each code back into the quantity it stood for.
//!
//! The interesting part is not the linear rescale, it is what surrounds it:
//!
//! * **Transforms.** Amplitudes are encoded through `log10(x + 1)` and fractions through
//!   `sqrt`, so the codes are evenly spaced in the *transformed* domain where the values are
//!   not. The range endpoints are transformed too, before the interpolation, and only the
//!   result is transformed back.
//! * **A reserved zero.** Stride frequency needs to say "no stride detected", which is not
//!   the same as "the lowest stride frequency". Code zero is that, one code is spent on it,
//!   and the remaining codes cover the range.
//! * **Three sub-windows in one row.** Each row carries three thirty-second windows sharing
//!   one accelerometer summary. The row is expanded back into three, with timestamps
//!   interpolated backwards from the row's own, spaced by a third of the gap to the previous
//!   row — and a gap outside 25.5 to 34.5 seconds is replaced with a nominal thirty, because
//!   a dropped packet should not stretch the sub-windows it was not part of.

/// The three columns that describe the whole row rather than one sub-window.
const SHARED_COLUMNS: usize = 3;

/// The eight columns each sub-window carries.
const PER_WINDOW_COLUMNS: usize = 8;

/// Sub-windows per row.
const WINDOWS_PER_ROW: usize = 3;

/// Columns in one input row.
pub const INPUT_COLUMNS: usize = SHARED_COLUMNS + WINDOWS_PER_ROW * PER_WINDOW_COLUMNS;

/// Columns in one output row.
pub const OUTPUT_COLUMNS: usize = SHARED_COLUMNS + PER_WINDOW_COLUMNS;

/// The nominal spacing between rows, in milliseconds.
const NOMINAL_SPACING_MS: i64 = 30_000;

/// Spacing outside this band is treated as a dropped packet and replaced with the nominal.
const MIN_PLAUSIBLE_SPACING_MS: i64 = 25_500;
const MAX_PLAUSIBLE_SPACING_MS: i64 = 34_500;

/// How a column's codes were spaced across its range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transform {
    /// Codes are evenly spaced in the value itself.
    None,
    /// Codes are evenly spaced in `log10(value + 1)`.
    Log,
    /// Codes are evenly spaced in `sqrt(value)`.
    Sqrt,
}

impl Transform {
    /// Into the domain the codes are evenly spaced in.
    fn forward(self, value: f32) -> f32 {
        match self {
            Self::None => value,
            Self::Log => (value + 1.0).log10(),
            Self::Sqrt => value.sqrt(),
        }
    }

    /// Back out of it.
    fn inverse(self, value: f32) -> f32 {
        match self {
            Self::None => value,
            Self::Log => 10f32.powf(value) - 1.0,
            Self::Sqrt => value * value,
        }
    }
}

/// One column's encoding.
#[derive(Debug, Clone, Copy)]
struct Column {
    low: f32,
    high: f32,
    bits: u32,
    /// True where code zero is reserved to mean "absent" rather than "lowest".
    reserved_zero: bool,
    transform: Transform,
}

const fn column(
    low: f32,
    high: f32,
    bits: u32,
    reserved_zero: bool,
    transform: Transform,
) -> Column {
    Column {
        low,
        high,
        bits,
        reserved_zero,
        transform,
    }
}

/// The eight per-sub-window columns, in order.
const WINDOW_COLUMNS: [Column; PER_WINDOW_COLUMNS] = [
    column(0.0, 8000.0, 9, false, Transform::Log), // total_amplitude_mg
    column(0.68, 3.4, 9, true, Transform::None),   // stride_frequency
    column(0.0, 1.0, 9, false, Transform::Sqrt),   // stride_amplitude_frac
    column(0.0, 24.804_688, 7, false, Transform::None), // first_non_locomotor_frequency
    column(0.0, 0.65, 8, false, Transform::Sqrt),  // first_non_locomotor_amplitude_frac
    column(0.0, 0.75, 8, false, Transform::Sqrt),  // gait_amplitude_frac
    column(0.0, 1.0, 8, false, Transform::None),   // frequency_bin_high_frac
    column(0.0, 1.0, 8, false, Transform::None),   // frequency_bin_mid_frac
];

/// The three columns shared across a row's sub-windows.
const ROW_COLUMNS: [Column; SHARED_COLUMNS] = [
    column(0.0, 8000.0, 10, false, Transform::Log), // sum_accel_mg_std
    column(0.0, 0.85, 8, false, Transform::None),   // y_accel_std_ratio
    column(0.0, 0.85, 8, false, Transform::None),   // z_accel_std_ratio
];

/// Why the archive refused the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderError {
    /// The two inputs describe different numbers of rows.
    RowCountMismatch,
    /// A row does not carry [`INPUT_COLUMNS`] values.
    WrongColumnCount,
}

/// One decoded thirty-second window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionWindow {
    /// When the window ends, in milliseconds.
    pub timestamp_ms: i64,
    /// The eleven decoded features, three shared and eight the window's own.
    pub features: [f32; OUTPUT_COLUMNS],
}

/// Turn one code back into the value it stood for.
fn decode(code: f32, column: Column) -> f32 {
    let mut levels = f32::from(1u16 << column.bits.min(15));
    let mut value = code;
    if column.reserved_zero {
        // One code is spent saying "absent", so the rest cover the range — and a code of zero
        // has to come back out as exactly zero rather than as the bottom of the range.
        levels -= 1.0;
        value = (value - 1.0).max(0.0);
    }
    let low = column.transform.forward(column.low);
    let high = column.transform.forward(column.high);
    let decoded = column
        .transform
        .inverse(value / levels * (high - low) + low);
    if column.reserved_zero && code <= 0.0 {
        0.0
    } else {
        decoded
    }
}

/// Where a row's sub-windows sit in time.
///
/// The row's own timestamp ends the last sub-window, and the two before it are spaced back by
/// a third of the gap to the previous row. The first row has no previous row, so it borrows
/// the second row's gap.
fn spacings(timestamps: &[i64]) -> Vec<i64> {
    if timestamps.len() < 2 {
        return vec![NOMINAL_SPACING_MS; timestamps.len().max(1)];
    }
    let first = timestamps[1] - timestamps[0];
    let mut gaps: Vec<i64> = core::iter::once(first)
        .chain(timestamps.windows(2).map(|pair| pair[1] - pair[0]))
        .collect();
    // A gap this far from nominal means a row went missing, and stretching the sub-windows to
    // fill it would place them where nothing was measured.
    if gaps
        .iter()
        .any(|gap| !(MIN_PLAUSIBLE_SPACING_MS..=MAX_PLAUSIBLE_SPACING_MS).contains(gap))
    {
        gaps = gaps
            .iter()
            .map(|gap| {
                if (MIN_PLAUSIBLE_SPACING_MS..=MAX_PLAUSIBLE_SPACING_MS).contains(gap) {
                    *gap
                } else {
                    NOMINAL_SPACING_MS
                }
            })
            .collect();
    }
    gaps
}

/// Decode a block of packed motion rows into thirty-second windows.
pub fn decode_motion(
    timestamps: &[i64],
    rows: &[[f32; INPUT_COLUMNS]],
) -> Result<Vec<MotionWindow>, DecoderError> {
    if timestamps.len() != rows.len() {
        return Err(DecoderError::RowCountMismatch);
    }
    let gaps = spacings(timestamps);
    let mut out = Vec::with_capacity(rows.len() * WINDOWS_PER_ROW);
    for (index, row) in rows.iter().enumerate() {
        let step = gaps[index].div_euclid(WINDOWS_PER_ROW as i64);
        for window in 0..WINDOWS_PER_ROW {
            let mut features = [0.0f32; OUTPUT_COLUMNS];
            for (slot, spec) in ROW_COLUMNS.iter().enumerate() {
                features[slot] = decode(row[slot], *spec);
            }
            for (slot, spec) in WINDOW_COLUMNS.iter().enumerate() {
                let source = SHARED_COLUMNS + window * PER_WINDOW_COLUMNS + slot;
                features[SHARED_COLUMNS + slot] = decode(row[source], *spec);
            }
            out.push(MotionWindow {
                timestamp_ms: timestamps[index] - step * (WINDOWS_PER_ROW - 1 - window) as i64,
                features,
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOLERANCE: f32 = 2e-3;
    const BASE: i64 = 1_700_000_000_000;

    fn row(fill: f32) -> [f32; INPUT_COLUMNS] {
        [fill; INPUT_COLUMNS]
    }

    #[test]
    fn a_row_becomes_three_windows_spaced_a_third_of_the_gap_apart() {
        let got = decode_motion(&[BASE, BASE + 30_000], &[row(0.0), row(0.0)]).expect("valid");
        assert_eq!(got.len(), 6);
        assert_eq!(got[0].timestamp_ms, BASE - 20_000);
        assert_eq!(got[1].timestamp_ms, BASE - 10_000);
        assert_eq!(got[2].timestamp_ms, BASE);
    }

    #[test]
    fn an_implausible_gap_is_replaced_with_the_nominal_spacing() {
        // A 40-second gap means a row went missing; the sub-windows stay ten seconds apart.
        let got = decode_motion(&[BASE, BASE + 40_000], &[row(0.0), row(0.0)]).expect("valid");
        assert_eq!(got[4].timestamp_ms - got[3].timestamp_ms, 10_000);
    }

    #[test]
    fn a_reserved_zero_decodes_to_zero_not_to_the_bottom_of_the_range() {
        let stride = WINDOW_COLUMNS[1];
        assert!(stride.reserved_zero);
        assert_eq!(decode(0.0, stride), 0.0);
        // Code one is the bottom of the range proper, which is well above zero.
        assert!((decode(1.0, stride) - stride.low).abs() < TOLERANCE);
    }

    #[test]
    fn a_column_without_a_reserved_zero_decodes_code_zero_to_its_low_bound() {
        let ratio = ROW_COLUMNS[1];
        assert!(!ratio.reserved_zero);
        assert_eq!(decode(0.0, ratio), ratio.low);
    }

    #[test]
    fn the_transformed_columns_are_not_linear_in_the_code() {
        let amplitude = WINDOW_COLUMNS[0];
        assert_eq!(amplitude.transform, Transform::Log);
        let top = f32::from(1u16 << amplitude.bits) - 1.0;
        let quarter = decode(top / 4.0, amplitude);
        let half = decode(top / 2.0, amplitude);
        // Under a log encoding the upper half of the code range carries far more than half
        // the value range; a linear decode would put `half` at twice `quarter`.
        assert!(half > quarter * 4.0, "{half} vs {quarter}");
    }

    #[test]
    fn the_top_code_decodes_to_the_top_of_the_range() {
        for spec in ROW_COLUMNS.iter().chain(WINDOW_COLUMNS.iter()) {
            let top = f32::from(1u16 << spec.bits) - if spec.reserved_zero { 0.0 } else { 1.0 };
            let decoded = decode(top, *spec);
            assert!(
                decoded <= spec.high * 1.01 + 0.01,
                "{decoded} exceeded {} for a {}-bit column",
                spec.high,
                spec.bits
            );
        }
    }

    #[test]
    fn refuses_mismatched_lengths() {
        assert_eq!(
            decode_motion(&[BASE], &[row(0.0), row(0.0)]),
            Err(DecoderError::RowCountMismatch)
        );
    }

    /// Vectors generated by `tools/ml/deterministic_vectors.py steps_motion_decoder_2_0_0`.
    #[test]
    fn matches_the_archive_on_generated_vectors() {
        let raw = include_str!(
            "../../../../../../artifacts/models/vectors/steps_motion_decoder_2_0_0.json"
        );
        let file: serde_json::Value =
            serde_json::from_str(raw).expect("the vector file should parse");
        let mut checked = 0;
        for vector in file["vectors"]
            .as_array()
            .expect("vectors should be a list")
        {
            let inputs = &vector["inputs"];
            let timestamps: Vec<i64> = inputs["timestamps"]
                .as_array()
                .expect("timestamps should be a list")
                .iter()
                .map(|row| {
                    row.as_array().expect("a row")[0]
                        .as_i64()
                        .expect("a timestamp")
                })
                .collect();
            let rows: Vec<[f32; INPUT_COLUMNS]> = inputs["data"]
                .as_array()
                .expect("data should be a list")
                .iter()
                .map(|row| {
                    let values = row.as_array().expect("a row");
                    let mut packed = [0.0f32; INPUT_COLUMNS];
                    for (slot, value) in values.iter().enumerate() {
                        packed[slot] = value.as_f64().expect("a code") as f32;
                    }
                    packed
                })
                .collect();
            let got = decode_motion(&timestamps, &rows).expect("the archive accepted this input");

            let want = vector["outputs"].as_array().expect("outputs are a list");
            let want_timestamps = want[0].as_array().expect("timestamps are a list");
            let want_data = want[1].as_array().expect("data is a list");
            assert_eq!(got.len(), want_timestamps.len(), "window count");
            for (index, window) in got.iter().enumerate() {
                assert_eq!(
                    window.timestamp_ms,
                    want_timestamps[index].as_array().expect("a row")[0]
                        .as_i64()
                        .expect("a timestamp"),
                    "window {index} timestamp"
                );
                let expected = want_data[index].as_array().expect("a row");
                for (slot, value) in window.features.iter().enumerate() {
                    let target = expected[slot].as_f64().expect("a feature") as f32;
                    assert!(
                        (value - target).abs() <= TOLERANCE * target.abs().max(1.0),
                        "window {index} feature {slot}: {value} vs {target}"
                    );
                }
            }
            checked += 1;
        }
        assert_eq!(checked, 5, "every generated vector should be checked");
    }
}
