//! The sample vocabulary: what kinds of streams exist, what a sample looks like, and how quality
//! travels with it. Every stage downstream of decode consumes and produces these.

use crate::ids::MetadataId;
use crate::time::{DeviceTime, WallTime};
use serde::{Deserialize, Serialize};

/// Every stream kind the pipeline knows about. Connectors map wire packets onto these; analytics
/// declare which of these they require. Adding a kind is additive and safe; renaming or removing
/// one is a frozen-interface change and needs an ADR.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    HeartRate,
    RrInterval,
    Ppg,
    /// Raw multi-channel optical ADC from the WHOOP 5.0/MG v20 deep buffer: 20-bit signed counts,
    /// six photodiode channels flattened into `seq = channel * samples_per_channel + sample`. Raw
    /// counts, no invented scale; distinct from `Ppg` (single-channel) — see ADR-015.
    OpticalRaw,
    Imu,
    /// Raw 3-axis gyroscope from the WHOOP 5.0/MG v21 deep buffer, `seq = sample * 3 + axis`. Raw
    /// `i16` LSB (× 2000/32768 deg/s per the upstream scale); distinct from `Imu` (accelerometer).
    /// See ADR-015.
    Gyro,
    Gravity,
    SkinTemp,
    /// An unscaled thermistor register readout in counts, from a device that publishes no
    /// calibrated temperature — WHOOP 4.0's v24/v25 records. Distinct from `SkinTemp`, which is
    /// degrees Celsius; see ADR-026.
    SkinTempRaw,
    Spo2Raw,
    /// A device-computed SpO2 percentage (0–100), distinct from `Spo2Raw` (unscaled optical ADC).
    /// On WHOOP 5.0/MG this is the sleep-only tri-mode byte in the K=18 record; see ADR-014 and
    /// docs/protocol/whoop.md.
    Spo2Percent,
    RespRaw,
    BatterySoc,
    StepCount,
    /// A device-classified coarse activity code (0 still, 1 walk, 2 run on WHOOP 5.0/MG K=18).
    /// The raw on-wire code, not a Maverick activity claim; see ADR-014.
    ActivityClass,
    SkinContact,
    SignalQuality,
    WristState,
    /// The K=18 packed on-wire sleep state `{0 STILL, 1 WAKE, 2 SLEEP, 3 UP}`, stored as decoded —
    /// the STILL/SLEEP split is corpus-pinned, the WAKE/UP half is provisional
    /// (docs/protocol/whoop.md). Raw wire state, not a Maverick sleep-stage claim.
    SleepStateRaw,
}

/// Why a sample was scored down or rejected. Carried with the sample so every downstream stage,
/// and the inspector, can see what the signal-quality stage decided and why.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    MotionArtifact,
    LowPerfusion,
    SensorNoise,
    OffWrist,
    ImplausibleValue,
    ImplausibleTimestamp,
}

/// A quality assessment attached to a sample: a score in [0, 1] and, when the score is poor, the
/// reason. `Quality::unassessed()` marks samples that have not passed through the SQI stage yet.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Quality {
    pub score: f32,
    pub reason: Option<RejectReason>,
}

impl Quality {
    /// Full-confidence quality, used for values that are exact on the wire (battery percent,
    /// event flags) rather than measured signals.
    pub const fn exact() -> Self {
        Self {
            score: 1.0,
            reason: None,
        }
    }

    /// The state before the SQI stage has run. Scored zero so that nothing downstream can mistake
    /// an unassessed sample for a good one.
    pub const fn unassessed() -> Self {
        Self {
            score: 0.0,
            reason: None,
        }
    }

    pub fn scored(score: f32) -> Self {
        Self {
            score: score.clamp(0.0, 1.0),
            reason: None,
        }
    }

    pub fn rejected(reason: RejectReason) -> Self {
        Self {
            score: 0.0,
            reason: Some(reason),
        }
    }
}

/// One typed sample. `device_time` is what the strap said; `wall_time` is present only once the
/// timeline has placed the sample via a clock map, and stays `None` rather than being guessed.
/// `seq` disambiguates equal values landing at the same instant (two identical RR intervals in one
/// second are two real beats, and collapsing them biases HRV; see docs/pipeline.md).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Sample<T> {
    pub kind: StreamKind,
    pub device_time: DeviceTime,
    pub wall_time: Option<WallTime>,
    pub seq: u16,
    pub value: T,
    pub quality: Quality,
    pub provenance: MetadataId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::MetadataId;

    #[test]
    fn scored_clamps_into_unit_interval() {
        assert_eq!(Quality::scored(1.7).score, 1.0);
        assert_eq!(Quality::scored(-0.2).score, 0.0);
        assert_eq!(Quality::scored(0.42).score, 0.42);
    }

    #[test]
    fn rejected_is_zero_scored_with_reason() {
        let q = Quality::rejected(RejectReason::OffWrist);
        assert_eq!(q.score, 0.0);
        assert_eq!(q.reason, Some(RejectReason::OffWrist));
    }

    #[test]
    fn stream_kind_serialises_snake_case() {
        let json = serde_json::to_string(&StreamKind::RrInterval).unwrap();
        assert_eq!(json, "\"rr_interval\"");
    }

    #[test]
    fn sample_roundtrips_through_json() {
        let sample = Sample {
            kind: StreamKind::HeartRate,
            device_time: DeviceTime::from_nanos(1_000_000_000),
            wall_time: None,
            seq: 1,
            value: 62u8,
            quality: Quality::unassessed(),
            provenance: MetadataId::new(3),
        };
        let json = serde_json::to_string(&sample).unwrap();
        let back: Sample<u8> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, sample);
    }
}
