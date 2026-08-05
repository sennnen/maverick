//! Deterministic PPG preprocessing: everything that happens between a stored sample and a
//! model input tensor.
//!
//! Each PPG model carries its own front-end inside its TorchScript archive. Those
//! front-ends are pure signal processing — jump limiting, a two-stage moving-average detrend, a
//! median smoother, foot detection, normalisation — and they are ported here rather than
//! converted, for the reason `docs/ml.md` gives: the resample-off-by-one class of bug belongs in
//! shared, fixture-tested Rust, not inside an opaque graph that two platforms compile
//! differently.
//!
//! Three front-ends live here, and they are deliberately not merged:
//!
//! - [`pulsenet_input`], the filter PulseNet-Foundation applies before its encoder. Jump limit
//!   5,000, trend windows 100 then 150, median 3, mean 3.
//! - [`cva_pulse`], the CVA 2.1.0 preprocessor. Same family, different constants (jump limit
//!   2,000), and it additionally emits the five-value feature vector the predictor takes
//!   alongside the pulse train.
//! - [`pulse_ppg_input`], the open-weight encoder's front-end: resample to 50 Hz, fit to the
//!   four-minute pre-training window, z-score.
//!
//! The constants differ between the two in-house front-ends and are not interchangeable. Using
//! CVA's jump limit for PulseNet would change which samples survive, so each is written out with
//! the value its own training wrapper holds.

use mav_model::error::{codes, MavError, Result};

/// One PPG segment as both in-house encoders expect it: thirty seconds at 50 Hz.
pub const PPG_SEGMENT_SAMPLES: usize = 1_500;
pub const PPG_SAMPLE_RATE_HZ: u32 = 50;

/// Pulse-PPG was pre-trained on four-minute windows at 50 Hz, and that is the window
/// contracted here. The encoder is fully convolutional so a shorter window would run, but a
/// shorter window is not what the weights were fitted against.
pub const PULSE_PPG_SEGMENT_SAMPLES: usize = 12_000;
pub const PULSE_PPG_SAMPLE_RATE_HZ: u32 = 50;

/// The pulse train CVA's predictor takes: one shorter than the segment, because the front-end
/// works on the first difference.
pub const CVA_PULSE_SAMPLES: usize = PPG_SEGMENT_SAMPLES - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterKind {
    Mean,
    Median,
}

/// The moving-average detrend chain, as parameterised by whichever wrapper it came from.
#[derive(Clone, Copy, Debug)]
pub struct DetrendConfig {
    /// Sample-to-sample jumps larger than this are zeroed before integration; the training
    /// wrappers use it to drop LED-gain steps rather than let them ring through the filter.
    pub jump_limit: f32,
    pub trend_window: usize,
    pub trend_window_second: usize,
    pub median_window: usize,
    pub smooth_window: usize,
}

/// PulseNet-Foundation v0.4.0's `MovingAverageFilter`.
pub const PULSENET_DETREND: DetrendConfig = DetrendConfig {
    jump_limit: 5_000.0,
    trend_window: 100,
    trend_window_second: 150,
    median_window: 3,
    smooth_window: 3,
};

/// CVA 2.1.0's `Preprocessor`.
pub const CVA_DETREND: DetrendConfig = DetrendConfig {
    jump_limit: 2_000.0,
    trend_window: 100,
    trend_window_second: 150,
    median_window: 3,
    smooth_window: 3,
};

const CVA_PEAK_SEARCH_WIDTH: usize = 60;
const CVA_VOCAB_SIZE: f32 = 128.0;
const CVA_STD_LIMIT: f32 = 20.36;
const CVA_MEAN_LOWER_LIMIT: f32 = 52.35;
const CVA_MEAN_UPPER_LIMIT: f32 = 79.81;

/// The five values CVA's predictor takes beside the pulse train.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CvaFeatures {
    /// Mean of the raw segment: the DC level the AFE was sitting at.
    pub mean_dc: f32,
    /// Peak-to-trough span of the filtered pulse train.
    pub max_min: f32,
    /// 1.0 when the normalised pulse train passed the mean and spread limits, else 0.0.
    pub accepted: f32,
    /// Signal-to-noise in dB: filtered energy over the residual the smoother removed.
    pub snr_db: f32,
    /// Heart rate in bpm from the median foot-to-foot interval, or `f32::NAN` when fewer than
    /// two feet were found. The training wrapper propagates that NaN rather than substituting.
    pub heart_rate_bpm: f32,
}

