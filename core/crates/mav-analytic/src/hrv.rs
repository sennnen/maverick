//! Time-domain variability over scored beat-to-beat intervals.
//!
//! The formulas follow the 1996 ESC/NASPE Task Force definitions. WHOOP derives its RR stream from
//! optical pulse timing rather than ECG R peaks, so callers must label that source as PPG and the
//! result serialises as pulse-rate variability, not diagnostic ECG HRV.

use mav_model::ids::MetadataId;
use mav_model::raw::RawValue;
use mav_model::stream::{Sample, StreamKind};
use mav_model::version::Version;
use serde::{Deserialize, Serialize};

pub const HRV_ALGORITHM: &str = "time_domain_interval_variability";
pub const HRV_VERSION: Version = Version::new(1, 0, 0);
pub const QUALITY_FLOOR: f32 = 0.5;
pub const MIN_RR_MS: f64 = 300.0;
pub const MAX_RR_MS: f64 = 2_000.0;
pub const MIN_INTERVALS: usize = 3;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntervalSource {
    Ecg,
    Ppg,
    Unknown,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct TimeDomainHrv {
    pub source: IntervalSource,
    pub label: String,
    pub mean_interval_ms: f64,
    pub rmssd_ms: f64,
    pub sdnn_ms: f64,
    pub nn50_count: u32,
    pub pnn50_percent: f64,
    pub interval_count: u32,
    pub excluded_count: u32,
    pub provenance: MetadataId,
}

/// Compute standard time-domain measures over trustworthy RR intervals.
///
/// Samples are sorted by `(device_time, seq)` before adjacent differences are taken. Poor-quality,
/// wrong-kind, and physiologically implausible intervals are excluded and counted; none are
/// corrected or interpolated. Fewer than three accepted intervals returns `None`, because two
/// intervals provide only one successive difference and no useful variance estimate.
pub fn time_domain(
    samples: &[Sample<RawValue>],
    source: IntervalSource,
    provenance: MetadataId,
) -> Option<TimeDomainHrv> {
    let mut accepted = Vec::with_capacity(samples.len());
    let mut excluded = 0u32;

    for sample in samples {
        let rr_ms = sample.value.as_f64();
        if sample.kind != StreamKind::RrInterval
            || sample.quality.score < QUALITY_FLOOR
            || !(MIN_RR_MS..=MAX_RR_MS).contains(&rr_ms)
        {
            excluded += 1;
            continue;
        }
        accepted.push((sample.device_time.as_nanos(), sample.seq, rr_ms));
    }

    if accepted.len() < MIN_INTERVALS {
        return None;
    }

    accepted.sort_by_key(|(device_ns, seq, _)| (*device_ns, *seq));
    let intervals: Vec<f64> = accepted.into_iter().map(|(_, _, rr_ms)| rr_ms).collect();
    let count = intervals.len();
    let mean = intervals.iter().sum::<f64>() / count as f64;

    let sum_squared_deviation = intervals
        .iter()
        .map(|rr| {
            let delta = rr - mean;
            delta * delta
        })
        .sum::<f64>();
    let sdnn = (sum_squared_deviation / (count - 1) as f64).sqrt();

    let differences: Vec<f64> = intervals.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let rmssd = (differences.iter().map(|delta| delta * delta).sum::<f64>()
        / differences.len() as f64)
        .sqrt();
    let nn50 = differences
        .iter()
        .filter(|delta| delta.abs() > 50.0)
        .count();
    let pnn50 = nn50 as f64 * 100.0 / differences.len() as f64;

    Some(TimeDomainHrv {
        source,
        label: match source {
            IntervalSource::Ecg => "heart_rate_variability",
            IntervalSource::Ppg => "pulse_rate_variability",
            IntervalSource::Unknown => "interval_variability",
        }
        .to_owned(),
        mean_interval_ms: mean,
        rmssd_ms: rmssd,
        sdnn_ms: sdnn,
        nn50_count: nn50 as u32,
        pnn50_percent: pnn50,
        interval_count: count as u32,
        excluded_count: excluded,
        provenance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_model::stream::Quality;
    use mav_model::time::DeviceTime;

    fn rr(device_ns: i64, seq: u16, rr_ms: u16, quality: Quality) -> Sample<RawValue> {
        Sample {
            kind: StreamKind::RrInterval,
            device_time: DeviceTime::from_nanos(device_ns),
            wall_time: None,
            seq,
            value: RawValue::U16(rr_ms),
            quality,
            provenance: MetadataId::new(1),
        }
    }

    fn exact_rr(device_ns: i64, seq: u16, rr_ms: u16) -> Sample<RawValue> {
        rr(device_ns, seq, rr_ms, Quality::exact())
    }

    fn close(left: f64, right: f64) {
        assert!((left - right).abs() < 1e-9, "{left} != {right}");
    }

    #[test]
    fn published_formulas_match_hand_calculated_vector() {
        // Intervals: 800, 850, 790, 900 ms.
        // Differences: +50, -60, +110; squared sum = 18,200.
        // Squared deviations around mean 835 ms sum to 7,700.
        let samples = vec![
            exact_rr(1, 0, 800),
            exact_rr(2, 0, 850),
            exact_rr(3, 0, 790),
            exact_rr(4, 0, 900),
        ];
        let result = time_domain(&samples, IntervalSource::Ecg, MetadataId::new(7)).unwrap();
        close(result.mean_interval_ms, 835.0);
        close(result.rmssd_ms, (18_200.0f64 / 3.0).sqrt());
        close(result.sdnn_ms, (7_700.0f64 / 3.0).sqrt());
        assert_eq!(result.nn50_count, 2);
        close(result.pnn50_percent, 200.0 / 3.0);
        assert_eq!(result.interval_count, 4);
    }

    #[test]
    fn equal_rr_intervals_same_second_remain_distinct() {
        let samples = vec![
            exact_rr(1_000_000_000, 0, 800),
            exact_rr(1_000_000_000, 1, 800),
            exact_rr(2_000_000_000, 0, 900),
        ];
        let result = time_domain(&samples, IntervalSource::Ppg, MetadataId::new(1)).unwrap();
        close(result.rmssd_ms, (5_000.0f64).sqrt());
        assert_eq!(result.interval_count, 3);
    }

    #[test]
    fn input_order_does_not_change_result() {
        let ordered = vec![
            exact_rr(1, 0, 800),
            exact_rr(2, 0, 820),
            exact_rr(3, 0, 780),
        ];
        let shuffled = vec![ordered[2], ordered[0], ordered[1]];
        assert_eq!(
            time_domain(&ordered, IntervalSource::Ppg, MetadataId::new(1)),
            time_domain(&shuffled, IntervalSource::Ppg, MetadataId::new(1))
        );
    }

    #[test]
    fn poor_quality_and_implausible_values_are_excluded_not_corrected() {
        let samples = vec![
            exact_rr(1, 0, 800),
            rr(2, 0, 50, Quality::exact()),
            rr(3, 0, 900, Quality::scored(0.2)),
            exact_rr(4, 0, 810),
            exact_rr(5, 0, 820),
        ];
        let result = time_domain(&samples, IntervalSource::Ppg, MetadataId::new(1)).unwrap();
        assert_eq!(result.interval_count, 3);
        assert_eq!(result.excluded_count, 2);
        close(result.mean_interval_ms, 810.0);
    }

    #[test]
    fn too_few_intervals_is_unavailable() {
        assert_eq!(
            time_domain(
                &[exact_rr(1, 0, 800), exact_rr(2, 0, 810)],
                IntervalSource::Ppg,
                MetadataId::new(1)
            ),
            None
        );
    }

    #[test]
    fn ppg_is_labelled_prv_not_ecg_hrv() {
        let samples = vec![
            exact_rr(1, 0, 800),
            exact_rr(2, 0, 810),
            exact_rr(3, 0, 820),
        ];
        let result = time_domain(&samples, IntervalSource::Ppg, MetadataId::new(1)).unwrap();
        assert_eq!(result.label, "pulse_rate_variability");
        assert_eq!(result.source, IntervalSource::Ppg);
    }
}
