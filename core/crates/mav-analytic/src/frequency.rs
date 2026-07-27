//! Frequency-domain variability over the 1996 ESC/NASPE Task Force bands.
//!
//! Beats do not arrive on an even grid, and the usual way round that is to resample the tachogram
//! before an FFT — which invents values the heart never produced, and this pipeline does not
//! interpolate (docs/pipeline.md). The Lomb–Scargle periodogram (Lomb 1976; Scargle 1982) is a
//! least-squares fit of sinusoids directly to unevenly sampled data, so it needs no grid at all;
//! Laguna, Moody and Mark (1998) showed it estimates HRV spectra more accurately than resampling
//! exactly because of that. It is the right transform for this data, not a compromise.
//!
//! Absolute band powers depend on the periodogram's normalisation convention, so this one is
//! stated: the spectrum is scaled so that its integral equals the variance of the series, which is
//! Parseval's relation and makes the powers millisecond-squared. The normalised units and the
//! LF/HF ratio are convention-free, and the Task Force recommends reporting LF and HF that way.

use serde::{Deserialize, Serialize};

/// Task Force band edges, in hertz.
pub const VLF_BAND: (f64, f64) = (0.0033, 0.04);
pub const LF_BAND: (f64, f64) = (0.04, 0.15);
pub const HF_BAND: (f64, f64) = (0.15, 0.40);

/// The shortest recording the Task Force's short-term bands are defined over. Below this the
/// lowest band has not completed a cycle and its power is a fitting artefact.
pub const MIN_SPAN_SECONDS: f64 = 120.0;
/// The fewest beats worth fitting sinusoids to.
pub const MIN_BEATS: usize = 32;
/// How finely the spectrum is sampled relative to the natural resolution `1/T`. Four is the
/// conventional oversampling for a Lomb periodogram.
const OVERSAMPLE: f64 = 4.0;

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct FrequencyDomainHrv {
    /// Very-low-frequency power, ms². Reported, but the Task Force warns it is not interpretable
    /// over a short recording.
    pub vlf_power_ms2: f64,
    pub lf_power_ms2: f64,
    pub hf_power_ms2: f64,
    pub total_power_ms2: f64,
    /// LF as a share of the LF+HF power, in percent — the convention-free form.
    pub lf_normalized: f64,
    pub hf_normalized: f64,
    pub lf_hf_ratio: f64,
    /// How long the analysed recording actually spanned, so a reader can judge the low bands.
    pub span_seconds: f64,
}

/// Band powers over one uninterrupted run of beats, given as `(beat time in milliseconds, interval
/// in milliseconds)`. `None` when the run is too short or too sparse for the bands to mean
/// anything, or when the intervals do not vary at all.
pub fn band_powers(beats: &[(i64, f64)]) -> Option<FrequencyDomainHrv> {
    if beats.len() < MIN_BEATS {
        return None;
    }
    let times: Vec<f64> = beats.iter().map(|(at, _)| *at as f64 / 1_000.0).collect();
    let values: Vec<f64> = beats.iter().map(|(_, interval)| *interval).collect();
    let span_seconds = times.last()? - times.first()?;
    if span_seconds < MIN_SPAN_SECONDS {
        return None;
    }

    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let centred: Vec<f64> = values.iter().map(|value| value - mean).collect();
    let variance =
        centred.iter().map(|value| value * value).sum::<f64>() / (centred.len() - 1) as f64;
    if variance <= 0.0 {
        return None;
    }

    let step = 1.0 / (OVERSAMPLE * span_seconds);
    let bins = ((HF_BAND.1 - step) / step) as usize;
    let spectrum: Vec<(f64, f64)> = (1..=bins)
        .map(|bin| {
            let frequency = bin as f64 * step;
            (frequency, periodogram(&times, &centred, frequency))
        })
        .collect();

    // Parseval: scale the spectrum so integrating it returns the variance the beats actually have.
    let raw_total: f64 = spectrum.iter().map(|(_, power)| power * step).sum();
    if raw_total <= 0.0 {
        return None;
    }
    let scale = variance / raw_total;
    let power_in = |band: (f64, f64)| -> f64 {
        spectrum
            .iter()
            .filter(|(frequency, _)| (band.0..band.1).contains(frequency))
            .map(|(_, power)| power * step * scale)
            .sum()
    };

    let (vlf, lf, hf) = (power_in(VLF_BAND), power_in(LF_BAND), power_in(HF_BAND));
    let short_term = lf + hf;
    Some(FrequencyDomainHrv {
        vlf_power_ms2: vlf,
        lf_power_ms2: lf,
        hf_power_ms2: hf,
        total_power_ms2: vlf + short_term,
        lf_normalized: if short_term > 0.0 {
            lf / short_term * 100.0
        } else {
            0.0
        },
        hf_normalized: if short_term > 0.0 {
            hf / short_term * 100.0
        } else {
            0.0
        },
        lf_hf_ratio: if hf > 0.0 { lf / hf } else { f64::INFINITY },
        span_seconds,
    })
}

