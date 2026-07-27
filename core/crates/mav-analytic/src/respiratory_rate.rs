//! Approximate sleeping respiratory rate (WHOOP-P6, `[WRS]`): breaths per minute from the
//! beat-interval stream via respiratory sinus arrhythmia — resample the tachogram onto an even
//! grid, detrend it, then per five-minute window peak-pick the breathing modulation and take the
//! median rate. Pure; `None` = no usable estimate. A wellness estimate, never clinical.
//!
//! The beat times are the recorded ones. An earlier version discarded them and rebuilt the time
//! axis as a running sum of the intervals themselves, which silently closed every gap in the
//! recording: a strap that stopped for ten minutes had those ten minutes deleted, the samples
//! either side became neighbours, and the resampler then interpolated a breathing waveform across
//! a hole. Runs are split on a real gap and analysed separately instead.

use crate::intervals::RUN_GAP_MS;
use crate::stats::median;

const RR_MIN_MS: f64 = 300.0;
const RR_MAX_MS: f64 = 2000.0;

const RSA_RESAMPLE_HZ: f64 = 4.0;
const RSA_DETREND_WINDOW_S: f64 = 8.0;
const RSA_MIN_PEAK_DISTANCE_S: f64 = 2.5;
const RSA_WINDOW_S: f64 = 300.0;
const RSA_MIN_BREATH_INTERVAL_S: f64 = 2.5;
const RSA_MAX_BREATH_INTERVAL_S: f64 = 10.0;
const MIN_BEATS_PER_RUN: usize = 30;

/// Plausible sleeping-respiratory-rate band (bpm); an estimate outside it is `None`.
pub const RESP_PLAUSIBLE_MIN_BPM: f64 = 8.0;
pub const RESP_PLAUSIBLE_MAX_BPM: f64 = 25.0;

/// Sleeping respiratory rate over the in-bed `[start, end]` window (unix seconds), from
/// `(ts, rr_ms)` beats. `None` on too little data.
pub fn resp_rate_from_rr(rr: &[(i64, u16)], start: i64, end: i64) -> Option<f64> {
    if end <= start {
        return None;
    }
    let mut beats: Vec<(f64, f64)> = rr
        .iter()
        .filter(|(ts, ms)| {
            (start..=end).contains(ts) && (RR_MIN_MS..=RR_MAX_MS).contains(&f64::from(*ms))
        })
        .map(|(ts, ms)| (*ts as f64, f64::from(*ms)))
        .collect();
    beats.sort_by(|left, right| left.0.total_cmp(&right.0));

    let gap_s = RUN_GAP_MS as f64 / 1_000.0;
    let mut rates = Vec::new();
    let mut run_start = 0usize;
    for index in 0..beats.len() {
        let breaks = index + 1 == beats.len() || beats[index + 1].0 - beats[index].0 > gap_s;
        if breaks {
            rates.extend(run_rates(&beats[run_start..=index]));
            run_start = index + 1;
        }
    }
    if rates.is_empty() {
        return None;
    }
    let estimate = median(&rates);
    (RESP_PLAUSIBLE_MIN_BPM..=RESP_PLAUSIBLE_MAX_BPM)
        .contains(&estimate)
        .then_some(estimate)
}

