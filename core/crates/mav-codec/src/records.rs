//! Admitted historical record decoders (M5-P4). A version lands here only with corpus-pinned
//! offsets from docs/protocol/whoop.md; fields marked residual, refuted, or [PROV]-only stay
//! absent, and there is no fallback decode for unknown versions — their bytes stay raw evidence
//! and the journal records the version byte.

use crate::manifest::Manifest;
use mav_model::error::{codes, MavError, Result};
use mav_model::raw::RawSample;

/// Every decoder id a manifest may name in `record_versions`. Admission means a reviewed module
/// below; manifest validation rejects any other id at parse time.
pub const ADMITTED_DECODERS: &[&str] = &["r20_k18", "r20_k26"];

/// Decode one historical-record payload through the decoder the manifest admits for its version.
///
/// The inner record is `[0]` packet type, `[1]` version, `[2]` command, `[3..]` body. The version
/// keying the decoder lookup is the second byte, not the third: on a real MG type-47 record the
/// third byte is the command (`0x80`/`0x82` on-wrist), and the layout version rides the byte the
/// gen frame otherwise calls sequence. This is pinned by a real capture (fixtures/records, and
/// docs/protocol/whoop.md); an earlier synthetic fixture placed the version in the third byte and
/// no real frame agrees with it.
pub fn decode_record(manifest: &Manifest, payload: &[u8]) -> Result<Vec<RawSample>> {
    let [_, version, _command, body @ ..] = payload else {
        return Err(MavError::new(
            codes::DECODE_FIELD_UNREADABLE,
            "historical record too short for its version byte",
        )
        .context(format!("payload length {}", payload.len())));
    };
    let Some(decoder) = manifest.record_versions.get(version) else {
        return Err(MavError::new(
            codes::DECODE_UNKNOWN_RECORD_VERSION,
            "no admitted decoder for this record version",
        )
        .context(format!("version {version} (0x{version:02x})")));
    };
    match decoder.as_str() {
        "r20_k18" => r20_k18::decode(body),
        "r20_k26" => r20_k26::decode(body),
        other => Err(MavError::new(
            codes::DECODE_LAYOUT_INVALID,
            "manifest names a record decoder this build does not carry",
        )
        .context(other.to_owned())),
    }
}

fn truncated(version: &str, need: usize, got: usize) -> MavError {
    MavError::new(
        codes::DECODE_FIELD_UNREADABLE,
        "historical record shorter than its pinned length",
    )
    .context(format!("{version}: need {need} bytes, got {got}"))
}

fn u32_le(body: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([body[at], body[at + 1], body[at + 2], body[at + 3]])
}

fn seconds_to_nanos(seconds: u32) -> i64 {
    i64::from(seconds) * 1_000_000_000
}

/// The MG per-second metrics record (R20, K=18), corpus-pinned by the fourth and fifth sources
/// against ~2M records. Admitted fields only: HR `u8` at `body[11]` (zero means no optical lock
/// and is a validity sentinel, not a sample), skin temperature `i16` LE centidegrees at
/// `body[62:64]`, and the packed sleep state in bits 5–4 of `body[70]`. The secondary HR, the
/// tri-mode SpO2 byte, the refuted motion/fusion bytes, and every residual byte stay unadmitted.
mod r20_k18 {
    use super::{seconds_to_nanos, truncated, u32_le};
    use mav_model::error::Result;
    use mav_model::raw::{RawSample, RawValue};
    use mav_model::stream::StreamKind;
    use mav_model::time::DeviceTime;

    /// The corpus-pinned record length. `body[71]` (SpO2) is the highest byte the ledger pins in
    /// the admitted region; the documented record is 109 bytes and anything shorter is truncation.
    pub const MIN_BODY_LEN: usize = 109;

    pub fn decode(body: &[u8]) -> Result<Vec<RawSample>> {
        if body.len() < MIN_BODY_LEN {
            return Err(truncated("r20_k18", MIN_BODY_LEN, body.len()));
        }
        let time = DeviceTime::from_nanos(seconds_to_nanos(u32_le(body, 4)));
        let mut samples = Vec::new();
        let hr = body[11];
        if hr != 0 {
            samples.push(RawSample {
                kind: StreamKind::HeartRate,
                device_time: time,
                seq: 0,
                value: RawValue::U8(hr),
            });
        }
        let skin_temp = i16::from_le_bytes([body[62], body[63]]);
        samples.push(RawSample {
            kind: StreamKind::SkinTemp,
            device_time: time,
            seq: 0,
            value: RawValue::I16(skin_temp),
        });
        samples.push(RawSample {
            kind: StreamKind::SleepStateRaw,
            device_time: time,
            seq: 0,
            value: RawValue::U8((body[70] >> 4) & 0x03),
        });
        Ok(samples)
    }
}

/// The MG raw-PPG burst (R20, K=26): 24 `i16` LE photodiode samples at `body[16:64]`, one record
/// per second (24 Hz confirmed across 2,332 bursts). Values stay raw ADC with no invented scale;
/// each sample carries the record's second and its in-burst index as `seq`, because inventing
/// sub-second timestamps would claim a phase the wire does not state.
mod r20_k26 {
    use super::{seconds_to_nanos, truncated, u32_le};
    use mav_model::error::Result;
    use mav_model::raw::{RawSample, RawValue};
    use mav_model::stream::StreamKind;
    use mav_model::time::DeviceTime;

    /// The corpus-pinned 73-byte record body.
    pub const MIN_BODY_LEN: usize = 73;

    const SAMPLES_START: usize = 16;
    const SAMPLE_COUNT: usize = 24;

    pub fn decode(body: &[u8]) -> Result<Vec<RawSample>> {
        if body.len() < MIN_BODY_LEN {
            return Err(truncated("r20_k26", MIN_BODY_LEN, body.len()));
        }
        let time = DeviceTime::from_nanos(seconds_to_nanos(u32_le(body, 4)));
        let samples = (0..SAMPLE_COUNT)
            .map(|index| {
                let at = SAMPLES_START + index * 2;
                RawSample {
                    kind: StreamKind::Ppg,
                    device_time: time,
                    seq: index as u16,
                    value: RawValue::I16(i16::from_le_bytes([body[at], body[at + 1]])),
                }
            })
            .collect();
        Ok(samples)
    }
}

// Re-exported so tests can pin the exact boundary lengths.
pub use r20_k18::MIN_BODY_LEN as R20_K18_MIN_BODY_LEN;
pub use r20_k26::MIN_BODY_LEN as R20_K26_MIN_BODY_LEN;
