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
/// A battery state of charge is a percentage or it is not a battery reading.
pub const BATTERY_PERCENT: RangeInclusive<f64> = 0.0..=100.0;
/// SpO2 as the device computed it. Below 70 the optical estimate is not meaningful, and both
/// surveyed codebases gate there.
pub const SPO2_PERCENT: RangeInclusive<f64> = 70.0..=100.0;
/// A boolean the device stated, as 0 or 1.
const BOOLEAN: RangeInclusive<f64> = 0.0..=1.0;

/// Score one raw sample.
///
/// Three groups. **Measured signals** — heart rate, RR — are gated on physiological plausibility.
/// **Exact-on-wire readouts** — a battery percentage, a wrist flag, a step counter — are things the
/// strap stated rather than things we measured, so they score `Quality::exact()` once their value
/// is in the range the reading can occupy; a value outside it is rejected with a reason, because a
/// battery cannot be at 140%. **Raw counts** are inputs to analysis that nobody has judged yet, and
/// come out `Quality::unassessed`, which scores zero on purpose: a stage downstream must never
/// mistake "nobody has judged this" for "good".
pub fn score_sample(raw: &RawSample, provenance: MetadataId) -> Sample<RawValue> {
    let value = raw.value.as_f64();
    let quality = match raw.kind {
        StreamKind::HeartRate => score_in_range(value, &HR_PLAUSIBLE_BPM),
        StreamKind::RrInterval => score_in_range(value, &RR_PLAUSIBLE_MS),
        StreamKind::BatterySoc => exact_in_range(value, &BATTERY_PERCENT),
        StreamKind::Spo2Percent => exact_in_range(value, &SPO2_PERCENT),
        StreamKind::WristState => exact_in_range(value, &BOOLEAN),
        // Gravity, counters, and device-stated codes carry no range we can assert without
        // inventing one, so they are exact whenever they are a finite number.
        StreamKind::Gravity
        | StreamKind::StepCount
        | StreamKind::ActivityClass
        | StreamKind::SleepStateRaw => exact_if_finite(value),
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

fn exact_in_range(value: f64, range: &RangeInclusive<f64>) -> Quality {
    if range.contains(&value) {
        Quality::exact()
    } else {
        Quality::rejected(RejectReason::ImplausibleValue)
    }
}

fn exact_if_finite(value: f64) -> Quality {
    if value.is_finite() {
        Quality::exact()
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

    /// A battery percentage is a number the strap stated, not a signal we measured. Leaving it
    /// unassessed scores it zero, and the FFI drops zero-scored samples — so the app could never
    /// see a battery level no matter what a connector emitted.
    #[test]
    fn exact_on_wire_readouts_score_full() {
        for (kind, value) in [
            (StreamKind::BatterySoc, RawValue::U8(81)),
            (StreamKind::WristState, RawValue::U8(1)),
            (StreamKind::WristState, RawValue::U8(0)),
            (StreamKind::Gravity, RawValue::I16(-1024)),
            (StreamKind::Spo2Percent, RawValue::U8(97)),
            (StreamKind::StepCount, RawValue::U16(4_200)),
            (StreamKind::ActivityClass, RawValue::U8(2)),
            (StreamKind::SleepStateRaw, RawValue::U8(3)),
        ] {
            let sample = score_sample(&raw(kind, value), meta());
            assert_eq!(sample.quality, Quality::exact(), "{kind:?} {value:?}");
        }
    }

    /// The range gate still applies: a battery percentage outside 0..=100 is not something the
    /// strap can have meant, so it is rejected with a reason rather than passed through as exact.
    #[test]
    fn an_impossible_battery_percentage_is_rejected_not_trusted() {
        let sample = score_sample(&raw(StreamKind::BatterySoc, RawValue::U16(140)), meta());
        assert_eq!(sample.quality.score, 0.0);
        assert_eq!(sample.quality.reason, Some(RejectReason::ImplausibleValue));
    }

    /// Raw sensor counts are inputs to analysis, not statements of fact. They stay unassessed
    /// until a stage that understands them scores them.
    #[test]
    fn raw_signal_kinds_stay_unassessed() {
        for kind in [
            StreamKind::Ppg,
            StreamKind::OpticalRaw,
            StreamKind::Imu,
            StreamKind::Gyro,
            StreamKind::SkinTemp,
            StreamKind::SkinTempRaw,
            StreamKind::Spo2Raw,
            StreamKind::RespRaw,
            StreamKind::SignalQuality,
            StreamKind::SkinContact,
        ] {
            let sample = score_sample(&raw(kind, RawValue::U16(1)), meta());
            assert_eq!(sample.quality, Quality::unassessed(), "{kind:?}");
        }
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
