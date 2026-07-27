//! The minimal heart-rate features the Milestone 1 snapshot needs: the current heart rate and a
//! session summary. Both are computed over the in-range samples only, but nothing is deleted; a
//! downscored sample is excluded from the summary and counted, so the summary can say how much it
//! set aside and why. This lands under the admission rule because it has a golden fixture derived
//! from the capture, which is the bar docs/testing.md sets.

use mav_model::ids::MetadataId;
use mav_model::raw::RawValue;
use mav_model::stream::Sample;
use mav_model::version::Version;
use serde::{Deserialize, Serialize};

pub const HR_FEATURE_ALGORITHM: &str = "hr_summary";
pub const HR_FEATURE_VERSION: Version = Version::new(1, 0, 0);

/// A sample counts toward the summary when its quality score is at least this. The SQI stage
/// scores an in-range heart rate 1.0 and a rejected one 0.0, so this cleanly separates the two
/// while leaving room for a future graded score.
pub const QUALITY_FLOOR: f32 = 0.5;

/// The heart-rate snapshot feature: the latest trustworthy reading and a summary of the window.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct HrSummary {
    /// The most recent in-range heart rate, or `None` when the window held no in-range sample.
    pub current_bpm: Option<u16>,
    /// The mean of the in-range heart rates, or `None` when there were none.
    pub mean_bpm: Option<f64>,
    /// How many in-range samples went into the summary.
    pub sample_count: u32,
    /// How many samples were set aside for a poor quality score.
    pub excluded_count: u32,
    /// The provenance row this feature points at, assigned by the engine.
    pub provenance: MetadataId,
}

/// Compute the heart-rate summary over a slice of scored HR samples. The slice need not be ordered;
/// "current" is the in-range sample with the greatest device time.
pub fn hr_summary(samples: &[Sample<RawValue>], provenance: MetadataId) -> HrSummary {
    let mut current: Option<(i64, f64)> = None;
    let mut sum = 0.0f64;
    let mut in_range = 0u32;
    let mut excluded = 0u32;

    for sample in samples {
        if sample.quality.score < QUALITY_FLOOR {
            excluded += 1;
            continue;
        }
        let bpm = sample.value.as_f64();
        sum += bpm;
        in_range += 1;
        let device_ns = sample.device_time.as_nanos();
        if current.is_none_or(|(latest, _)| device_ns >= latest) {
            current = Some((device_ns, bpm));
        }
    }

    HrSummary {
        current_bpm: current.map(|(_, bpm)| bpm.round() as u16),
        mean_bpm: if in_range > 0 {
            Some(sum / f64::from(in_range))
        } else {
            None
        },
        sample_count: in_range,
        excluded_count: excluded,
        provenance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_model::stream::{Placement, Quality, RejectReason, StreamKind};
    use mav_model::time::DeviceTime;

    fn hr(device_ns: i64, bpm: u16, score: f32) -> Sample<RawValue> {
        Sample {
            kind: StreamKind::HeartRate,
            device_time: DeviceTime::from_nanos(device_ns),
            placement: Placement::Unplaced,
            seq: 0,
            value: RawValue::U16(bpm),
            quality: Quality {
                score,
                reason: (score < QUALITY_FLOOR).then_some(RejectReason::ImplausibleValue),
            },
            provenance: MetadataId::new(0),
        }
    }

    fn meta() -> MetadataId {
        MetadataId::new(5)
    }

    #[test]
    fn current_hr_is_latest_in_range() {
        // The series ends with an out-of-range spike; current must be the latest in-range value.
        let samples = vec![hr(1_000, 58, 1.0), hr(2_000, 61, 1.0), hr(3_000, 300, 0.0)];
        let summary = hr_summary(&samples, meta());
        assert_eq!(summary.current_bpm, Some(61));
    }

    #[test]
    fn session_summary_counts_in_range_only() {
        let samples = vec![hr(1_000, 60, 1.0), hr(2_000, 62, 1.0), hr(3_000, 500, 0.0)];
        let summary = hr_summary(&samples, meta());
        assert_eq!(summary.sample_count, 2);
        assert_eq!(summary.excluded_count, 1);
        assert_eq!(summary.mean_bpm, Some(61.0));
    }

    #[test]
    fn feature_carries_provenance_and_version() {
        let summary = hr_summary(&[hr(1_000, 60, 1.0)], MetadataId::new(42));
        assert_eq!(summary.provenance, MetadataId::new(42));
        assert_eq!(HR_FEATURE_VERSION, Version::new(1, 0, 0));
        assert_eq!(HR_FEATURE_ALGORITHM, "hr_summary");
    }

    #[test]
    fn all_out_of_range_yields_no_current_and_no_mean() {
        let summary = hr_summary(&[hr(1_000, 300, 0.0), hr(2_000, 10, 0.0)], meta());
        assert_eq!(summary.current_bpm, None);
        assert_eq!(summary.mean_bpm, None);
        assert_eq!(summary.sample_count, 0);
        assert_eq!(summary.excluded_count, 2);
    }

    #[test]
    fn current_is_by_device_time_not_slice_order() {
        // Out of order: the latest device time is in the middle of the slice.
        let samples = vec![hr(1_000, 55, 1.0), hr(9_000, 70, 1.0), hr(3_000, 60, 1.0)];
        let summary = hr_summary(&samples, meta());
        assert_eq!(summary.current_bpm, Some(70));
    }

    #[test]
    fn empty_input_is_all_none() {
        let summary = hr_summary(&[], meta());
        assert_eq!(summary.current_bpm, None);
        assert_eq!(summary.sample_count, 0);
        assert_eq!(summary.excluded_count, 0);
    }
}
