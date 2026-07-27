//! Time-domain variability over scored beat-to-beat intervals.
//!
//! The formulas follow the 1996 ESC/NASPE Task Force definitions, and the Poincaré descriptors the
//! exact identities Brennan et al. (2001) derived from them. Which physiological event timed the
//! beats decides what the result may be called, and that is carried by the stream kind rather than
//! asserted by the caller: only an electrical R peak produces heart-rate variability, and an
//! optical pulse produces pulse-rate variability.

use crate::intervals::BeatSeries;
use mav_model::ids::MetadataId;
use mav_model::raw::RawValue;
use mav_model::stream::{Sample, StreamKind};
use mav_model::version::Version;
use serde::{Deserialize, Serialize};

pub const HRV_ALGORITHM: &str = "time_domain_interval_variability";
pub const HRV_VERSION: Version = Version::new(2, 0, 0);
pub const QUALITY_FLOOR: f32 = 0.5;
pub const MIN_INTERVAL_MS: f64 = 300.0;
pub const MAX_INTERVAL_MS: f64 = 2_000.0;
/// Two intervals give one difference and no variance estimate, so three is the floor. It is enough
/// to exercise the formulas and not enough to claim a clinically meaningful window; window-length
/// policy belongs to the metric that uses this, never to the primitive.
pub const MIN_INTERVALS: usize = 3;

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct TimeDomainHrv {
    /// Which stream timed the beats — the fact that decides `label`.
    pub source: StreamKind,
    pub label: String,
    pub mean_interval_ms: f64,
    pub rmssd_ms: f64,
    pub sdnn_ms: f64,
    /// Poincaré short-term scatter; the beat-to-beat axis, parasympathetically driven.
    pub sd1_ms: f64,
    /// Poincaré long-term scatter; the axis along the identity line.
    pub sd2_ms: f64,
    pub nn50_count: u32,
    pub pnn50_percent: f64,
    /// Short-term detrended fluctuation exponent. Absent when no uninterrupted run is long enough
    /// for the scaling to be measurable.
    pub alpha1: Option<f64>,
    pub interval_count: u32,
    pub excluded_count: u32,
    pub provenance: MetadataId,
}

/// What a variability result computed over `kind` may honestly be called.
pub fn label_for(kind: StreamKind) -> &'static str {
    match kind {
        StreamKind::RrInterval => "heart_rate_variability",
        StreamKind::PulseInterval => "pulse_rate_variability",
        _ => "interval_variability",
    }
}

