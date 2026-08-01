//! R-peak detection over a single-lead electrical cardiac waveform.
//!
//! This is the Pan–Tompkins detector (Pan & Tompkins, *A Real-Time QRS Detection Algorithm*, IEEE
//! Trans. Biomed. Eng. 32(3), 1985): band-pass, derivative, squaring, moving-window integration,
//! then the paper's two-threshold adaptive decision with its refractory period, T-wave
//! discrimination and search-back. One deviation is deliberate and stated here rather than hidden:
//! the paper specifies integer filters designed for 200 Hz, and straps sample at whatever rate
//! they sample at, so the band-pass is a Butterworth cascade computed for the actual rate. Every
//! decision constant is the paper's.
//!
//! What this buys is the only route to genuine heart-rate variability from a device that exposes
//! a waveform rather than intervals. An optical pulse is a different physiological event and stays
//! [`mav_model::stream::StreamKind::PulseInterval`]; see docs/analytics.md.

use mav_model::version::Version;

pub const R_PEAK_ALGORITHM: &str = "pan_tompkins_r_peak";
pub const R_PEAK_VERSION: Version = Version::new(1, 0, 0);

/// Band-pass corners, in Hz: the band where QRS energy dominates muscle noise, baseline wander and
/// T waves.
const PASSBAND_HZ: (f64, f64) = (5.0, 15.0);
/// Integration window. Roughly the width of a QRS complex, so the integrator's output rises once
/// per beat rather than once per deflection.
const INTEGRATION_MS: f64 = 150.0;
/// No second beat can occur within this of the last — physiologically impossible, and it is what
/// stops the detector counting the two halves of one complex.
const REFRACTORY_MS: f64 = 200.0;
/// A peak this soon after a beat is a T wave unless its slope says otherwise.
const T_WAVE_WINDOW_MS: f64 = 360.0;
/// Once the interval since the last beat exceeds this multiple of the running average, the
/// detector goes back over the window at the lower threshold.
const SEARCH_BACK_FACTOR: f64 = 1.66;
/// How many beats the running interval average is taken over.
const RR_AVERAGE_BEATS: usize = 8;
/// The paper's threshold placement between the running noise and signal peak estimates.
const THRESHOLD_FRACTION: f64 = 0.25;
/// The paper's exponential update weight for both peak estimates.
const PEAK_UPDATE: f64 = 0.125;
/// How long the initial signal and noise estimates are learned over.
const LEARNING_MS: f64 = 2_000.0;
/// A waveform is detected over chunks this long so that memory stays bounded whatever the length
/// of the recording — the filters, the squaring and the integrator each hold a copy.
const CHUNK_MS: f64 = 30_000.0;
/// Chunks overlap by this much so no beat falls in a seam. Every interval is keyed by the device
/// time of its closing beat, so an interval detected in both chunks is the same interval.
const CHUNK_OVERLAP_MS: f64 = 3_000.0;