/// Breathing rates from every full window of one uninterrupted run of beats.
fn run_rates(run: &[(f64, f64)]) -> Vec<f64> {
    if run.len() < MIN_BEATS_PER_RUN {
        return Vec::new();
    }
    let span_s = run[run.len() - 1].0 - run[0].0;
    if span_s < RSA_WINDOW_S / 2.0 {
        return Vec::new();
    }

    // Resample the tachogram onto an even grid anchored to the run's own first beat.
    let dt = 1.0 / RSA_RESAMPLE_HZ;
    let cells = (span_s / dt) as usize + 1;
    if cells < 8 {
        return Vec::new();
    }
    let origin = run[0].0;
    let mut segment = 0usize;
    let grid: Vec<f64> = (0..cells)
        .map(|cell| {
            let at = origin + cell as f64 * dt;
            while segment + 2 < run.len() && run[segment + 1].0 < at {
                segment += 1;
            }
            let ((t0, v0), (t1, v1)) = (run[segment], run[segment + 1]);
            if t1 <= t0 {
                v0
            } else {
                v0 + ((at - t0) / (t1 - t0)).clamp(0.0, 1.0) * (v1 - v0)
            }
        })
        .collect();

    let half_window = ((RSA_DETREND_WINDOW_S * RSA_RESAMPLE_HZ / 2.0).round() as usize).max(1);
    let detrended: Vec<f64> = (0..cells)
        .map(|cell| {
            let low = cell.saturating_sub(half_window);
            let high = (cell + half_window).min(cells - 1);
            grid[cell] - grid[low..=high].iter().sum::<f64>() / (high - low + 1) as f64
        })
        .collect();
    if population_sd(&detrended) <= 1e-9 {
        return Vec::new();
    }

    let min_distance = ((RSA_MIN_PEAK_DISTANCE_S * RSA_RESAMPLE_HZ).round() as usize).max(2);
    let window = ((RSA_WINDOW_S * RSA_RESAMPLE_HZ).round() as usize).max(min_distance * 3);
    (0..cells)
        .step_by(window)
        .filter_map(|from| {
            let to = (from + window).min(cells);
            (to - from >= min_distance * 3)
                .then(|| breath_rate(&detrended[from..to], min_distance, dt))?
        })
        .collect()
}

/// The median breathing rate in one window, from the spacing of its modulation peaks.
fn breath_rate(window: &[f64], min_distance: usize, dt: f64) -> Option<f64> {
    let peaks = find_peaks(window, min_distance, 0.0);
    if peaks.len() < 3 {
        return None;
    }
    let intervals: Vec<f64> = peaks
        .windows(2)
        .map(|pair| (pair[1] - pair[0]) as f64 * dt)
        .filter(|seconds| (RSA_MIN_BREATH_INTERVAL_S..=RSA_MAX_BREATH_INTERVAL_S).contains(seconds))
        .collect();
    if intervals.len() < 2 {
        return None;
    }
    let spacing = median(&intervals);
    (spacing > 0.0).then(|| 60.0 / spacing)
}

/// Population standard deviation (divides by `n`); `0.0` for an empty slice.
fn population_sd(xs: &[f64]) -> f64 {
    if xs.is_empty() {
        return 0.0;
    }
    let m = xs.iter().sum::<f64>() / xs.len() as f64;
    (xs.iter().map(|v| (v - m) * (v - m)).sum::<f64>() / xs.len() as f64).sqrt()
}