/// Compute the standard time-domain measures over trustworthy intervals of one kind.
///
/// Poor-quality, wrong-kind and physiologically implausible samples are excluded and counted, then
/// the series is split into runs of genuinely successive beats and filtered; none of it is
/// corrected or interpolated. See [`crate::intervals`] for why both of those are one job.
pub fn time_domain(
    samples: &[Sample<RawValue>],
    kind: StreamKind,
    provenance: MetadataId,
) -> Option<TimeDomainHrv> {
    let (ordered, mut excluded) = crate::intervals::ordered_intervals(samples, |sample| {
        sample.kind == kind
            && sample.quality.score >= QUALITY_FLOOR
            && (MIN_INTERVAL_MS..=MAX_INTERVAL_MS).contains(&sample.value.as_f64())
    });
    let series = BeatSeries::from_ordered(&ordered);
    excluded += series.excluded();
    if series.len() < MIN_INTERVALS {
        return None;
    }

    let (nn50_count, pnn50_percent) = series.nn50()?;
    let (sd1_ms, sd2_ms) = series.poincare_ms()?;
    Some(TimeDomainHrv {
        source: kind,
        label: label_for(kind).to_owned(),
        mean_interval_ms: series.mean_interval_ms()?,
        rmssd_ms: series.rmssd_ms()?,
        sdnn_ms: series.sdnn_ms()?,
        sd1_ms,
        sd2_ms,
        nn50_count,
        pnn50_percent,
        alpha1: series.alpha1(),
        interval_count: series.len() as u32,
        excluded_count: excluded,
        provenance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_model::stream::{Placement, Quality};
    use mav_model::time::DeviceTime;

    fn interval(
        kind: StreamKind,
        device_ns: i64,
        seq: u16,
        ms: u16,
        quality: Quality,
    ) -> Sample<RawValue> {
        Sample {
            kind,
            device_time: DeviceTime::from_nanos(device_ns),
            placement: Placement::Unplaced,
            seq,
            value: RawValue::U16(ms),
            quality,
            provenance: MetadataId::new(1),
        }
    }

    fn pulse(device_ns: i64, seq: u16, ms: u16) -> Sample<RawValue> {
        interval(
            StreamKind::PulseInterval,
            device_ns,
            seq,
            ms,
            Quality::exact(),
        )
    }

    fn close(left: f64, right: f64) {
        assert!((left - right).abs() < 1e-9, "{left} != {right}");
    }

    fn compute(samples: &[Sample<RawValue>], kind: StreamKind) -> Option<TimeDomainHrv> {
        time_domain(samples, kind, MetadataId::new(7))
    }

    #[test]
    fn published_formulas_match_hand_calculated_vector() {
        let samples: Vec<_> = [800u16, 850, 790, 900]
            .iter()
            .enumerate()
            .map(|(i, ms)| pulse(i as i64 * 1_000_000_000, 0, *ms))
            .collect();
        let result = compute(&samples, StreamKind::PulseInterval).unwrap();
        close(result.mean_interval_ms, 835.0);
        close(result.rmssd_ms, (18_200.0f64 / 3.0).sqrt());
        close(result.sdnn_ms, (7_700.0f64 / 3.0).sqrt());
        assert_eq!(result.nn50_count, 2);
        assert_eq!(result.interval_count, 4);
    }

    #[test]
    fn equal_intervals_in_one_second_remain_distinct() {
        let samples = vec![
            pulse(1_000_000_000, 0, 800),
            pulse(1_000_000_000, 1, 800),
            pulse(2_000_000_000, 0, 900),
        ];
        let result = compute(&samples, StreamKind::PulseInterval).unwrap();
        close(result.rmssd_ms, 5_000.0f64.sqrt());
        assert_eq!(result.interval_count, 3);
    }

    #[test]
    fn input_order_does_not_change_result() {
        let ordered = vec![
            pulse(1_000_000_000, 0, 800),
            pulse(2_000_000_000, 0, 820),
            pulse(3_000_000_000, 0, 780),
        ];
        let shuffled = vec![ordered[2], ordered[0], ordered[1]];
        assert_eq!(
            compute(&ordered, StreamKind::PulseInterval),
            compute(&shuffled, StreamKind::PulseInterval)
        );
    }

    #[test]
    fn poor_quality_and_implausible_values_are_excluded_not_corrected() {
        let samples = vec![
            pulse(1_000_000_000, 0, 800),
            interval(
                StreamKind::PulseInterval,
                2_000_000_000,
                0,
                50,
                Quality::exact(),
            ),
            interval(
                StreamKind::PulseInterval,
                3_000_000_000,
                0,
                900,
                Quality::scored(0.2),
            ),
            pulse(4_000_000_000, 0, 810),
            pulse(5_000_000_000, 0, 820),
        ];
        let result = compute(&samples, StreamKind::PulseInterval).unwrap();
        assert_eq!(result.interval_count, 3);
        assert_eq!(result.excluded_count, 2);
        close(result.mean_interval_ms, 810.0);
    }

    #[test]
    fn too_few_intervals_is_unavailable() {
        let samples = vec![pulse(1_000_000_000, 0, 800), pulse(2_000_000_000, 0, 810)];
        assert_eq!(compute(&samples, StreamKind::PulseInterval), None);
    }

    /// A doubled beat sits inside the plausible band and would still wreck RMSSD, because one
    /// enormous successive difference dominates the root-mean-square. Found against a live optical
    /// capture that reported around 600 ms where the truth was tens.
    #[test]
    fn a_single_ectopic_beat_does_not_dominate_rmssd() {
        let mut beats: Vec<u16> = (0..20)
            .map(|i| if i % 2 == 0 { 900 } else { 950 })
            .collect();
        beats[10] = 1_800;
        let samples: Vec<_> = beats
            .iter()
            .enumerate()
            .map(|(i, ms)| pulse(i as i64 * 1_000_000_000, 0, *ms))
            .collect();
        let result = compute(&samples, StreamKind::PulseInterval).unwrap();
        close(result.rmssd_ms, 50.0);
        assert_eq!(result.excluded_count, 1);
    }

    /// The distinction the whole project turns on: only an electrical R peak may be called HRV,
    /// and the stream kind is what decides it — no caller can assert otherwise.
    #[test]
    fn the_stream_kind_decides_hrv_against_prv() {
        let optical: Vec<_> = [800u16, 810, 820]
            .iter()
            .enumerate()
            .map(|(i, ms)| pulse(i as i64 * 1_000_000_000, 0, *ms))
            .collect();
        let electrical: Vec<_> = optical
            .iter()
            .map(|sample| Sample {
                kind: StreamKind::RrInterval,
                ..*sample
            })
            .collect();

        let prv = compute(&optical, StreamKind::PulseInterval).unwrap();
        assert_eq!(prv.label, "pulse_rate_variability");
        assert_eq!(prv.source, StreamKind::PulseInterval);

        let hrv = compute(&electrical, StreamKind::RrInterval).unwrap();
        assert_eq!(hrv.label, "heart_rate_variability");
        assert_eq!(hrv.source, StreamKind::RrInterval);

        close(prv.rmssd_ms, hrv.rmssd_ms);
    }

    #[test]
    fn samples_of_another_kind_are_excluded_not_mixed_in() {
        let mut samples: Vec<_> = [800u16, 810, 820, 830]
            .iter()
            .enumerate()
            .map(|(i, ms)| pulse(i as i64 * 1_000_000_000, 0, *ms))
            .collect();
        samples.push(interval(
            StreamKind::RrInterval,
            5_000_000_000,
            0,
            1_400,
            Quality::exact(),
        ));
        let result = compute(&samples, StreamKind::PulseInterval).unwrap();
        assert_eq!(result.interval_count, 4);
        assert_eq!(result.excluded_count, 1);
    }
}