/// R-peak positions, as indices into the input samples, in order.
///
/// The input is raw converter counts; no scale is assumed and none is invented, because every
/// stage below is either linear or a comparison against a threshold derived from the signal
/// itself. A signal too short to learn a threshold from returns nothing.
///
/// No *offset* is assumed either. The band-pass rejects DC in steady state but starts from rest,
/// so a waveform sitting on a converter pedestal — a WHOOP MG electrode idles near 1,220 counts —
/// drives a start transient orders of magnitude larger than any QRS. That transient lands inside
/// the learning window, sets the signal-peak estimate from itself, and leaves an adaptive
/// threshold no real beat can clear. Removing the baseline first costs nothing on an already
/// centred waveform and is what makes an unmodified converter stream detectable at all.
pub fn r_peaks(signal: &[f64], sample_rate_hz: f64) -> Vec<usize> {
    let per_ms = sample_rate_hz / 1_000.0;
    let window = ((INTEGRATION_MS * per_ms).round() as usize).max(1);
    if sample_rate_hz <= 0.0 || signal.len() < window * 4 {
        return Vec::new();
    }

    let band = band_pass(&without_baseline(signal), sample_rate_hz);
    let integrated = integrate(&square(&derivative(&band)), window);

    let refractory = (REFRACTORY_MS * per_ms).round() as usize;
    let t_wave = (T_WAVE_WINDOW_MS * per_ms).round() as usize;
    let learning = ((LEARNING_MS * per_ms).round() as usize).min(integrated.len());

    let mut signal_peak = integrated[..learning]
        .iter()
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let mut noise_peak = integrated[..learning].iter().sum::<f64>() / learning as f64;
    if !signal_peak.is_finite() || signal_peak <= noise_peak {
        return Vec::new();
    }

    let mut beats: Vec<usize> = Vec::new();
    let mut intervals: Vec<usize> = Vec::new();

    for (at, height) in local_maxima(&integrated, refractory) {
        let threshold = noise_peak + THRESHOLD_FRACTION * (signal_peak - noise_peak);
        // Search-back: an interval far longer than the running average means a beat was missed, so
        // a candidate over half the threshold is reconsidered, exactly as the paper prescribes.
        let overdue = || {
            let average = average_interval(&intervals);
            average > 0.0
                && beats
                    .last()
                    .is_some_and(|last| (at - last) as f64 > SEARCH_BACK_FACTOR * average)
        };
        let accepted = height > threshold || (height > threshold / 2.0 && overdue());

        if !accepted {
            noise_peak = PEAK_UPDATE * height + (1.0 - PEAK_UPDATE) * noise_peak;
            continue;
        }

        // T-wave discrimination: a deflection close behind a beat is the repolarisation wave
        // unless it rises at least as steeply as the beat did.
        if let Some(&last) = beats.last() {
            if at - last < t_wave && slope(&band, at) < slope(&band, last) / 2.0 {
                noise_peak = PEAK_UPDATE * height + (1.0 - PEAK_UPDATE) * noise_peak;
                continue;
            }
            intervals.push(at - last);
            if intervals.len() > RR_AVERAGE_BEATS {
                intervals.remove(0);
            }
        }
        signal_peak = PEAK_UPDATE * height + (1.0 - PEAK_UPDATE) * signal_peak;
        beats.push(at);
    }

    // The integrator and the filters delay the signal; the R peak itself is the largest excursion
    // of the band-passed waveform inside the window the integrator was summing.
    beats
        .into_iter()
        .map(|at| peak_within(&band, at.saturating_sub(window), at))
        .collect()
}

/// Beat-to-beat intervals in milliseconds, one per pair of successive R peaks.
pub fn rr_intervals_ms(signal: &[f64], sample_rate_hz: f64) -> Vec<(usize, f64)> {
    r_peaks(signal, sample_rate_hz)
        .windows(2)
        .map(|pair| {
            (
                pair[1],
                (pair[1] - pair[0]) as f64 * 1_000.0 / sample_rate_hz,
            )
        })
        .collect()
}

/// Beat-to-beat intervals from a timed waveform: `(device time in milliseconds, counts)` in, and
/// `(device time of the closing beat, interval in milliseconds)` out.
///
/// The sample rate is inferred from the timestamps rather than assumed, and a break longer than
/// twice the sample spacing starts a new recording — a detector run across a dropout would read
/// the join as one enormous beat.
pub fn intervals_from_timed(samples: &[(i64, f64)]) -> Vec<(i64, f64)> {
    let Some(spacing_ms) = median_spacing(samples) else {
        return Vec::new();
    };
    let rate_hz = 1_000.0 / spacing_ms;
    let mut out = Vec::new();
    let mut run_start = 0usize;
    for index in 0..samples.len() {
        let breaks = index + 1 == samples.len()
            || (samples[index + 1].0 - samples[index].0) as f64 > 2.0 * spacing_ms;
        if !breaks {
            continue;
        }
        out.extend(intervals_in_run(
            &samples[run_start..=index],
            rate_hz,
            spacing_ms,
        ));
        run_start = index + 1;
    }
    out.sort_unstable_by_key(|(at, _)| *at);
    out.dedup_by_key(|(at, _)| *at);
    out
}

/// One contiguous recording, detected in overlapping chunks so peak memory does not grow with the
/// length of the recording.
fn intervals_in_run(run: &[(i64, f64)], rate_hz: f64, spacing_ms: f64) -> Vec<(i64, f64)> {
    let chunk = ((CHUNK_MS / spacing_ms).round() as usize).max(1);
    let overlap = ((CHUNK_OVERLAP_MS / spacing_ms).round() as usize).min(chunk / 2);
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < run.len() {
        let end = (start + chunk).min(run.len());
        let counts: Vec<f64> = run[start..end].iter().map(|(_, value)| *value).collect();
        out.extend(
            rr_intervals_ms(&counts, rate_hz)
                .into_iter()
                .map(|(at, interval_ms)| (run[start + at].0, interval_ms)),
        );
        if end == run.len() {
            break;
        }
        start = end - overlap;
    }
    out
}

