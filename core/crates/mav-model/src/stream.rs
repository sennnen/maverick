//! The sample vocabulary: what kinds of streams exist, what a sample looks like, and how quality
//! and clock placement travel with it. Every stage downstream of decode consumes and produces
//! these.

use crate::ids::MetadataId;
use crate::time::{DeviceTime, WallTime};
use serde::{Deserialize, Serialize};

/// Every stream kind the pipeline knows about. Connectors map wire packets onto these; analytics
/// declare which of these they require. Appending a kind is additive and safe; renaming, removing,
/// or reordering one is a frozen-interface change and needs an ADR — the declaration order is the
/// on-disk code (see [`StreamKind::code`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    HeartRate,
    /// True R-peak-to-R-peak intervals from an electrical cardiac signal. Only this kind may be
    /// labelled heart-rate variability; see docs/analytics.md.
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
    /// A single-lead electrical cardiac waveform in raw ADC counts. The source of `RrInterval`
    /// when a device exposes the waveform rather than the intervals; see ADR-027.
    Ecg,
    /// Beat-to-beat intervals timed from an optical pulse rather than an electrical R peak. The
    /// same arithmetic as `RrInterval` over a different physiological event, which is why it is a
    /// different kind and serialises as pulse-rate variability; see ADR-027.
    PulseInterval,
    /// Red-channel reflectance photoplethysmogram in raw ADC counts, paired with `InfraredPpg` for
    /// ratio-of-ratios oximetry; see ADR-027.
    RedPpg,
    /// Infrared-channel reflectance photoplethysmogram in raw ADC counts; see ADR-027.
    InfraredPpg,
    /// Ambient-light photodiode counts, the reference channel the two illuminated channels are
    /// interpreted against; see ADR-027.
    AmbientLight,
}

/// Every kind, ordered so that index equals [`StreamKind::code`]. Appending is safe; reordering is
/// a schema break and `stream_codes_are_frozen` fails on it.
pub const STREAM_KINDS: [StreamKind; 24] = [
    StreamKind::HeartRate,
    StreamKind::RrInterval,
    StreamKind::Ppg,
    StreamKind::OpticalRaw,
    StreamKind::Imu,
    StreamKind::Gyro,
    StreamKind::Gravity,
    StreamKind::SkinTemp,
    StreamKind::SkinTempRaw,
    StreamKind::Spo2Raw,
    StreamKind::Spo2Percent,
    StreamKind::RespRaw,
    StreamKind::BatterySoc,
    StreamKind::StepCount,
    StreamKind::ActivityClass,
    StreamKind::SkinContact,
    StreamKind::SignalQuality,
    StreamKind::WristState,
    StreamKind::SleepStateRaw,
    StreamKind::Ecg,
    StreamKind::PulseInterval,
    StreamKind::RedPpg,
    StreamKind::InfraredPpg,
    StreamKind::AmbientLight,
];

/// The name each kind is published under, in the same order. This is the one vocabulary the
/// stored JSON, the FFI and both apps share; `stream_names_match_serialisation` pins it to what
/// serde emits so the two can never drift into naming the same stream twice.
pub const STREAM_NAMES: [&str; 24] = [
    "heart_rate",
    "rr_interval",
    "ppg",
    "optical_raw",
    "imu",
    "gyro",
    "gravity",
    "skin_temp",
    "skin_temp_raw",
    "spo2_raw",
    "spo2_percent",
    "resp_raw",
    "battery_soc",
    "step_count",
    "activity_class",
    "skin_contact",
    "signal_quality",
    "wrist_state",
    "sleep_state_raw",
    "ecg",
    "pulse_interval",
    "red_ppg",
    "infrared_ppg",
    "ambient_light",
];

