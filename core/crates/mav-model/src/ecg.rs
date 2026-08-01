//! Durable, platform-neutral records for one captured ECG and its provisional interpretation.

use crate::ids::{DeviceId, EcgCaptureId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EcgRhythmClass {
    SinusRhythm,
    AtrialFibrillation,
    OtherAbnormalRhythm,
}

impl EcgRhythmClass {
    pub const fn model_index(self) -> usize {
        match self {
            Self::SinusRhythm => 0,
            Self::AtrialFibrillation => 1,
            Self::OtherAbnormalRhythm => 2,
        }
    }

    pub const fn model_code(self) -> &'static str {
        match self {
            Self::SinusRhythm => "N",
            Self::AtrialFibrillation => "A",
            Self::OtherAbnormalRhythm => "O",
        }
    }

    pub const fn from_model_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::SinusRhythm),
            1 => Some(Self::AtrialFibrillation),
            2 => Some(Self::OtherAbnormalRhythm),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EcgExplanationSegment {
    pub start_second: u8,
    pub end_second: u8,
    pub importance_milli: u16,
}

/// Native tensor output plus enough immutable capture identity to rebuild the interpretation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EcgInferenceEvidence {
    pub capture_id: EcgCaptureId,
    pub device_id: DeviceId,
    pub started_ns: i64,
    pub ended_ns: i64,
    pub source_rate_hz: u32,
    pub source_unit: String,
    pub sample_count: u32,
    pub raw_sha256: String,
    pub tensor_sha256: String,
    pub preprocessing_sha256: String,
    pub model_sha256: String,
    pub quality_milli: u16,
    /// Baseline `N/A/O`, followed by six ordered five-second occlusion outputs.
    pub predictions: Vec<[f32; 3]>,
    pub created_ns: i64,
}

/// Rebuildable result. Raw evidence and native predictions remain authoritative.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EcgResult {
    pub capture_id: EcgCaptureId,
    pub device_id: DeviceId,
    pub started_ns: i64,
    pub ended_ns: i64,
    pub source_rate_hz: u32,
    pub sample_count: u32,
    pub rhythm: EcgRhythmClass,
    pub probabilities: [f32; 3],
    pub confidence_milli: u16,
    pub quality_milli: u16,
    /// Mean rate over the recording, from the admitted R-peak detector (ADR-034). `None` on
    /// results stored before the field existed, and on a recording with too few beats to average
    /// — an absent rate is a fact about the reading, not a zero.
    #[serde(default)]
    pub mean_heart_rate_bpm: Option<u16>,
    pub explanation: Vec<EcgExplanationSegment>,
    pub raw_sha256: String,
    pub tensor_sha256: String,
    pub preprocessing_sha256: String,
    pub model_sha256: String,
    pub algorithm_id: String,
    pub algorithm_version: String,
    pub provisional: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_class_order_is_frozen() {
        for (index, class, code) in [
            (0, EcgRhythmClass::SinusRhythm, "N"),
            (1, EcgRhythmClass::AtrialFibrillation, "A"),
            (2, EcgRhythmClass::OtherAbnormalRhythm, "O"),
        ] {
            assert_eq!(EcgRhythmClass::from_model_index(index), Some(class));
            assert_eq!(class.model_index(), index);
            assert_eq!(class.model_code(), code);
        }
        assert_eq!(EcgRhythmClass::from_model_index(3), None);
    }
}