fn median_spacing(samples: &[(i64, f64)]) -> Option<f64> {
    let mut gaps: Vec<i64> = samples
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].0)
        .filter(|gap| *gap > 0)
        .collect();
    if gaps.is_empty() {
        return None;
    }
    gaps.sort_unstable();
    Some(gaps[gaps.len() / 2] as f64)
}

/// The waveform with its converter pedestal removed. The median, not the mean: QRS complexes are
/// one-sided excursions, so the mean of a slow rhythm sits above the isoelectric line it is
/// supposed to find, and a window that opens part-way through a complex pulls it further.
fn without_baseline(signal: &[f64]) -> Vec<f64> {
    let mut sorted: Vec<f64> = signal.to_vec();
    sorted.sort_unstable_by(f64::total_cmp);
    let baseline = sorted[sorted.len() / 2];
    if !baseline.is_finite() {
        return signal.to_vec();
    }
    signal.iter().map(|sample| sample - baseline).collect()
}

/// Second-order Butterworth high-pass then low-pass, each run forwards and then backwards so the
/// band-pass has no phase delay. Zero phase matters because the R peak's *position* in the
/// filtered signal is what the detector reports, and a one-directional pass would move every beat
/// later by the filter's group delay — tens of milliseconds, straight into every interval at the
/// edges of a window.
fn band_pass(signal: &[f64], rate: f64) -> Vec<f64> {
    let mut pass = zero_phase(signal, || Butterworth::high_pass(PASSBAND_HZ.0, rate));
    pass = zero_phase(&pass, || Butterworth::low_pass(PASSBAND_HZ.1, rate));
    pass
}

fn zero_phase(signal: &[f64], filter: impl Fn() -> Butterworth) -> Vec<f64> {
    let mut forward = biquad(signal, filter());
    forward.reverse();
    let mut back = biquad(&forward, filter());
    back.reverse();
    back
}

struct Butterworth {
    feed_forward: [f64; 3],
    feed_back: [f64; 2],
}

impl Butterworth {
    /// Bilinear-transformed second-order sections, Q = 1/sqrt(2).
    fn sections(cutoff: f64, rate: f64) -> (f64, f64, f64) {
        let omega = (std::f64::consts::PI * cutoff / rate).tan();
        let scale = 1.0 / (1.0 + std::f64::consts::SQRT_2 * omega + omega * omega);
        (omega, scale, omega * omega)
    }

    fn low_pass(cutoff: f64, rate: f64) -> Self {
        let (omega, scale, squared) = Self::sections(cutoff, rate);
        Self {
            feed_forward: [squared * scale, 2.0 * squared * scale, squared * scale],
            feed_back: [
                2.0 * (squared - 1.0) * scale,
                (1.0 - std::f64::consts::SQRT_2 * omega + squared) * scale,
            ],
        }
    }

    fn high_pass(cutoff: f64, rate: f64) -> Self {
        let (omega, scale, squared) = Self::sections(cutoff, rate);
        Self {
            feed_forward: [scale, -2.0 * scale, scale],
            feed_back: [
                2.0 * (squared - 1.0) * scale,
                (1.0 - std::f64::consts::SQRT_2 * omega + squared) * scale,
            ],
        }
    }
}

fn biquad(signal: &[f64], filter: Butterworth) -> Vec<f64> {
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    signal
        .iter()
        .map(|&x| {
            let y = filter.feed_forward[0] * x
                + filter.feed_forward[1] * x1
                + filter.feed_forward[2] * x2
                - filter.feed_back[0] * y1
                - filter.feed_back[1] * y2;
            x2 = x1;
            x1 = x;
            y2 = y1;
            y1 = y;
            y
        })
        .collect()
}

/// The paper's five-point derivative, which emphasises the steep QRS slope over slower waves.
fn derivative(signal: &[f64]) -> Vec<f64> {
    (0..signal.len())
        .map(|index| {
            let at = |offset: usize| {
                signal
                    .get(index.wrapping_sub(offset))
                    .copied()
                    .unwrap_or(0.0)
            };
            (2.0 * signal[index] + at(1) - at(3) - 2.0 * at(4)) / 8.0
        })
        .collect()
}

fn square(signal: &[f64]) -> Vec<f64> {
    signal.iter().map(|value| value * value).collect()
}