impl CvaFeatures {
    /// In the order the predictor's second input tensor expects.
    pub fn to_tensor(self) -> Vec<f32> {
        vec![
            self.mean_dc,
            self.max_min,
            self.accepted,
            self.snr_db,
            self.heart_rate_bpm,
        ]
    }
}

/// A prepared CVA input: the normalised pulse train and the feature vector beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct CvaPulse {
    pub pulses: Vec<f32>,
    pub features: CvaFeatures,
}

/// Reflect-pad, then take a sliding statistic — the `moving_average` both wrappers share.
///
/// The window is centred by padding `window / 2` on each side, so an odd window returns the input
/// length and an even window returns one more. The callers depend on that asymmetry: the trend
/// stage uses even windows and trims the extra element afterwards.
pub fn moving_average(data: &[f32], kind: FilterKind, window: usize) -> Vec<f32> {
    if window == 0 || data.is_empty() {
        return data.to_vec();
    }
    let pad = window / 2;
    let padded = reflect_pad(data, pad);
    if padded.len() < window {
        return data.to_vec();
    }
    let count = padded.len() - window + 1;
    let mut output = Vec::with_capacity(count);
    let mut scratch = vec![0.0_f32; window];
    for start in 0..count {
        let slice = &padded[start..start + window];
        output.push(match kind {
            FilterKind::Mean => {
                let total: f64 = slice.iter().map(|value| f64::from(*value)).sum();
                (total / window as f64) as f32
            }
            FilterKind::Median => {
                scratch.copy_from_slice(slice);
                median_in_place(&mut scratch)
            }
        });
    }
    output
}

/// The shared detrend body: jump-limit the first difference, integrate, remove a twice-smoothed
/// trend, then smooth what is left.
///
/// `prepend_first` selects between the two variants. PulseNet prepends the first sample
/// to the difference so the output keeps the input length; CVA does not, so its output is one
/// shorter. That single flag is the whole difference in structure between them.
fn detrend(signal: &[f32], config: DetrendConfig, prepend_first: bool) -> (Vec<f32>, Vec<f32>) {
    let mut differences = Vec::with_capacity(signal.len());
    if prepend_first {
        differences.push(0.0);
    }
    for pair in signal.windows(2) {
        let step = pair[1] - pair[0];
        differences.push(if step.abs() > config.jump_limit {
            0.0
        } else {
            step
        });
    }

    let mut integrated = Vec::with_capacity(differences.len());
    let mut running = 0.0_f32;
    for value in &differences {
        running += *value;
        integrated.push(running);
    }

    let trend = moving_average(&integrated, FilterKind::Mean, config.trend_window);
    let trend = moving_average(&trend, FilterKind::Mean, config.trend_window_second);
    // Two even windows added one element each; the wrapper drops the outermost pair to line the
    // trend back up with the signal it is subtracted from.
    let offset = trend.len().saturating_sub(integrated.len()) / 2;
    let detrended: Vec<f32> = integrated
        .iter()
        .enumerate()
        .map(|(index, value)| value - trend.get(index + offset).copied().unwrap_or(0.0))
        .collect();

    let filtered = moving_average(&detrended, FilterKind::Median, config.median_window);
    let filtered = moving_average(&filtered, FilterKind::Mean, config.smooth_window);
    (detrended, filtered)
}

/// Prepare one segment for the PulseNet-Foundation encoder.
///
/// Returns exactly [`PPG_SEGMENT_SAMPLES`] values, which is what the encoder's own validator
/// demanded before it would run.
pub fn pulsenet_input(segment: &[f32]) -> Result<Vec<f32>> {
    if segment.len() != PPG_SEGMENT_SAMPLES {
        return Err(preprocessing_error(format!(
            "PulseNet needs {PPG_SEGMENT_SAMPLES} samples, received {}",
            segment.len()
        )));
    }
    if segment.iter().any(|value| !value.is_finite()) {
        return Err(preprocessing_error(
            "PPG segment contains a non-finite sample",
        ));
    }
    let (_detrended, filtered) = detrend(segment, PULSENET_DETREND, true);
    Ok(filtered)
}

