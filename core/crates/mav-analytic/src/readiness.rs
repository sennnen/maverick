//! HRV readiness (WHOOP-P6, `[WRS]`): a log-domain rolling-baseline vs personal-normal-band
//! reading over a nightly RMSSD series. Distinct from the Task-Force `time_domain` calculation in
//! `hrv`: that one is the reference formula over one scored window; this one is the longitudinal
//! readout — a 7-night baseline against a 30/60-night normal band (smallest-worthwhile-change,
//! k = 0.5) and a tier. The nightly RMSSD values it consumes come from [`crate::intervals`], the
//! one place successive differences are taken. Wellness estimate, never medical.
use serde::{Deserialize, Serialize};

use crate::stats::{least_squares_slope, mean, sample_sd};

const HRV_MIN_MS: f64 = 5.0;
const HRV_MAX_MS: f64 = 250.0;
const ROLL_WINDOW: usize = 7;
const LONG_WINDOW: usize = 60;
const LONG_WINDOW_FALLBACK: usize = 30;
const SWC_K: f64 = 0.5;
/// The fewest nights for which every number in the result means what it says: the baseline is
/// described as a seven-night one, so seven is the floor. This is a statistical minimum and
/// deliberately not a product unlock schedule — `crate::calibration` holds those, and reading one
/// as the other is how a display rule ends up deciding whether an estimator is defined.
const MIN_NIGHTS: usize = ROLL_WINDOW;
const CV_TREND_WINDOW: usize = 28;
const LONG_SD_FLOOR: f64 = 1e-9;

/// Where the short baseline sits vs the personal normal band.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessTier {
    Primed,
    Normal,
    Suppressed,
}

/// A readiness reading. The `*_ms` fields are back in milliseconds (exp of the log-domain the
/// engine works in). `overreaching_watch` is informational and never changes the tier.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HrvReadinessResult {
    pub tier: ReadinessTier,
    pub baseline7_ms: f64,
    pub normal_low_ms: f64,
    pub normal_high_ms: f64,
    pub overreaching_watch: bool,
}

pub struct HrvReadiness;

impl HrvReadiness {
    /// Readiness over a nightly RMSSD series (ms), oldest → newest; `None` slots = missing
    /// nights. Implausible nights (outside 5..250 ms) are dropped. `None` result = calibrating.
    pub fn evaluate(nightly_rmssd_ms: &[Option<f64>]) -> Option<HrvReadinessResult> {
        let valid: Vec<f64> = nightly_rmssd_ms
            .iter()
            .filter_map(|&v| v)
            .filter(|&v| (HRV_MIN_MS..=HRV_MAX_MS).contains(&v))
            .collect();
        if valid.len() < MIN_NIGHTS {
            return None;
        }

        let ell: Vec<f64> = valid.iter().map(|&v| v.max(1.0).ln()).collect();
        let baseline7 = mean(tail(&ell, ROLL_WINDOW));

        let long_win = if valid.len() >= LONG_WINDOW {
            LONG_WINDOW
        } else {
            LONG_WINDOW_FALLBACK
        };
        let long_ell = tail(&ell, long_win);
        let long_mean = mean(long_ell);
        let long_sd_raw = if long_ell.len() >= 2 {
            sample_sd(long_ell)
        } else {
            sample_sd(tail(&ell, ROLL_WINDOW))
        };
        let long_sd = long_sd_raw.max(LONG_SD_FLOOR);

        let swc_half = SWC_K * long_sd;
        let normal_low = long_mean - swc_half;
        let normal_high = long_mean + swc_half;

        let tier = if baseline7 > normal_high {
            ReadinessTier::Primed
        } else if baseline7 >= normal_low {
            ReadinessTier::Normal
        } else {
            ReadinessTier::Suppressed
        };

        let overreaching_watch = cv_slope(&ell) < 0.0 && baseline7 < long_mean;

        Some(HrvReadinessResult {
            tier,
            baseline7_ms: baseline7.exp(),
            normal_low_ms: normal_low.exp(),
            normal_high_ms: normal_high.exp(),
            overreaching_watch,
        })
    }
}

/// The last `n` elements, or all if fewer.
fn tail(xs: &[f64], n: usize) -> &[f64] {
    &xs[xs.len().saturating_sub(n)..]
}

/// OLS slope of the rolling 7-night coefficient-of-variation series over the trailing
/// `CV_TREND_WINDOW`.
fn cv_slope(ell: &[f64]) -> f64 {
    let start = (ROLL_WINDOW - 1).max(ell.len().saturating_sub(CV_TREND_WINDOW));
    let mut cv = Vec::new();
    for i in start..ell.len() {
        let w = &ell[i + 1 - ROLL_WINDOW..=i];
        let m = mean(w);
        cv.push(if m != 0.0 {
            100.0 * sample_sd(w) / m
        } else {
            0.0
        });
    }
    least_squares_slope(&cv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluate_calibrating_below_min_nights() {
        let nights = vec![Some(50.0); MIN_NIGHTS - 1];
        assert!(HrvReadiness::evaluate(&nights).is_none());
    }

    #[test]
    fn flat_history_reads_normal() {
        let nights = vec![Some(50.0); 20];
        assert_eq!(
            HrvReadiness::evaluate(&nights).unwrap().tier,
            ReadinessTier::Normal
        );
    }

    #[test]
    fn rising_last_week_is_primed_falling_is_suppressed() {
        let mut rising: Vec<Option<f64>> = vec![Some(40.0); 13];
        rising.extend(vec![Some(80.0); 7]);
        assert_eq!(
            HrvReadiness::evaluate(&rising).unwrap().tier,
            ReadinessTier::Primed
        );

        let mut falling: Vec<Option<f64>> = vec![Some(80.0); 13];
        falling.extend(vec![Some(40.0); 7]);
        assert_eq!(
            HrvReadiness::evaluate(&falling).unwrap().tier,
            ReadinessTier::Suppressed
        );
    }

    #[test]
    fn implausible_and_missing_nights_are_dropped() {
        // 2 valid + a null + an out-of-range 400 → still only 2 valid → below MIN_NIGHTS.
        let mut nights = vec![Some(50.0); 2];
        nights.push(None);
        nights.push(Some(400.0));
        assert!(HrvReadiness::evaluate(&nights).is_none());
    }
}