/// Moving-window integration by running sum, so the cost is one add and one subtract per sample
/// rather than a window-sized loop.
fn integrate(signal: &[f64], window: usize) -> Vec<f64> {
    let mut sum = 0.0;
    signal
        .iter()
        .enumerate()
        .map(|(index, value)| {
            sum += value;
            if index >= window {
                sum -= signal[index - window];
            }
            sum / window as f64
        })
        .collect()
}

/// Peaks separated by at least the refractory period, each the largest value in its neighbourhood.
fn local_maxima(signal: &[f64], refractory: usize) -> Vec<(usize, f64)> {
    let mut peaks: Vec<(usize, f64)> = Vec::new();
    for index in 1..signal.len().saturating_sub(1) {
        if signal[index] <= signal[index - 1] || signal[index] < signal[index + 1] {
            continue;
        }
        match peaks.last_mut() {
            Some(last) if index - last.0 < refractory => {
                if signal[index] > last.1 {
                    *last = (index, signal[index]);
                }
            }
            _ => peaks.push((index, signal[index])),
        }
    }
    peaks
}

/// Steepest rise in the eight samples before `at` — the paper's slope test for telling a QRS from
/// the slower T wave.
fn slope(signal: &[f64], at: usize) -> f64 {
    let start = at.saturating_sub(8);
    signal[start..=at.min(signal.len() - 1)]
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .fold(0.0, f64::max)
}

fn peak_within(signal: &[f64], from: usize, to: usize) -> usize {
    (from..=to.min(signal.len() - 1))
        .max_by(|left, right| signal[*left].abs().total_cmp(&signal[*right].abs()))
        .unwrap_or(to)
}

