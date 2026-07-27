//! Signal quality: raw samples in, scored samples out, nothing dropped.
//!
//! Every stream kind has a plausibility band and the match over them is exhaustive, so adding a
//! kind is a compile error here rather than a silent fall-through to zero. That fall-through was
//! real: ten kinds scored zero because nobody had written a rule, the FFI drops zero-scored
//! samples, and so those streams could never reach the app however well the sensor worked.
//!
//! A value outside its band is scored down with a reason and kept. Deleting it here would hide
//! what the sensor said from the inspector, and the no-silent-drops rule (docs/pipeline.md)
//! applies to quality judgements too: downstream stages decide what a low score is worth for their
//! metric, with the reason attached.
//!
//! The bands live here and not in a manifest because they are analytic judgement about physiology,
//! not facts about a wire format.
#![forbid(unsafe_code)]

use mav_model::ids::MetadataId;
use mav_model::raw::{RawSample, RawSampleBatch, RawValue};
use mav_model::stream::{Placement, Quality, RejectReason, Sample, StreamKind};
use std::ops::RangeInclusive;

/// Plausible human heart rate, in whole bpm.
pub const HR_PLAUSIBLE_BPM: RangeInclusive<f64> = 30.0..=220.0;
/// Plausible beat-to-beat interval, in milliseconds; matches the range both surveyed codebases
/// gate on before variability work.
pub const INTERVAL_PLAUSIBLE_MS: RangeInclusive<f64> = 300.0..=2000.0;
/// A state of charge is a percentage or it is not a battery reading.
pub const BATTERY_PERCENT: RangeInclusive<f64> = 0.0..=100.0;
/// SpO2 as the device computed it. Below 70 the optical estimate is not meaningful, and both
/// surveyed codebases gate there.
pub const SPO2_PERCENT: RangeInclusive<f64> = 70.0..=100.0;
/// Skin temperature of a strap being worn. Outside this the thermistor is reading the room.
pub const SKIN_TEMP_CELSIUS: RangeInclusive<f64> = 20.0..=45.0;
/// A percentage the device stated about itself.
const PERCENT: RangeInclusive<f64> = 0.0..=100.0;
/// A boolean the device stated, as 0 or 1.
const BOOLEAN: RangeInclusive<f64> = 0.0..=1.0;
/// Raw converter counts and device-stated codes carry no band we could assert without inventing
/// one. They still have to be finite, which [`score_sample`] checks separately.
const ANY_FINITE: RangeInclusive<f64> = f64::NEG_INFINITY..=f64::INFINITY;

/// The band a kind's value has to fall inside to be believable. Exhaustive on purpose: a new
/// `StreamKind` cannot ship without someone deciding what a plausible value of it looks like.
pub fn plausible(kind: StreamKind) -> RangeInclusive<f64> {
    match kind {
        StreamKind::HeartRate => HR_PLAUSIBLE_BPM,
        StreamKind::RrInterval | StreamKind::PulseInterval => INTERVAL_PLAUSIBLE_MS,
        StreamKind::BatterySoc => BATTERY_PERCENT,
        StreamKind::Spo2Percent => SPO2_PERCENT,
        StreamKind::SkinTemp => SKIN_TEMP_CELSIUS,
        StreamKind::SignalQuality => PERCENT,
        StreamKind::WristState | StreamKind::SkinContact => BOOLEAN,
        StreamKind::Ppg
        | StreamKind::OpticalRaw
        | StreamKind::Ecg
        | StreamKind::RedPpg
        | StreamKind::InfraredPpg
        | StreamKind::AmbientLight
        | StreamKind::Imu
        | StreamKind::Gyro
        | StreamKind::Gravity
        | StreamKind::SkinTempRaw
        | StreamKind::Spo2Raw
        | StreamKind::RespRaw
        | StreamKind::StepCount
        | StreamKind::ActivityClass
        | StreamKind::SleepStateRaw => ANY_FINITE,
    }
}