/// Prepare one segment for the CVA predictor: the normalised pulse train plus its features.
pub fn cva_pulse(segment: &[f32]) -> Result<CvaPulse> {
    if segment.len() != PPG_SEGMENT_SAMPLES {
        return Err(preprocessing_error(format!(
            "CVA needs {PPG_SEGMENT_SAMPLES} samples, received {}",
            segment.len()
        )));
    }
    if segment.iter().any(|value| !value.is_finite()) {
        return Err(preprocessing_error(
            "PPG segment contains a non-finite sample",
        ));
    }

    let mean_dc = mean(segment);
    let (detrended, filtered) = detrend(segment, CVA_DETREND, false);
    let snr_db = signal_to_noise_db(&filtered, &detrended);
    let heart_rate_bpm = foot_heart_rate(&filtered);

    let maximum = filtered.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let minimum = filtered.iter().copied().fold(f32::INFINITY, f32::min);
    let span = maximum - minimum;
    let scaled: Vec<f32> = filtered
        .iter()
        .map(|value| (((value - minimum) / (span + 1e-8)) * (CVA_VOCAB_SIZE - 1.0)).round())
        .collect();

    let scaled_mean = mean(&scaled);
    let scaled_std = population_std(&scaled, scaled_mean);
    let accepted = (CVA_MEAN_LOWER_LIMIT..=CVA_MEAN_UPPER_LIMIT).contains(&scaled_mean)
        && scaled_std >= CVA_STD_LIMIT;

    let pulses: Vec<f32> = scaled.iter().map(|value| value / CVA_VOCAB_SIZE).collect();
    if pulses.len() != CVA_PULSE_SAMPLES {
        return Err(preprocessing_error(format!(
            "CVA preprocessing produced {} pulse samples, expected {CVA_PULSE_SAMPLES}",
            pulses.len()
        )));
    }

    Ok(CvaPulse {
        pulses,
        features: CvaFeatures {
            mean_dc,
            max_min: span,
            accepted: if accepted { 1.0 } else { 0.0 },
            snr_db,
            heart_rate_bpm,
        },
    })
}

/// Prepare a window for Pulse-PPG: resample to 50 Hz, fit to four minutes, z-score.
///
/// The published pipeline z-scores each user against their own multi-day mean and standard
/// deviation and clips at a per-user border. Maverick has no such per-user distribution at
/// inference time, so this z-scores the window itself and does not clip. That is a smaller
/// change than it sounds: the encoder's first layer is an `InstanceNorm1d`, so it renormalises
/// per window regardless, and the omitted clipping only bounded outliers the connector's own
/// wear gating has already rejected.
pub fn pulse_ppg_input(signal: &[f32], source_rate_hz: u32) -> Result<Vec<f32>> {
    if signal.len() < 2 {
        return Err(preprocessing_error("Pulse-PPG needs at least two samples"));
    }
    if source_rate_hz == 0 {
        return Err(preprocessing_error(
            "Pulse-PPG needs a positive source rate",
        ));
    }
    if signal.iter().any(|value| !value.is_finite()) {
        return Err(preprocessing_error(
            "PPG segment contains a non-finite sample",
        ));
    }
    let resampled = linear_resample(signal, source_rate_hz, PULSE_PPG_SAMPLE_RATE_HZ);
    let fitted = fit_or_pad(&resampled, PULSE_PPG_SEGMENT_SAMPLES);
    Ok(z_score(&fitted))
}

/// Resample by linear interpolation at `index / target * source`, the same positions
/// `ecg_model::linear_resample` uses. Keeping one convention across the core means a signal
/// resampled for two different models lands on the same grid.
pub fn linear_resample(signal: &[f32], source_rate_hz: u32, target_rate_hz: u32) -> Vec<f32> {
    if source_rate_hz == target_rate_hz || signal.len() < 2 {
        return signal.to_vec();
    }
    let duration = (signal.len() - 1) as f64 / f64::from(source_rate_hz);
    let count = (duration * f64::from(target_rate_hz)).round() as usize + 1;
    let mut output = Vec::with_capacity(count);
    for index in 0..count {
        let position = index as f64 / f64::from(target_rate_hz) * f64::from(source_rate_hz);
        let left = (position.floor() as usize).min(signal.len() - 1);
        let right = (left + 1).min(signal.len() - 1);
        let fraction = position - left as f64;
        output.push(
            ((1.0 - fraction) * f64::from(signal[left]) + fraction * f64::from(signal[right]))
                as f32,
        );
    }
    output
}