/// Local-maxima peak finder mirroring `scipy.find_peaks(distance, height)`: a plateau-aware
/// maximum at or above `height`, with peaks closer than `distance` resolved by keeping the taller.
fn find_peaks(x: &[f64], distance: usize, height: f64) -> Vec<usize> {
    let n = x.len();
    if n < 3 {
        return Vec::new();
    }
    let mut candidates = Vec::new();
    let mut i = 1;
    while i < n - 1 {
        if x[i] > x[i - 1] && x[i] >= height {
            let mut j = i;
            while j + 1 < n && x[j + 1] == x[i] {
                j += 1;
            }
            if j + 1 < n && x[j + 1] < x[i] {
                candidates.push((i + j) / 2);
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    if distance <= 1 || candidates.is_empty() {
        return candidates;
    }
    let mut by_height: Vec<usize> = (0..candidates.len()).collect();
    by_height.sort_by(|&a, &b| x[candidates[b]].total_cmp(&x[candidates[a]]));
    let mut keep = vec![true; candidates.len()];
    for &pi in &by_height {
        if !keep[pi] {
            continue;
        }
        let p = candidates[pi] as isize;
        for qi in 0..candidates.len() {
            if qi != pi && keep[qi] && (candidates[qi] as isize - p).unsigned_abs() < distance {
                keep[qi] = false;
            }
        }
    }
    candidates
        .iter()
        .enumerate()
        .filter(|(off, _)| keep[*off])
        .map(|(_, &c)| c)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Synthetic generator: mean HR with a known-Hz RSA modulation, so the recovered rate can be
    /// cross-checked against the planted breathing frequency.
    fn synth(
        breath_hz: f64,
        base_rr_ms: f64,
        amp_ms: f64,
        span_s: f64,
    ) -> (Vec<(i64, u16)>, i64, i64) {
        let start = 1_700_000_000_i64;
        let mut rows = Vec::new();
        let mut t_sec = 0.0_f64;
        while t_sec < span_s {
            let rr_ms =
                base_rr_ms + amp_ms * (2.0 * std::f64::consts::PI * breath_hz * t_sec).sin();
            t_sec += rr_ms / 1000.0;
            rows.push((start + t_sec as i64, rr_ms as u16));
        }
        let end = start + t_sec as i64;
        (rows, start, end)
    }

    #[test]
    fn recovers_known_breathing_frequency() {
        // 15 breaths/min (0.25 Hz), mean HR 60, ±40 ms RSA, ~7 min.
        let (rows, start, end) = synth(0.25, 1000.0, 40.0, 420.0);
        let est = resp_rate_from_rr(&rows, start, end).expect("finite estimate");
        assert!((est - 15.0).abs() <= 3.0, "expected ~15, got {est}");
    }

    #[test]
    fn slow_breather_is_not_doubled() {
        // 11 breaths/min must read ~11, not the doubled ~20-21 harmonic.
        let (rows, start, end) = synth(11.0 / 60.0, 60000.0 / 55.0, 45.0, 480.0);
        let est = resp_rate_from_rr(&rows, start, end).expect("finite estimate");
        assert!((est - 11.0).abs() <= 2.0, "expected ~11, got {est}");
        assert!(est < 16.0, "must not double toward ~22, got {est}");
    }

    #[test]
    fn too_few_beats_is_none() {
        let start = 1_700_000_000_i64;
        let rows = vec![(start + 1, 1000u16), (start + 2, 1000), (start + 3, 1000)];
        assert!(resp_rate_from_rr(&rows, start, start + 10).is_none());
        assert!(resp_rate_from_rr(&[], start, start + 10).is_none());
    }

    #[test]
    fn empty_or_inverted_window_is_none() {
        let (rows, start, end) = synth(0.25, 1000.0, 40.0, 420.0);
        assert!(resp_rate_from_rr(&rows, end, start).is_none());
    }

    /// The finding this module was rewritten for. A ten-minute dropout used to vanish, because the
    /// time axis was rebuilt from the intervals themselves; the two halves became neighbours and
    /// the resampler drew a breathing waveform across the hole. Splitting on the real gap means the
    /// answer is the same as it would have been without the second half.
    #[test]
    fn a_recording_gap_is_not_closed_by_reconstructing_the_time_axis() {
        let (first, start, end) = synth(0.25, 1000.0, 40.0, 420.0);
        let alone = resp_rate_from_rr(&first, start, end).expect("the first run reads");

        // The same run again, ten minutes later and breathing at a different rate.
        let (second, _, _) = synth(0.25, 1000.0, 40.0, 420.0);
        let offset = end - start + 600;
        let mut both = first.clone();
        both.extend(second.iter().map(|(ts, ms)| (ts + offset, *ms)));
        let together = resp_rate_from_rr(&both, start, start + offset + (end - start))
            .expect("both runs read");

        assert!(
            (together - alone).abs() < 1.0,
            "a silent gap must not change the answer: {alone} alone, {together} together"
        );
    }
}