/// Score one raw sample against its band. A finite in-band value is full confidence; anything else
/// is rejected with `ImplausibleValue` and kept.
pub fn score_sample(raw: &RawSample, provenance: MetadataId) -> Sample<RawValue> {
    let value = raw.value.as_f64();
    Sample {
        kind: raw.kind,
        device_time: raw.device_time,
        placement: Placement::Unplaced,
        seq: raw.seq,
        value: raw.value,
        quality: if value.is_finite() && plausible(raw.kind).contains(&value) {
            Quality::exact()
        } else {
            Quality::rejected(RejectReason::ImplausibleValue)
        },
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

#[cfg(test)]
mod tests {
    use super::*;
    use mav_model::ids::DeviceId;
    use mav_model::stream::STREAM_KINDS;
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

    fn score(kind: StreamKind, value: RawValue) -> Quality {
        score_sample(&raw(kind, value), meta()).quality
    }

    #[test]
    fn in_band_values_score_full() {
        for (kind, value) in [
            (StreamKind::HeartRate, RawValue::U8(60)),
            (StreamKind::RrInterval, RawValue::U16(812)),
            (StreamKind::PulseInterval, RawValue::U16(812)),
            (StreamKind::BatterySoc, RawValue::U8(81)),
            (StreamKind::Spo2Percent, RawValue::U8(97)),
            (StreamKind::SkinTemp, RawValue::Converted(33.4)),
            (StreamKind::WristState, RawValue::U8(1)),
            (StreamKind::SignalQuality, RawValue::U8(72)),
        ] {
            assert_eq!(score(kind, value), Quality::exact(), "{kind:?} {value:?}");
        }
    }

    #[test]
    fn out_of_band_values_are_downscored_with_a_reason() {
        for (kind, value) in [
            (StreamKind::HeartRate, RawValue::U16(300)),
            (StreamKind::RrInterval, RawValue::U16(2500)),
            (StreamKind::RrInterval, RawValue::U16(299)),
            (StreamKind::PulseInterval, RawValue::U16(0)),
            (StreamKind::BatterySoc, RawValue::U16(140)),
            (StreamKind::SkinTemp, RawValue::Converted(-5.0)),
            (StreamKind::WristState, RawValue::U8(2)),
        ] {
            assert_eq!(
                score(kind, value),
                Quality::rejected(RejectReason::ImplausibleValue),
                "{kind:?} {value:?}"
            );
        }
    }

    #[test]
    fn boundary_values_are_in_band() {
        for (kind, value) in [
            (StreamKind::HeartRate, RawValue::U8(30)),
            (StreamKind::HeartRate, RawValue::U8(220)),
            (StreamKind::RrInterval, RawValue::U16(300)),
            (StreamKind::RrInterval, RawValue::U16(2000)),
        ] {
            assert_eq!(score(kind, value), Quality::exact(), "{kind:?} {value:?}");
        }
    }

    /// The finding this file was rewritten for. Ten kinds used to fall through to a zero score,
    /// the FFI drops zero-scored samples, and so raw optical, IMU, respiration and skin
    /// temperature could never reach the app — while the capability graph blamed a missing stream.
    #[test]
    fn every_stream_kind_can_produce_a_usable_sample() {
        for kind in STREAM_KINDS {
            let band = plausible(kind);
            let representative = if band.start().is_finite() && band.end().is_finite() {
                (band.start() + band.end()) / 2.0
            } else {
                1_234.0
            };
            let quality = score(kind, RawValue::Converted(representative));
            assert!(
                quality.is_usable(),
                "{kind:?} scored {} — nothing downstream would ever see it",
                quality.score
            );
        }
    }

    #[test]
    fn a_non_finite_raw_count_is_still_rejected() {
        assert_eq!(
            score(StreamKind::Ecg, RawValue::F32(f32::NAN)),
            Quality::rejected(RejectReason::ImplausibleValue)
        );
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
        assert_eq!(score_batch(&batch, meta()).len(), batch.samples.len());
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
        assert_eq!(sample.placement, Placement::Unplaced);
    }
}
