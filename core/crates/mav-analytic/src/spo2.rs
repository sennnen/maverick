//! Pulse oximetry from a dual-wavelength photoplethysmogram (WHOOP-P6, `[WRS]`).
//!
//! The measurement is the ratio of ratios, `R = (AC_red/DC_red) / (AC_ir/DC_ir)`, which is what the
//! optics actually give you. Turning `R` into a saturation percentage needs an empirical curve
//! fitted against a reference oximeter on calibrated channels; ours are raw converter counts from a
//! strap nobody has co-oximetered, so the percentage here is explicitly `uncalibrated_percent` and
//! the ratio is the number to trust. Wellness estimate, never clinical.
//!
//! Two things this module now refuses to do, both of which it used to.
//!
//! It will not read a channel sampled too slowly to carry a pulse. The pulsatile component sits
//! around 1–3 Hz, so recovering it needs several times that; the 4.0's paired red/IR arrive once a
//! second, and at 1 Hz the "AC amplitude" of a heartbeat is an aliasing artefact rather than a
//! measurement. The 5.0/MG raw pulse-ox triad at 25 Hz is fast enough, which is what makes this
//! path worth having at all.
//!
//! And it will not anchor a multi-night median to a plausible-looking number. The previous rolling
//! readout shifted the 30-night median onto 96.5% by construction, so it reported a healthy
//! saturation whatever the sensor said. What survives a missing calibration is *change*: tonight
//! against the wearer's own baseline, reported as a difference and labelled as one.

use crate::stats::{amplitude, mean, median};

/// Below this, a heartbeat is not resolvable and any "AC" component is an aliasing artefact.
/// Three times the fastest plausible pulse, which is the usual engineering margin over Nyquist.
pub const MIN_SAMPLE_RATE_HZ: f64 = 10.0;
/// Analysis window, in seconds. Long enough for several beats at any plausible rate.
const WINDOW_SECONDS: f64 = 30.0;
/// A window needs at least this many beats' worth of samples to be worth measuring.
const MIN_SAMPLES_PER_WINDOW: usize = 64;
/// The conventional empirical curve, `SpO2 ≈ A − B·R`. Uncalibrated on raw counts.
const CURVE_A: f64 = 110.0;
const CURVE_B: f64 = 25.0;
const CLAMP_LOW: f64 = 70.0;
const CLAMP_HIGH: f64 = 100.0;

/// How many trailing nights a personal baseline is taken over.
pub const BASELINE_NIGHTS: usize = 30;
/// The fewest nights that make a median a baseline rather than an anecdote.
pub const MIN_BASELINE_NIGHTS: usize = 7;

/// One night's oximetry. The ratio is the measurement; the percentage is what the conventional
/// curve makes of it on channels nobody has calibrated, and is named accordingly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spo2Reading {
    pub ratio_of_ratios: f64,
    pub uncalibrated_percent: f64,
}

pub struct Spo2;

impl Spo2 {
    /// A night's reading from parallel red and infrared channels sampled at `sample_rate_hz`.
    ///
    /// `None` when the channels are too short, too slow, or carry no pulsatile component — all of
    /// which are honest answers, and none of which is a saturation.
    pub fn from_paired(red: &[f64], ir: &[f64], sample_rate_hz: f64) -> Option<Spo2Reading> {
        if sample_rate_hz < MIN_SAMPLE_RATE_HZ {
            return None;
        }
        let window = ((WINDOW_SECONDS * sample_rate_hz) as usize).max(MIN_SAMPLES_PER_WINDOW);
        let paired = red.len().min(ir.len());
        let ratios: Vec<f64> = (0..paired)
            .step_by(window)
            .filter(|start| paired - start >= MIN_SAMPLES_PER_WINDOW)
            .filter_map(|start| {
                let end = (start + window).min(paired);
                window_ratio(&red[start..end], &ir[start..end])
            })
            .collect();
        if ratios.is_empty() {
            return None;
        }
        let ratio_of_ratios = median(&ratios);
        Some(Spo2Reading {
            ratio_of_ratios,
            uncalibrated_percent: (CURVE_A - CURVE_B * ratio_of_ratios)
                .clamp(CLAMP_LOW, CLAMP_HIGH),
        })
    }

    /// Tonight against the wearer's own trailing baseline, in percentage points.
    ///
    /// This is what an uncalibrated sensor can honestly report: a constant offset cancels in a
    /// difference, so a drop of two points is a drop of two points even when the absolute level is
    /// unknown. `None` until there are enough nights for a baseline to mean anything.
    pub fn baseline_delta(nightly_percent: &[f64]) -> Option<f64> {
        let window = &nightly_percent[nightly_percent.len().saturating_sub(BASELINE_NIGHTS)..];
        if window.len() < MIN_BASELINE_NIGHTS {
            return None;
        }
        Some(window.last()? - median(window))
    }
}

