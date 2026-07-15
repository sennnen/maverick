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
    Imu,
    Gravity,
    SkinTemp,
    Spo2Raw,
    RespRaw,
    BatterySoc,
    StepCount,
    SkinContact,
    SignalQuality,
    WristState,
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