/// The Lomb–Scargle power at one frequency: the least-squares fit of a sinusoid to the samples,
/// with the time offset `tau` that makes the sine and cosine components orthogonal on this
/// particular set of sample times.
fn periodogram(times: &[f64], centred: &[f64], frequency: f64) -> f64 {
    let omega = 2.0 * std::f64::consts::PI * frequency;
    let (sin_two, cos_two) = times.iter().fold((0.0, 0.0), |(sines, cosines), at| {
        let angle = 2.0 * omega * at;
        (sines + angle.sin(), cosines + angle.cos())
    });
    let tau = sin_two.atan2(cos_two) / (2.0 * omega);

    let mut cosine_sum = 0.0;
    let mut sine_sum = 0.0;
    let mut cosine_norm = 0.0;
    let mut sine_norm = 0.0;
    for (at, value) in times.iter().zip(centred) {
        let (sine, cosine) = (omega * (at - tau)).sin_cos();
        cosine_sum += value * cosine;
        sine_sum += value * sine;
        cosine_norm += cosine * cosine;
        sine_norm += sine * sine;
    }
    let cosine_term = if cosine_norm > 0.0 {
        cosine_sum * cosine_sum / cosine_norm
    } else {
        0.0
    };
    let sine_term = if sine_norm > 0.0 {
        sine_sum * sine_sum / sine_norm
    } else {
        0.0
    };
    0.5 * (cosine_term + sine_term)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tachogram with a sinusoidal modulation planted at a known frequency, sampled at the beats
    /// themselves — so the sample times are as uneven as the heartbeat that produced them.
    fn modulated(frequency_hz: f64, amplitude_ms: f64, seconds: f64) -> Vec<(i64, f64)> {
        let mut beats = Vec::new();
        let mut at_ms = 0.0;
        while at_ms / 1_000.0 < seconds {
            let phase = 2.0 * std::f64::consts::PI * frequency_hz * (at_ms / 1_000.0);
            let interval = 1_000.0 + amplitude_ms * phase.sin();
            at_ms += interval;
            beats.push((at_ms as i64, interval));
        }
        beats
    }

    #[test]
    fn a_respiratory_modulation_lands_in_the_high_frequency_band() {
        // 0.25 Hz is 15 breaths a minute, squarely inside HF.
        let bands = band_powers(&modulated(0.25, 40.0, 300.0)).expect("five minutes of beats");
        assert!(
            bands.hf_normalized > 90.0,
            "expected HF dominance, got {:.1} n.u.",
            bands.hf_normalized
        );
        assert!(bands.lf_hf_ratio < 0.2);
    }

    #[test]
    fn a_baroreflex_modulation_lands_in_the_low_frequency_band() {
        // 0.1 Hz is the classic Mayer wave, squarely inside LF.
        let bands = band_powers(&modulated(0.1, 40.0, 300.0)).expect("five minutes of beats");
        assert!(
            bands.lf_normalized > 90.0,
            "expected LF dominance, got {:.1} n.u.",
            bands.lf_normalized
        );
        assert!(bands.lf_hf_ratio > 5.0);
    }

    /// Parseval: the spectrum is scaled to hold the variance the beats actually have, so total
    /// power has to match the series variance rather than float on a normalisation convention.
    #[test]
    fn total_power_matches_the_variance_of_the_intervals() {
        let beats = modulated(0.25, 40.0, 300.0);
        let intervals: Vec<f64> = beats.iter().map(|(_, interval)| *interval).collect();
        let mean = intervals.iter().sum::<f64>() / intervals.len() as f64;
        let variance = intervals
            .iter()
            .map(|v| (v - mean) * (v - mean))
            .sum::<f64>()
            / (intervals.len() - 1) as f64;

        let bands = band_powers(&beats).expect("reads");
        assert!(
            (bands.total_power_ms2 - variance).abs() / variance < 0.25,
            "total {} against variance {variance}",
            bands.total_power_ms2
        );
    }

    #[test]
    fn a_recording_too_short_for_the_bands_reads_nothing() {
        assert_eq!(band_powers(&modulated(0.25, 40.0, 60.0)), None);
        assert_eq!(band_powers(&[]), None);
    }

    #[test]
    fn a_perfectly_regular_rhythm_has_no_spectrum_to_report() {
        let steady: Vec<(i64, f64)> = (0..400)
            .map(|beat| (beat as i64 * 1_000, 1_000.0))
            .collect();
        assert_eq!(band_powers(&steady), None);
    }
}