/// Centre-crop or zero-pad to `length`, matching the ECG path's fit rule.
pub fn fit_or_pad(signal: &[f32], length: usize) -> Vec<f32> {
    if signal.len() == length {
        return signal.to_vec();
    }
    if signal.len() > length {
        let start = (signal.len() - length) / 2;
        return signal[start..start + length].to_vec();
    }
    let mut output = vec![0.0_f32; length];
    let start = (length - signal.len()) / 2;
    output[start..start + signal.len()].copy_from_slice(signal);
    output
}

/// Z-score with a population standard deviation and a floor, as the ECG path does.
pub fn z_score(signal: &[f32]) -> Vec<f32> {
    let average = mean(signal);
    let deviation = population_std(signal, average).max(1e-9);
    signal
        .iter()
        .map(|value| (value - average) / deviation)
        .collect()
}

// --------------------------------------------------------------------------------- internals

/// Mirror `pad` samples onto each end without repeating the edge sample, which is what
/// `torch.nn.functional.pad(mode="reflect")` does and what the filters were fitted
/// against. A short signal clamps instead of reflecting past its own start.
fn reflect_pad(data: &[f32], pad: usize) -> Vec<f32> {
    if pad == 0 || data.is_empty() {
        return data.to_vec();
    }
    let last = data.len() - 1;
    let mut output = Vec::with_capacity(data.len() + 2 * pad);
    for offset in (1..=pad).rev() {
        output.push(data[offset.min(last)]);
    }
    output.extend_from_slice(data);
    for offset in 1..=pad {
        output.push(data[last.saturating_sub(offset)]);
    }
    output
}

fn median_in_place(values: &mut [f32]) -> f32 {
    values.sort_by(f32::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        values[middle]
    } else {
        // Torch's `nanmedian` returns the lower of the two central values rather than their
        // mean. Averaging here would silently change every even-window median filter.
        values[middle - 1]
    }
}

fn mean(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let total: f64 = values.iter().map(|value| f64::from(*value)).sum();
    (total / values.len() as f64) as f32
}

fn population_std(values: &[f32], average: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let variance: f64 = values
        .iter()
        .map(|value| {
            let delta = f64::from(*value) - f64::from(average);
            delta * delta
        })
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt() as f32
}

fn l2_norm(values: &[f32]) -> f64 {
    values
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt()
}

fn signal_to_noise_db(filtered: &[f32], detrended: &[f32]) -> f32 {
    let noise: Vec<f32> = detrended
        .iter()
        .zip(filtered)
        .map(|(raw, smooth)| raw - smooth)
        .collect();
    let numerator = l2_norm(filtered);
    let denominator = l2_norm(&noise);
    if denominator == 0.0 || numerator == 0.0 {
        return f32::NAN;
    }
    (20.0 * (numerator / denominator).log10()) as f32
}

/// Heart rate from pulse feet, the way CVA's preprocessor finds them.
///
/// Feet are minima of the filtered pulse train, so the search runs on its negation. A linear ramp
/// from zero to the last sample is removed first, which stops a drifting baseline from hiding
/// every foot at one end. A candidate must be a strict local maximum and must survive a
/// 60-sample dominance window, which at 50 Hz is 1.2 s: one foot per beat up to 50 bpm, and the
/// dominance test is what keeps the dicrotic notch from counting as a second beat.
fn foot_heart_rate(filtered: &[f32]) -> f32 {
    if filtered.len() < 3 {
        return f32::NAN;
    }
    let last = -filtered[filtered.len() - 1];
    let count = filtered.len();
    let inverted: Vec<f32> = filtered
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let ramp = last * (index as f32 / (count - 1) as f32);
            -value - ramp
        })
        .collect();

    let half = CVA_PEAK_SEARCH_WIDTH / 2;
    let mut feet = Vec::new();
    for index in 1..count - 1 {
        if inverted[index] <= inverted[index - 1] || inverted[index] <= inverted[index + 1] {
            continue;
        }
        let start = index.saturating_sub(half);
        let end = (index + half).min(count - 1);
        let dominant = inverted[start..=end]
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        if inverted[index] >= dominant {
            feet.push(index);
        }
    }
    // The wrapper drops the first and last foot before measuring, because a segment boundary
    // truncates those two beats.
    if feet.len() < 4 {
        return f32::NAN;
    }
    let mut intervals: Vec<f32> = feet[1..feet.len() - 1]
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f32 / PPG_SAMPLE_RATE_HZ as f32)
        .collect();
    if intervals.is_empty() {
        return f32::NAN;
    }
    let median = median_in_place(&mut intervals);
    if median <= 0.0 {
        return f32::NAN;
    }
    60.0 / median
}