/// One window's ratio of ratios; `None` unless both channels have a positive baseline and a
/// positive pulsatile component.
fn window_ratio(red: &[f64], ir: &[f64]) -> Option<f64> {
    let (dc_red, dc_ir) = (mean(red), mean(ir));
    let (ac_red, ac_ir) = (amplitude(red), amplitude(ir));
    let perfusion_ir = ac_ir / dc_ir;
    (dc_red > 0.0 && dc_ir > 0.0 && ac_red > 0.0 && ac_ir > 0.0 && perfusion_ir > 0.0)
        .then(|| (ac_red / dc_red) / perfusion_ir)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE_HZ: f64 = 25.0;

    /// A channel with baseline `dc` and a p95−p5 pulsatile amplitude of `ac`.
    fn channel(dc: f64, ac: f64, samples: usize) -> Vec<f64> {
        (0..samples)
            .map(|index| {
                if index % 2 == 0 {
                    dc - ac / 2.0
                } else {
                    dc + ac / 2.0
                }
            })
            .collect()
    }

    #[test]
    fn identical_channels_give_a_ratio_of_one() {
        let both = channel(100.0, 4.0, 256);
        let reading = Spo2::from_paired(&both, &both, RATE_HZ).expect("a pulsatile pair reads");
        assert!((reading.ratio_of_ratios - 1.0).abs() < 1e-9);
        assert!((reading.uncalibrated_percent - 85.0).abs() < 1e-9);
    }

    #[test]
    fn less_red_absorption_reads_as_a_higher_saturation() {
        let red = channel(100.0, 2.0, 256);
        let infrared = channel(100.0, 4.0, 256);
        let reading = Spo2::from_paired(&red, &infrared, RATE_HZ).expect("reads");
        assert!((reading.ratio_of_ratios - 0.5).abs() < 1e-9);
        assert!((reading.uncalibrated_percent - 97.5).abs() < 1e-9);
    }

    /// The Nyquist refusal. A heartbeat at 1 Hz sampling is not a heartbeat, it is aliasing, and a
    /// number derived from it is not a measurement however plausible it looks.
    #[test]
    fn a_channel_sampled_too_slowly_for_a_pulse_reads_nothing() {
        let both = channel(100.0, 4.0, 256);
        assert_eq!(Spo2::from_paired(&both, &both, 1.0), None);
        assert_eq!(
            Spo2::from_paired(&both, &both, MIN_SAMPLE_RATE_HZ - 0.1),
            None
        );
        assert!(Spo2::from_paired(&both, &both, MIN_SAMPLE_RATE_HZ).is_some());
    }

    #[test]
    fn a_flat_or_absent_signal_reads_nothing() {
        assert_eq!(Spo2::from_paired(&[], &[], RATE_HZ), None);
        let flat = vec![100.0; 256];
        assert_eq!(Spo2::from_paired(&flat, &flat, RATE_HZ), None);
        let short = channel(100.0, 4.0, 8);
        assert_eq!(Spo2::from_paired(&short, &short, RATE_HZ), None);
    }

    /// The finding this module was rewritten for. A constant calibration error must not survive
    /// into the reported change, and a flat history must read as no change rather than as a
    /// healthy-looking absolute number.
    #[test]
    fn the_baseline_reading_reports_change_and_not_a_manufactured_level() {
        let flat = vec![90.0; 30];
        assert_eq!(Spo2::baseline_delta(&flat), Some(0.0));

        let mut dipped = vec![95.0; 29];
        dipped.push(92.0);
        assert_eq!(Spo2::baseline_delta(&dipped), Some(-3.0));

        // The same series shifted by a constant offset reports the same change.
        let shifted: Vec<f64> = dipped.iter().map(|value| value - 7.0).collect();
        assert_eq!(
            Spo2::baseline_delta(&shifted),
            Spo2::baseline_delta(&dipped)
        );
    }

    #[test]
    fn too_few_nights_is_no_baseline_rather_than_a_guess() {
        assert_eq!(Spo2::baseline_delta(&[]), None);
        assert_eq!(Spo2::baseline_delta(&[96.0; MIN_BASELINE_NIGHTS - 1]), None);
        assert!(Spo2::baseline_delta(&[96.0; MIN_BASELINE_NIGHTS]).is_some());
    }
}
