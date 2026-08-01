//! Deterministic preprocessing and bounded occlusion tensors for the provisional ECG classifier.

use mav_model::version::Version;

pub const ECG_MODEL_INPUT_LEN: usize = 7_680;
pub const ECG_MODEL_SAMPLE_RATE_HZ: u32 = 256;
pub const ECG_PREPROCESSING_ALGORITHM: &str = "nao_full_v2_ecg_preprocessing";
pub const ECG_PREPROCESSING_VERSION: Version = Version::new(1, 0, 0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EcgUnit {
    Microvolts,
    Millivolts,
    Volts,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EcgPreprocessError {
    InvalidSampleRate,
    EmptySignal,
    NonFiniteSample,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedEcg {
    pub source_mv: Vec<f32>,
    pub resampled_mv: Vec<f32>,
    pub filtered_mv: Vec<f32>,
    pub fitted_mv: Vec<f32>,
    pub normalized: Vec<f32>,
}

const FILTER_SECTIONS: [[f32; 6]; 5] = [
    [
        0.053_037_636,
        0.106_075_27,
        0.053_037_636,
        1.0,
        -0.795_042_7,
        0.421_211_87,
    ],
    [1.0, 0.0, -1.0, 1.0, -1.301_494_5, 0.310_059_82],
    [1.0, -2.0, 1.0, 1.0, -1.987_795_8, 0.987_947_17],
    [
        0.979_954_1,
        -0.660_273_2,
        0.979_954_1,
        1.0,
        -0.660_273_2,
        0.959_908_25,
    ],
    [
        0.976_039_6,
        -0.191_337_21,
        0.976_039_6,
        1.0,
        -0.191_337_21,
        f32::from_bits(0x3f73_bb75),
    ],
];

pub fn linear_resample(
    signal: &[f32],
    source_rate_hz: u32,
    target_rate_hz: u32,
) -> Result<Vec<f32>, EcgPreprocessError> {
    validate(signal, source_rate_hz)?;
    if target_rate_hz == 0 {
        return Err(EcgPreprocessError::InvalidSampleRate);
    }
    if source_rate_hz == target_rate_hz {
        return Ok(signal.to_vec());
    }

    let duration = (signal.len() - 1) as f64 / f64::from(source_rate_hz);
    let output_len = (duration * f64::from(target_rate_hz)).round() as usize + 1;
    let mut output = Vec::with_capacity(output_len);
    for index in 0..output_len {
        let position = index as f64 / f64::from(target_rate_hz) * f64::from(source_rate_hz);
        let left = (position.floor() as usize).min(signal.len() - 1);
        let right = (left + 1).min(signal.len() - 1);
        let fraction = position - left as f64;
        output.push(
            ((1.0 - fraction) * f64::from(signal[left]) + fraction * f64::from(signal[right]))
                as f32,
        );
    }
    Ok(output)
}

pub fn prepare_ecg(
    values: &[f32],
    sample_rate_hz: u32,
    unit: EcgUnit,
) -> Result<PreparedEcg, EcgPreprocessError> {
    validate(values, sample_rate_hz)?;
    let source_mv = convert_to_mv(values, unit);
    let resampled_mv = linear_resample(&source_mv, sample_rate_hz, ECG_MODEL_SAMPLE_RATE_HZ)?;

    // This apparently redundant scale round-trip is compatibility behaviour from the recovered
    // implementation. Preserve its f32 rounding rather than algebraically cancelling it.
    let mut analysis_signal: Vec<f32> = resampled_mv
        .iter()
        .map(|value| *value / 1_000.0_f32)
        .collect();
    let signal_median = median(&analysis_signal);
    let deviations: Vec<f32> = analysis_signal
        .iter()
        .map(|value| (*value - signal_median).abs())
        .collect();
    if median(&deviations) < 0.01 {
        for value in &mut analysis_signal {
            *value *= 1_000.0;
        }
    }

    let mean = mean_f64(&analysis_signal) as f32;
    let centered: Vec<f32> = analysis_signal.iter().map(|value| *value - mean).collect();
    let mut filtered_mv = zero_phase_sos(&centered);
    let filtered_mean = mean_f64(&filtered_mv);
    let filtered_std = population_std(&filtered_mv, filtered_mean);
    if filtered_mean.abs() > 0.02 || filtered_std < 0.000_1 {
        filtered_mv = one_pole_highpass(&filtered_mv, ECG_MODEL_SAMPLE_RATE_HZ, 0.5);
    }

    let fitted_mv = center_fit(&filtered_mv, ECG_MODEL_INPUT_LEN);
    let normalized = zscore_per_record(&fitted_mv);
    Ok(PreparedEcg {
        source_mv,
        resampled_mv,
        filtered_mv,
        fitted_mv,
        normalized,
    })
}

pub fn inference_tensors(normalized: &[f32]) -> Result<Vec<Vec<f32>>, EcgPreprocessError> {
    validate(normalized, ECG_MODEL_SAMPLE_RATE_HZ)?;
    if normalized.len() != ECG_MODEL_INPUT_LEN {
        return Err(EcgPreprocessError::EmptySignal);
    }
    let window = 5 * ECG_MODEL_SAMPLE_RATE_HZ as usize;
    let mut tensors = Vec::with_capacity(7);
    tensors.push(normalized.to_vec());
    for start in (0..ECG_MODEL_INPUT_LEN).step_by(window) {
        let mut occluded = normalized.to_vec();
        occluded[start..start + window].fill(0.0);
        tensors.push(occluded);
    }
    Ok(tensors)
}

fn validate(signal: &[f32], sample_rate_hz: u32) -> Result<(), EcgPreprocessError> {
    if sample_rate_hz == 0 {
        return Err(EcgPreprocessError::InvalidSampleRate);
    }
    if signal.is_empty() {
        return Err(EcgPreprocessError::EmptySignal);
    }
    if signal.iter().any(|value| !value.is_finite()) {
        return Err(EcgPreprocessError::NonFiniteSample);
    }
    Ok(())
}

fn convert_to_mv(values: &[f32], unit: EcgUnit) -> Vec<f32> {
    let divisor = match unit {
        EcgUnit::Microvolts => 1_000.0,
        EcgUnit::Millivolts => 1.0,
        EcgUnit::Volts => 0.001,
        EcgUnit::Unknown => {
            let stride = (values.len() / 5_000).max(1);
            let count = values.iter().step_by(stride).count().max(1);
            let mean_abs = values
                .iter()
                .step_by(stride)
                .map(|value| f64::from(value.abs()))
                .sum::<f64>()
                / count as f64;
            if mean_abs > 20.0 {
                1_000.0
            } else if mean_abs < 0.005 {
                0.001
            } else {
                1.0
            }
        }
    };
    values.iter().map(|value| *value / divisor).collect()
}

fn median(values: &[f32]) -> f32 {
    let mut ordered = values.to_vec();
    ordered.sort_unstable_by(f32::total_cmp);
    ordered[ordered.len() / 2]
}

fn run_sos_forward(signal: &[f32]) -> Vec<f32> {
    let mut current = signal.to_vec();
    let mut scratch = vec![0.0; signal.len()];
    for [b0, b1, b2, a0, a1, a2] in FILTER_SECTIONS {
        let divisor = if a0.abs() >= 1e-12 { a0 } else { 1.0 };
        let b0 = b0 / divisor;
        let b1 = b1 / divisor;
        let b2 = b2 / divisor;
        let a1 = a1 / divisor;
        let a2 = a2 / divisor;
        let mut state1 = 0.0_f32;
        let mut state2 = 0.0_f32;
        for (index, sample) in current.iter().copied().enumerate() {
            let feed_zero = b0 * sample;
            let output = feed_zero + state1;
            let feed_one = b1 * sample;
            let back_one = a1 * output;
            let next_state_one = feed_one - back_one;
            state1 = next_state_one + state2;
            let feed_two = b2 * sample;
            let back_two = a2 * output;
            state2 = feed_two - back_two;
            scratch[index] = output;
        }
        std::mem::swap(&mut current, &mut scratch);
    }
    current
}

fn zero_phase_sos(signal: &[f32]) -> Vec<f32> {
    let mut forward = run_sos_forward(signal);
    forward.reverse();
    let mut backward = run_sos_forward(&forward);
    backward.reverse();
    backward
}

fn one_pole_highpass(signal: &[f32], sample_rate_hz: u32, cutoff_hz: f64) -> Vec<f32> {
    let period = 1.0 / f64::from(sample_rate_hz.max(1));
    let rc = 1.0 / (cutoff_hz.max(0.001) * 2.0 * std::f64::consts::PI);
    let alpha = (rc / (period + rc)) as f32;
    let mut output = vec![0.0; signal.len()];
    let mut previous_input = signal[0];
    let mut previous_output = 0.0_f32;
    for (index, sample) in signal.iter().copied().enumerate().skip(1) {
        previous_output = alpha * (previous_output + sample - previous_input);
        output[index] = previous_output;
        previous_input = sample;
    }
    output
}

fn center_fit(signal: &[f32], length: usize) -> Vec<f32> {
    if signal.len() == length {
        return signal.to_vec();
    }
    let mut output = vec![0.0; length];
    if signal.len() > length {
        let start = (signal.len() - length) / 2;
        output.copy_from_slice(&signal[start..start + length]);
    } else {
        let start = (length - signal.len()) / 2;
        output[start..start + signal.len()].copy_from_slice(signal);
    }
    output
}

fn mean_f64(signal: &[f32]) -> f64 {
    signal.iter().map(|value| f64::from(*value)).sum::<f64>() / signal.len() as f64
}

fn population_std(signal: &[f32], mean: f64) -> f64 {
    let variance = signal
        .iter()
        .map(|value| {
            let delta = f64::from(*value) - mean;
            delta * delta
        })
        .sum::<f64>()
        / signal.len() as f64;
    variance.max(0.0).sqrt()
}

fn zscore_per_record(signal: &[f32]) -> Vec<f32> {
    let mean = mean_f64(signal);
    let mean_squares = signal
        .iter()
        .map(|value| {
            let value = f64::from(*value);
            value * value
        })
        .sum::<f64>()
        / signal.len() as f64;
    let standard_deviation = (mean_squares - mean * mean).max(1e-9).sqrt();
    signal
        .iter()
        .map(|value| ((f64::from(*value) - mean) / standard_deviation) as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    const GOLDEN_CSV: &str = include_str!("../../../../fixtures/ecg/n_regular_72_v1.csv");

    fn golden_values() -> Vec<f32> {
        GOLDEN_CSV
            .lines()
            .skip(1)
            .map(|line| line.split(',').nth(1).unwrap().parse::<f32>().unwrap())
            .collect()
    }

    #[test]
    fn recovered_reference_tensor_is_byte_identical() {
        let prepared = prepare_ecg(&golden_values(), 256, EcgUnit::Millivolts).unwrap();
        assert_eq!(prepared.normalized.len(), ECG_MODEL_INPUT_LEN);
        let bytes: Vec<u8> = prepared
            .normalized
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect();
        let actual = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            actual,
            "1480c4ac65402ce3d87870f869513404961b410671f9ecb0fe9bccf776db9af0"
        );
    }

    #[test]
    fn resampling_100_hz_to_256_hz_uses_reference_positions() {
        let output = linear_resample(&[0.0, 10.0, 20.0], 100, 256).unwrap();
        assert_eq!(output.len(), 6);
        assert_eq!(
            output
                .iter()
                .map(|value| value.to_bits())
                .collect::<Vec<_>>(),
            [0.0_f32, 3.90625, 7.8125, 11.71875, 15.625, 19.53125]
                .into_iter()
                .map(f32::to_bits)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn centre_fit_crops_and_pads_around_the_middle() {
        let short = center_fit(&[0.0, 1.0, 0.0], ECG_MODEL_INPUT_LEN);
        assert_eq!(short.len(), ECG_MODEL_INPUT_LEN);
        assert_eq!(short[(ECG_MODEL_INPUT_LEN - 3) / 2 + 1], 1.0);

        let long: Vec<f32> = (0..ECG_MODEL_INPUT_LEN + 2)
            .map(|value| value as f32)
            .collect();
        let cropped = center_fit(&long, ECG_MODEL_INPUT_LEN);
        assert_eq!(cropped[0], 1.0);
        assert_eq!(cropped[ECG_MODEL_INPUT_LEN - 1], ECG_MODEL_INPUT_LEN as f32);
    }

    #[test]
    fn invalid_input_is_rejected_exactly() {
        assert_eq!(
            prepare_ecg(&[1.0], 0, EcgUnit::Millivolts),
            Err(EcgPreprocessError::InvalidSampleRate)
        );
        assert_eq!(
            prepare_ecg(&[], 256, EcgUnit::Millivolts),
            Err(EcgPreprocessError::EmptySignal)
        );
        assert_eq!(
            prepare_ecg(&[f32::NAN], 256, EcgUnit::Millivolts),
            Err(EcgPreprocessError::NonFiniteSample)
        );
    }

    #[test]
    fn inference_request_is_baseline_then_six_ordered_occlusions() {
        let signal: Vec<f32> = (0..ECG_MODEL_INPUT_LEN).map(|value| value as f32).collect();
        let tensors = inference_tensors(&signal).unwrap();
        assert_eq!(tensors.len(), 7);
        assert_eq!(tensors[0], signal);
        for window in 0..6 {
            let start = window * 1_280;
            let end = start + 1_280;
            assert!(tensors[window + 1][start..end]
                .iter()
                .all(|value| *value == 0.0));
            assert_eq!(&tensors[window + 1][..start], &signal[..start]);
            assert_eq!(&tensors[window + 1][end..], &signal[end..]);
        }
    }
}