impl StreamKind {
    /// The durable integer this kind is stored and indexed by. Integer keys are what let the
    /// sample table's primary key stay narrow and its index stay a straight range scan.
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// The name this kind is published under, everywhere outside the connector wire.
    pub const fn name(self) -> &'static str {
        STREAM_NAMES[self.code() as usize]
    }

    pub fn from_code(code: u8) -> Option<Self> {
        STREAM_KINDS.get(code as usize).copied()
    }

    /// True for the two kinds that carry beat-to-beat intervals in milliseconds. The variability
    /// analytics accept either and label the result by which one arrived.
    pub const fn is_interval(self) -> bool {
        matches!(self, Self::RrInterval | Self::PulseInterval)
    }
}

/// Why a sample's *value* was scored down. Placement of the sample in time is a separate axis and
/// lives in [`Placement`]: a sample whose clock needed correcting still measured what it measured.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    MotionArtifact,
    LowPerfusion,
    SensorNoise,
    OffWrist,
    ImplausibleValue,
}

/// Every reason, ordered so that index equals [`RejectReason::code`].
pub const REJECT_REASONS: [RejectReason; 5] = [
    RejectReason::MotionArtifact,
    RejectReason::LowPerfusion,
    RejectReason::SensorNoise,
    RejectReason::OffWrist,
    RejectReason::ImplausibleValue,
];

impl RejectReason {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(code: u8) -> Option<Self> {
        REJECT_REASONS.get(code as usize).copied()
    }
}

/// A quality assessment attached to a sample: a score in [0, 1] and, when the score is poor, the
/// reason. Every sample leaving the signal-quality stage is scored; `Quality::unassessed` marks
/// only the window before that stage has run.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Quality {
    pub score: f32,
    pub reason: Option<RejectReason>,
}

impl Quality {
    /// Full-confidence quality, used for values that are exact on the wire (battery percent,
    /// event flags, raw ADC counts) rather than measured signals inferred from them.
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

    pub const fn rejected(reason: RejectReason) -> Self {
        Self {
            score: 0.0,
            reason: Some(reason),
        }
    }

    /// True when the value is worth using at all. The one gate every consumer shares, so that
    /// "usable" cannot mean two things in two places.
    pub fn is_usable(self) -> bool {
        self.score > 0.0
    }
}

/// Where a sample's wall-clock time came from. Carrying the instant inside the variant makes an
/// unexplained wall time unrepresentable, and keeps clock trouble out of [`Quality`], which is
/// about the value alone.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// The timeline has not placed this sample yet.
    Unplaced,
    /// The device timestamp was plausible and was used directly.
    DeviceClock(WallTime),
    /// The device clock was stale and a learned offset shifted the whole segment, so the gaps
    /// between samples survive intact.
    Corrected(WallTime),
    /// The device clock was stale and no correction covered it, so the phone's capture time
    /// applies and the gaps inside that burst are gone.
    CaptureFallback(WallTime),
}

impl Placement {
    pub const fn wall_time(self) -> Option<WallTime> {
        match self {
            Self::Unplaced => None,
            Self::DeviceClock(at) | Self::Corrected(at) | Self::CaptureFallback(at) => Some(at),
        }
    }

    /// True when the instant came from a clock we trust, rather than from a correction or the
    /// phone. Analytics that measure intervals care; a daily bucket does not.
    pub const fn is_trusted(self) -> bool {
        matches!(self, Self::DeviceClock(_))
    }

    pub const fn code(self) -> u8 {
        match self {
            Self::Unplaced => 0,
            Self::DeviceClock(_) => 1,
            Self::Corrected(_) => 2,
            Self::CaptureFallback(_) => 3,
        }
    }

    /// Rebuild from the two columns the store keeps. An unknown code or a missing instant reads
    /// back as `Unplaced` rather than inventing a placement.
    pub fn from_parts(code: u8, at: Option<WallTime>) -> Self {
        match (code, at) {
            (1, Some(at)) => Self::DeviceClock(at),
            (2, Some(at)) => Self::Corrected(at),
            (3, Some(at)) => Self::CaptureFallback(at),
            _ => Self::Unplaced,
        }
    }
}