fn preprocessing_error(message: impl Into<String>) -> MavError {
    MavError::new(codes::ML_MODEL_PREPROCESSING, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// A pulse-shaped test signal: a fundamental, a dicrotic-notch harmonic, and a slow drift.
    fn synthetic_ppg(samples: usize, bpm: f32) -> Vec<f32> {
        let rate = PPG_SAMPLE_RATE_HZ as f32;
        (0..samples)
            .map(|index| {
                let seconds = index as f32 / rate;
                let phase = TAU * (bpm / 60.0) * seconds;
                let drift = 40.0 * seconds;
                2_000.0 + drift + 300.0 * phase.sin() + 90.0 * (2.0 * phase).sin()
            })
            .collect()
    }

    #[test]
    fn reflect_padding_mirrors_without_repeating_the_edge() {
        let padded = reflect_pad(&[1.0, 2.0, 3.0, 4.0], 2);
        assert_eq!(padded, vec![3.0, 2.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0]);
    }

    #[test]
    fn an_odd_mean_window_keeps_the_length_and_an_even_one_adds_a_sample() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        assert_eq!(moving_average(&data, FilterKind::Mean, 3).len(), 5);
        assert_eq!(moving_average(&data, FilterKind::Mean, 4).len(), 6);
    }

    #[test]
    fn a_three_point_mean_smooths_a_single_spike() {
        let data = vec![0.0, 0.0, 9.0, 0.0, 0.0];
        let smoothed = moving_average(&data, FilterKind::Mean, 3);
        assert_eq!(smoothed[2], 3.0);
        assert_eq!(smoothed[1], 3.0);
    }

    #[test]
    fn a_three_point_median_removes_a_single_spike_entirely() {
        let data = vec![0.0, 0.0, 9.0, 0.0, 0.0];
        let smoothed = moving_average(&data, FilterKind::Median, 3);
        assert_eq!(smoothed, vec![0.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn pulsenet_input_keeps_the_contracted_length_and_removes_the_drift() {
        let segment = synthetic_ppg(PPG_SEGMENT_SAMPLES, 60.0);
        let prepared = pulsenet_input(&segment).expect("prepared");
        assert_eq!(prepared.len(), PPG_SEGMENT_SAMPLES);
        // The input drifts by 40 units per second over thirty seconds; the detrended output must
        // not carry that ramp into the encoder.
        let head = mean(&prepared[..200]);
        let tail = mean(&prepared[PPG_SEGMENT_SAMPLES - 200..]);
        assert!(
            (head - tail).abs() < 50.0,
            "detrend left a ramp: head {head}, tail {tail}"
        );
    }

    #[test]
    fn pulsenet_input_rejects_the_wrong_length() {
        let error = pulsenet_input(&[0.0; 100]).expect_err("wrong length");
        assert_eq!(error.code, codes::ML_MODEL_PREPROCESSING);
    }

    #[test]
    fn pulsenet_input_rejects_a_non_finite_sample() {
        let mut segment = synthetic_ppg(PPG_SEGMENT_SAMPLES, 60.0);
        segment[7] = f32::INFINITY;
        let error = pulsenet_input(&segment).expect_err("non-finite");
        assert_eq!(error.code, codes::ML_MODEL_PREPROCESSING);
    }

    #[test]
    fn a_jump_larger_than_the_limit_is_not_integrated() {
        let mut segment = synthetic_ppg(PPG_SEGMENT_SAMPLES, 60.0);
        for value in segment.iter_mut().skip(750) {
            *value += 9_000.0;
        }
        let prepared = pulsenet_input(&segment).expect("prepared");
        let step = prepared[751] - prepared[749];
        assert!(
            step.abs() < 200.0,
            "gain step survived the jump limit: {step}"
        );
    }

    #[test]
    fn cva_pulse_produces_the_contracted_tensors() {
        let segment = synthetic_ppg(PPG_SEGMENT_SAMPLES, 72.0);
        let prepared = cva_pulse(&segment).expect("prepared");
        assert_eq!(prepared.pulses.len(), CVA_PULSE_SAMPLES);
        assert_eq!(prepared.features.to_tensor().len(), 5);
        assert!(prepared
            .pulses
            .iter()
            .all(|value| (0.0..=1.0).contains(value)));
    }

    #[test]
    fn cva_features_recover_the_synthesised_heart_rate() {
        let segment = synthetic_ppg(PPG_SEGMENT_SAMPLES, 72.0);
        let prepared = cva_pulse(&segment).expect("prepared");
        let bpm = prepared.features.heart_rate_bpm;
        assert!(
            (bpm - 72.0).abs() < 3.0,
            "foot detection reported {bpm} bpm for a 72 bpm signal"
        );
    }

    #[test]
    fn cva_features_report_the_dc_level_and_a_positive_snr() {
        let segment = synthetic_ppg(PPG_SEGMENT_SAMPLES, 60.0);
        let prepared = cva_pulse(&segment).expect("prepared");
        // 2,000 baseline plus a 40/s ramp over thirty seconds averages to 2,600.
        assert!((prepared.features.mean_dc - 2_600.0).abs() < 5.0);
        assert!(prepared.features.snr_db > 10.0);
        assert!(prepared.features.max_min > 100.0);
    }

    #[test]
    fn a_flat_segment_reports_no_heart_rate_rather_than_a_wrong_one() {
        let prepared = cva_pulse(&[1_000.0; PPG_SEGMENT_SAMPLES]).expect("prepared");
        assert!(prepared.features.heart_rate_bpm.is_nan());
        assert_eq!(prepared.features.accepted, 0.0);
    }

    #[test]
    fn pulse_ppg_input_fits_the_window_and_normalises() {
        let segment = synthetic_ppg(PULSE_PPG_SEGMENT_SAMPLES, 65.0);
        let prepared = pulse_ppg_input(&segment, PPG_SAMPLE_RATE_HZ).expect("prepared");
        assert_eq!(prepared.len(), PULSE_PPG_SEGMENT_SAMPLES);
        let average = mean(&prepared);
        let deviation = population_std(&prepared, average);
        assert!(average.abs() < 1e-3, "z-scored mean was {average}");
        assert!(
            (deviation - 1.0).abs() < 1e-3,
            "z-scored deviation was {deviation}"
        );
    }

    #[test]
    fn pulse_ppg_input_pads_a_short_window_to_the_contract() {
        let short = synthetic_ppg(200, 65.0);
        let prepared = pulse_ppg_input(&short, PPG_SAMPLE_RATE_HZ).expect("prepared");
        assert_eq!(prepared.len(), PULSE_PPG_SEGMENT_SAMPLES);
    }

    #[test]
    fn pulse_ppg_input_resamples_a_faster_source_onto_the_50_hz_grid() {
        // A 100 Hz connector stream of the same four minutes must land on the same window.
        let fast: Vec<f32> = (0..PULSE_PPG_SEGMENT_SAMPLES * 2)
            .map(|index| {
                let seconds = index as f32 / 100.0;
                (TAU * (65.0 / 60.0) * seconds).sin()
            })
            .collect();
        let prepared = pulse_ppg_input(&fast, 100).expect("prepared");
        assert_eq!(prepared.len(), PULSE_PPG_SEGMENT_SAMPLES);
    }

    #[test]
    fn pulse_ppg_input_rejects_a_zero_rate() {
        let error = pulse_ppg_input(&[0.0, 1.0], 0).expect_err("zero rate");
        assert_eq!(error.code, codes::ML_MODEL_PREPROCESSING);
    }

    #[test]
    fn resampling_up_then_down_returns_close_to_the_original() {
        let original = synthetic_ppg(500, 60.0);
        let up = linear_resample(&original, 50, 200);
        let back = linear_resample(&up, 200, 50);
        assert_eq!(back.len(), original.len());
        let worst = original
            .iter()
            .zip(&back)
            .map(|(left, right)| (left - right).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst < 1.0, "round trip drifted by {worst}");
    }

    #[test]
    fn fit_or_pad_centres_in_both_directions() {
        assert_eq!(fit_or_pad(&[1.0, 2.0, 3.0, 4.0], 2), vec![2.0, 3.0]);
        assert_eq!(fit_or_pad(&[1.0, 2.0], 4), vec![0.0, 1.0, 2.0, 0.0]);
    }
}