fn average_interval(intervals: &[usize]) -> f64 {
    if intervals.is_empty() {
        return 0.0;
    }
    intervals.iter().sum::<usize>() as f64 / intervals.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE_HZ: f64 = 250.0;

    /// A worn WHOOP MG, finger on the electrode: raw converter counts on a ~1,220 pedestal, the
    /// shape no synthetic fixture in this directory has. Before the baseline removal this whole
    /// 45-second capture yielded a single R peak, so every calibration window read `Contact` and
    /// no ECG capture could start on real hardware.
    #[test]
    fn live_mg_counts_detect_beats_through_the_converter_pedestal() {
        let signal: Vec<f64> = include_str!("../../../../fixtures/ecg/mg_electrode_100hz_v1.csv")
            .lines()
            .skip(1)
            .map(|line| line.split(',').nth(1).unwrap().parse().unwrap())
            .collect();
        assert_eq!(signal.len(), 4_500);
        let mean = signal.iter().sum::<f64>() / signal.len() as f64;
        assert!(mean > 1_000.0, "fixture must keep its pedestal, got {mean}");

        let peaks = r_peaks(&signal, 100.0);
        assert_eq!(peaks.len(), 59, "expected the fixture's pinned beat count");
        let bpm = peaks.len() as f64 * 60.0 / (signal.len() as f64 / 100.0);
        assert!((bpm - 78.7).abs() < 0.5, "implied rate {bpm} bpm");

        // The same waveform with the pedestal already gone must find the same beats: the fix is a
        // baseline removal, not a change to what counts as a beat.
        let centred: Vec<f64> = signal.iter().map(|sample| sample - mean).collect();
        assert_eq!(r_peaks(&centred, 100.0), peaks);
    }

    /// A synthetic lead II: a sharp QRS, a rounded T wave, and a small P wave, repeated at a set
    /// interval. Shaped so the T wave is tall enough to be counted by a detector without T-wave
    /// discrimination, which is exactly what the paper's slope test exists to prevent.
    fn synthetic_ecg(beats: usize, interval_ms: f64, rate_hz: f64) -> (Vec<f64>, Vec<usize>) {
        let period = (interval_ms * rate_hz / 1_000.0).round() as usize;
        let mut signal = vec![0.0; beats * period + period];
        let mut expected = Vec::new();
        let gaussian = |x: f64, centre: f64, width: f64, height: f64| {
            let z = (x - centre) / width;
            height * (-0.5 * z * z).exp()
        };
        for beat in 0..beats {
            let r_at = beat * period + period / 2;
            expected.push(r_at);
            for offset in 0..period {
                let index = beat * period + offset;
                let t = (offset as f64 - period as f64 / 2.0) * 1_000.0 / rate_hz;
                signal[index] = gaussian(t, 0.0, 8.0, 1.0)
                    + gaussian(t, 24.0, 10.0, -0.20)
                    + gaussian(t, 250.0, 45.0, 0.28)
                    + gaussian(t, -180.0, 30.0, 0.10);
            }
        }
        (signal, expected)
    }

    #[test]
    fn a_synthetic_recording_yields_one_peak_per_beat_at_the_right_place() {
        let (signal, expected) = synthetic_ecg(12, 800.0, RATE_HZ);
        let found = r_peaks(&signal, RATE_HZ);
        assert_eq!(found.len(), expected.len(), "one peak per beat");
        let tolerance = (0.030 * RATE_HZ) as usize;
        for (found, expected) in found.iter().zip(&expected) {
            assert!(
                found.abs_diff(*expected) <= tolerance,
                "peak at {found} should be within {tolerance} samples of {expected}"
            );
        }
    }

    /// The T wave in the fixture is a quarter of the R amplitude and rounded. Counting it would
    /// double the heart rate, which is the classic failure this detector's slope test prevents.
    #[test]
    fn the_t_wave_is_not_counted_as_a_beat() {
        let (signal, _) = synthetic_ecg(20, 750.0, RATE_HZ);
        assert_eq!(r_peaks(&signal, RATE_HZ).len(), 20);
    }

    #[test]
    fn intervals_recover_the_beat_period() {
        let (signal, _) = synthetic_ecg(15, 900.0, RATE_HZ);
        let intervals = rr_intervals_ms(&signal, RATE_HZ);
        assert_eq!(intervals.len(), 14);
        for (_, ms) in &intervals {
            assert!((ms - 900.0).abs() < 20.0, "recovered {ms} ms, expected 900");
        }
    }

    /// A strap that streams at 100 Hz has to work as well as one at 250, because the band-pass is
    /// designed for the rate rather than assumed.
    #[test]
    fn detection_works_at_the_rate_the_hardware_actually_samples() {
        let (signal, _) = synthetic_ecg(15, 850.0, 100.0);
        let intervals = rr_intervals_ms(&signal, 100.0);
        assert_eq!(intervals.len(), 14);
        for (_, ms) in &intervals {
            assert!((ms - 850.0).abs() < 25.0, "recovered {ms} ms, expected 850");
        }
    }

    #[test]
    fn a_recording_too_short_to_learn_from_detects_nothing() {
        assert!(r_peaks(&[0.0; 10], RATE_HZ).is_empty());
        assert!(r_peaks(&[], RATE_HZ).is_empty());
        assert!(r_peaks(&[1.0; 5_000], 0.0).is_empty());
    }

    #[test]
    fn flat_silence_produces_no_beats() {
        assert!(r_peaks(&[0.0; 5_000], RATE_HZ).is_empty());
    }

    /// A dropout is not a beat. Two recordings either side of a five-second silence contribute
    /// their own intervals and nothing spanning the gap, and the rate comes from the timestamps
    /// rather than from an assumption about the hardware.
    #[test]
    fn a_recording_gap_never_becomes_one_enormous_interval() {
        let (signal, _) = synthetic_ecg(10, 800.0, RATE_HZ);
        let step_ms = (1_000.0 / RATE_HZ) as i64;
        let timed = |offset_ms: i64| {
            signal
                .iter()
                .enumerate()
                .map(|(index, value)| (offset_ms + index as i64 * step_ms, *value))
                .collect::<Vec<_>>()
        };
        let mut samples = timed(0);
        let end = samples.last().expect("non-empty").0;
        samples.extend(timed(end + 5_000));

        let intervals = intervals_from_timed(&samples);
        assert!(!intervals.is_empty());
        for (_, ms) in &intervals {
            assert!(
                (ms - 800.0).abs() < 20.0,
                "recovered {ms} ms; a bridged gap would read as thousands"
            );
        }
    }

    /// A recording longer than one detection chunk must not lose beats at the seams or report the
    /// seam interval twice.
    #[test]
    fn chunked_detection_recovers_every_beat_exactly_once() {
        let beats = 120;
        let (signal, _) = synthetic_ecg(beats, 800.0, RATE_HZ);
        let step_ms = (1_000.0 / RATE_HZ) as i64;
        let samples: Vec<(i64, f64)> = signal
            .iter()
            .enumerate()
            .map(|(index, value)| (index as i64 * step_ms, *value))
            .collect();
        assert!(
            samples.last().expect("non-empty").0 > 30_000,
            "the fixture has to be longer than one chunk to exercise the seams"
        );

        let intervals = intervals_from_timed(&samples);
        assert_eq!(intervals.len(), beats - 1);
        for (_, ms) in &intervals {
            assert!((ms - 800.0).abs() < 20.0, "recovered {ms} ms");
        }
    }
}