/// One typed sample. `device_time` is what the strap said and is never mutated; `placement` is
/// what the timeline made of it. `seq` disambiguates equal values landing at the same instant (two
/// identical intervals in one second are two real beats, and collapsing them biases variability;
/// see docs/pipeline.md).
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Sample<T> {
    pub kind: StreamKind,
    pub device_time: DeviceTime,
    pub placement: Placement,
    pub seq: u16,
    pub value: T,
    pub quality: Quality,
    pub provenance: MetadataId,
}

impl<T> Sample<T> {
    pub const fn wall_time(&self) -> Option<WallTime> {
        self.placement.wall_time()
    }
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
        assert!(!q.is_usable());
        assert_eq!(q.reason, Some(RejectReason::OffWrist));
    }

    #[test]
    fn stream_kind_serialises_snake_case() {
        let json = serde_json::to_string(&StreamKind::RrInterval).unwrap();
        assert_eq!(json, "\"rr_interval\"");
    }

    /// The stored code is the declaration index, so reordering the enum would silently relabel
    /// every row already on disk. These pairs are the frozen contract; appending is what is safe.
    #[test]
    fn stream_codes_are_frozen() {
        for (index, kind) in STREAM_KINDS.iter().enumerate() {
            assert_eq!(kind.code() as usize, index, "{kind:?}");
            assert_eq!(StreamKind::from_code(kind.code()), Some(*kind));
        }
        assert_eq!(StreamKind::HeartRate.code(), 0);
        assert_eq!(StreamKind::RrInterval.code(), 1);
        assert_eq!(StreamKind::SleepStateRaw.code(), 18);
        assert_eq!(StreamKind::Ecg.code(), 19);
        assert_eq!(StreamKind::AmbientLight.code(), 23);
        assert_eq!(StreamKind::from_code(24), None);
    }

    /// The FFI used to publish `format!("{kind:?}").to_lowercase()`, which named the stream
    /// `rrinterval` while the same stream serialised as `rr_interval` two fields away. Both apps
    /// then matched on the wrong one. One name, checked against the serialiser.
    #[test]
    fn stream_names_match_serialisation() {
        for kind in STREAM_KINDS {
            let serialised = serde_json::to_string(&kind).unwrap();
            assert_eq!(format!("\"{}\"", kind.name()), serialised, "{kind:?}");
        }
    }

    #[test]
    fn reject_codes_are_frozen() {
        for (index, reason) in REJECT_REASONS.iter().enumerate() {
            assert_eq!(reason.code() as usize, index, "{reason:?}");
            assert_eq!(RejectReason::from_code(reason.code()), Some(*reason));
        }
        assert_eq!(RejectReason::from_code(5), None);
    }

    #[test]
    fn only_the_two_interval_kinds_are_intervals() {
        for kind in STREAM_KINDS {
            assert_eq!(
                kind.is_interval(),
                matches!(kind, StreamKind::RrInterval | StreamKind::PulseInterval),
                "{kind:?}"
            );
        }
    }

    /// A wall time can only exist alongside the story of where it came from, and that story is
    /// never a quality judgement about the measurement.
    #[test]
    fn placement_roundtrips_through_its_stored_parts() {
        let at = crate::time::WallTime::from_unix_seconds(1_752_600_000);
        for placement in [
            Placement::Unplaced,
            Placement::DeviceClock(at),
            Placement::Corrected(at),
            Placement::CaptureFallback(at),
        ] {
            assert_eq!(
                Placement::from_parts(placement.code(), placement.wall_time()),
                placement
            );
        }
        assert_eq!(Placement::from_parts(1, None), Placement::Unplaced);
        assert_eq!(Placement::from_parts(9, Some(at)), Placement::Unplaced);
    }

    #[test]
    fn sample_roundtrips_through_json() {
        let sample = Sample {
            kind: StreamKind::HeartRate,
            device_time: DeviceTime::from_nanos(1_000_000_000),
            placement: Placement::Unplaced,
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
