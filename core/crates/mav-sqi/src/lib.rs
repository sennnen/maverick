//! Signal quality: raw samples in, scored samples out, nothing dropped. The M1 scoring is
//! deliberately minimal and plausibility-based; motion-artifact and perfusion scoring arrive with
//! the raw optical streams in later milestones. The plausibility bounds live here and not in a
//! manifest because they are analytic judgement about physiology, not facts about a wire format.
//!
//! A poor sample is scored and kept. Deleting it here would hide what the sensor said from the
//! inspector, and the no-silent-drops rule (docs/pipeline.md) applies to quality judgements too:
//! downstream stages decide what a low score is worth for their metric, with the reason attached.
#![forbid(unsafe_code)]

use mav_model::ids::MetadataId;
use mav_model::raw::{RawSample, RawSampleBatch, RawValue};
use mav_model::stream::{Quality, RejectReason, Sample, StreamKind};
use std::ops::RangeInclusive;

/// Plausible human heart rate, in whole bpm. Values outside are downscored, not deleted.
pub const HR_PLAUSIBLE_BPM: RangeInclusive<f64> = 30.0..=220.0;
/// Plausible RR interval, in milliseconds; matches the range both surveyed codebases gate on
/// before HRV work.
pub const RR_PLAUSIBLE_MS: RangeInclusive<f64> = 300.0..=2000.0;

/// Score one raw sample. Kinds without a rule yet come out `Quality::unassessed`, which scores
/// zero on purpose: a stage downstream must never mistake "nobody has judged this" for "good".
pub fn score_sample(raw: &RawSample, provenance: MetadataId) -> Sample<RawValue> {
    let quality = match raw.kind {
        StreamKind::HeartRate => score_in_range(raw.value.as_f64(), &HR_PLAUSIBLE_BPM),
        StreamKind::RrInterval => score_in_range(raw.value.as_f64(), &RR_PLAUSIBLE_MS),
        _ => Quality::unassessed(),
    };
    Sample {
        kind: raw.kind,
        device_time: raw.device_time,
        wall_time: None,
        seq: raw.seq,
        value: raw.value,
        quality,
        provenance,
    }
}

/// Score a whole decoded batch. The output length always equals the input length.
pub fn score_batch(batch: &RawSampleBatch, provenance: MetadataId) -> Vec<Sample<RawValue>> {
    batch
        .samples
        .iter()
        .map(|raw| score_sample(raw, provenance))
        .collect()
}

fn score_in_range(value: f64, range: &RangeInclusive<f64>) -> Quality {
    if range.contains(&value) {
        Quality::scored(1.0)
    } else {
        Quality::rejected(RejectReason::ImplausibleValue)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_model::ids::DeviceId;
    use mav_model::time::DeviceTime;

    fn raw(kind: StreamKind, value: RawValue) -> RawSample {
        RawSample {
            kind,
            device_time: DeviceTime::from_nanos(1_000),
            seq: 0,
            value,
        }
    }

    fn meta() -> MetadataId {
        MetadataId::new(1)
    }

    #[test]
    fn in_range_hr_scores_full() {
        let sample = score_sample(&raw(StreamKind::HeartRate, RawValue::U8(60)), meta());
        assert_eq!(sample.quality.score, 1.0);
        assert_eq!(sample.quality.reason, None);
    }

    #[test]
    fn out_of_range_hr_is_downscored_with_reason() {
        let sample = score_sample(&raw(StreamKind::HeartRate, RawValue::U16(300)), meta());
        assert_eq!(sample.quality.score, 0.0);
        assert_eq!(sample.quality.reason, Some(RejectReason::ImplausibleValue));
    }

    #[test]
    fn out_of_range_rr_is_downscored_with_reason() {
        let sample = score_sample(&raw(StreamKind::RrInterval, RawValue::U16(2500)), meta());
        assert_eq!(sample.quality.score, 0.0);
        assert_eq!(sample.quality.reason, Some(RejectReason::ImplausibleValue));
        let low = score_sample(&raw(StreamKind::RrInterval, RawValue::U16(299)), meta());
        assert_eq!(low.quality.reason, Some(RejectReason::ImplausibleValue));
    }

    #[test]
    fn boundary_values_are_in_range() {
        for (kind, value) in [
            (StreamKind::HeartRate, RawValue::U8(30)),
            (StreamKind::HeartRate, RawValue::U8(220)),
            (StreamKind::RrInterval, RawValue::U16(300)),
            (StreamKind::RrInterval, RawValue::U16(2000)),
        ] {
            let sample = score_sample(&raw(kind, value), meta());
            assert_eq!(
                sample.quality.score, 1.0,
                "{kind:?} {value:?} should be in range"
            );
        }
    }

    #[test]
    fn no_sample_is_dropped() {
        let batch = RawSampleBatch {
            device: DeviceId::new(1),
            samples: vec![
                raw(StreamKind::HeartRate, RawValue::U8(60)),
                raw(StreamKind::HeartRate, RawValue::U16(300)),
                raw(StreamKind::RrInterval, RawValue::U16(0)),
                raw(StreamKind::SkinTemp, RawValue::U16(830)),
            ],
        };
        let scored = score_batch(&batch, meta());
        assert_eq!(scored.len(), batch.samples.len());
    }

    #[test]
    fn unruled_kinds_come_out_unassessed_not_good() {
        let sample = score_sample(&raw(StreamKind::SkinTemp, RawValue::U16(830)), meta());
        assert_eq!(sample.quality, Quality::unassessed());
    }

    #[test]
    fn scored_sample_preserves_value_time_and_seq() {
        let input = RawSample {
            kind: StreamKind::RrInterval,
            device_time: DeviceTime::from_nanos(42),
            seq: 3,
            value: RawValue::U16(812),
        };
        let sample = score_sample(&input, meta());
        assert_eq!(sample.device_time, input.device_time);
        assert_eq!(sample.seq, 3);
        assert_eq!(sample.value, RawValue::U16(812));
        assert_eq!(sample.wall_time, None);
    }
}
