//! Admitted historical record decoders (M5-P4). A version lands here only with corpus-pinned
//! offsets from docs/protocol/whoop.md; fields marked residual, refuted, or [PROV]-only stay
//! absent, and there is no fallback decode for unknown versions — their bytes stay raw evidence
//! and the journal records the version byte.

use crate::manifest::Manifest;
use mav_model::error::{codes, MavError, Result};
use mav_model::raw::RawSample;

/// Every decoder id a manifest may name in `record_versions`. Admission means a reviewed module
/// below; manifest validation rejects any other id at parse time.
pub const ADMITTED_DECODERS: &[&str] = &["r20_k18", "r20_k26", "gen4_v24", "gen4_v5", "gen4_v25"];

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
        "gen4_v24" => gen4_v24::decode(body),
        "gen4_v5" => gen4_v5::decode(body),
        "gen4_v25" => gen4_v25::decode(body),
        other => Err(MavError::new(
            codes::DECODE_LAYOUT_INVALID,
            "manifest names a record decoder this build does not carry",
        )
        .context(other.to_owned())),
    }
}

/// The historical R-R block is a 4-wide fixed slot array across every version that carries it.
const RR_MAX_SLOTS: usize = 4;

/// Read a `u16` LE only if both bytes are present, so a variable-length R-R block past the record
/// end drops the slot rather than panicking.
fn u16_le_opt(body: &[u8], at: usize) -> Option<u16> {
    body.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
}

/// The historical R-R block: a slot count at `count_at` (clamped to `max`) then that many `u16` LE
/// from `first_at`, dropping any zero (an empty slot) and any slot past the record end. The caller
/// assigns `seq` by kept-index so two equal intervals in one second survive dedup as two beats.
fn rr_values(body: &[u8], count_at: usize, first_at: usize, max: usize) -> Vec<u16> {
    let count = body.get(count_at).map_or(0, |&c| c as usize).min(max);
    (0..count)
        .filter_map(|slot| u16_le_opt(body, first_at + slot * 2))
        .filter(|&rr| rr != 0)
        .collect()
}

/// Accept a gravity triplet only if every component is finite and the magnitude is physically
/// plausible (`|g|` in `[0.5, 1.5)`), so a wrong offset or garbage bytes drop rather than store.
fn accept_gravity(g: [f32; 3]) -> Option<[f32; 3]> {
    if !g.iter().all(|v| v.is_finite()) {
        return None;
    }
    let magnitude = (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt();
    (0.5..1.5).contains(&magnitude).then_some(g)
}

fn truncated(version: &str, need: usize, got: usize) -> MavError {
    MavError::new(
        codes::DECODE_FIELD_UNREADABLE,
        "historical record shorter than its pinned length",
    )
    .context(format!("{version}: need {need} bytes, got {got}"))
}

fn u16_le(body: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([body[at], body[at + 1]])
}

fn u32_le(body: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([body[at], body[at + 1], body[at + 2], body[at + 3]])
}

fn seconds_to_nanos(seconds: u32) -> i64 {
    i64::from(seconds) * 1_000_000_000
}

/// The MG per-second metrics record (R20, K=18), corpus-pinned by the fourth, fifth, and sixth
/// (`[WRS]`) sources against millions of records. Admitted fields, each range-gated so a wrong
/// offset on unmapped firmware yields nothing:
///
/// - HR `u8` at `body[11]`; zero means no optical lock and is a validity sentinel, not a sample.
/// - R-R intervals: a slot count `u8` at `body[12]` (clamped to 4) then that many consecutive
///   `u16` LE from `body[13]`, dropping any zero (an empty slot); `seq` is the kept-index so two
///   equal intervals in one second survive dedup as two beats.
/// - gravity: three `f32` LE from `body[34]`, accepted only if finite and `|g|` in `[0.5, 1.5)`;
///   emitted as three samples `seq` 0/1/2 (x, y, z).
/// - skin temperature: `u16` LE register at `body[62:64]`, kept only when `raw/100` °C is in
///   `[5, 45)` — a raw register, not a Maverick °C claim (the consumer owns the final scale).
/// - SpO2 percent: the sleep-only tri-mode `u8` at `body[71]`, kept only in `70..=100` (bit-7
///   sentinels and sub-70 diagnostic codes drop); an awake record carries 0 and emits nothing.
/// - steps: cumulative `u16` LE counter at `body[46]`.
/// - activity class: `u8` at `body[52]`, kept only in `{0, 1, 2}`.
/// - packed sleep state in bits 5–4 of `body[70]`.
/// - signal quality: the PPG confidence `u8` at `body[29]` (255 = clean; empirical).
///
/// The secondary HR, the refuted motion/fusion bytes, and the empirical `signal_flags` bitfield at
/// `body[22]` (no clean stream kind yet) stay unadmitted; their bytes remain raw evidence.
mod r20_k18 {
    use super::{
        accept_gravity, rr_values, seconds_to_nanos, truncated, u16_le, u32_le, RR_MAX_SLOTS,
    };
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
        let sample = |kind, seq, value| RawSample {
            kind,
            device_time: time,
            seq,
            value,
        };
        let mut samples = Vec::new();

        let hr = body[11];
        if hr != 0 {
            samples.push(sample(StreamKind::HeartRate, 0, RawValue::U8(hr)));
        }

        for (i, rr) in rr_values(body, 12, 13, RR_MAX_SLOTS)
            .into_iter()
            .enumerate()
        {
            samples.push(sample(StreamKind::RrInterval, i as u16, RawValue::U16(rr)));
        }

        let g = [
            f32::from_le_bytes([body[34], body[35], body[36], body[37]]),
            f32::from_le_bytes([body[38], body[39], body[40], body[41]]),
            f32::from_le_bytes([body[42], body[43], body[44], body[45]]),
        ];
        if let Some(g) = accept_gravity(g) {
            for (axis, component) in g.iter().enumerate() {
                samples.push(sample(
                    StreamKind::Gravity,
                    axis as u16,
                    RawValue::Converted(f64::from(*component)),
                ));
            }
        }

        let skin_temp = u16_le(body, 62);
        if (5.0..45.0).contains(&(f32::from(skin_temp) / 100.0)) {
            samples.push(sample(StreamKind::SkinTemp, 0, RawValue::U16(skin_temp)));
        }

        let spo2 = body[71];
        if (70..=100).contains(&spo2) {
            samples.push(sample(StreamKind::Spo2Percent, 0, RawValue::U8(spo2)));
        }

        samples.push(sample(
            StreamKind::StepCount,
            0,
            RawValue::U16(u16_le(body, 46)),
        ));

        let activity = body[52];
        if activity <= 2 {
            samples.push(sample(StreamKind::ActivityClass, 0, RawValue::U8(activity)));
        }

        samples.push(sample(
            StreamKind::SleepStateRaw,
            0,
            RawValue::U8((body[70] >> 4) & 0x03),
        ));
        samples.push(sample(StreamKind::SignalQuality, 0, RawValue::U8(body[29])));

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

/// The WHOOP 4.0 historical DSP record (v24, also carried as v12): HR `u8` at `body[14]`, the R-R
/// block at `body[15]`/`body[16]`, gravity as three `f32` LE from `body[33]`, the SpO2 red/IR raw
/// ADC pair at `body[61]`/`body[63]`, the skin-temperature register at `body[65:67]`, and raw
/// respiration at `body[73:75]`. `[PROV]`/`[WRS]`: the offsets reproduce a real 4.0 capture
/// byte-for-byte, but the 4.0 offload path is not yet exercised on our own hardware, and the
/// absolute skin-temp °C scale stays a per-device learned anchor (deferred, ADR-009) — so the raw
/// register is admitted, not a temperature claim.
mod gen4_v24 {
    use super::{
        accept_gravity, rr_values, seconds_to_nanos, truncated, u16_le, u32_le, RR_MAX_SLOTS,
    };
    use mav_model::error::Result;
    use mav_model::raw::{RawSample, RawValue};
    use mav_model::stream::StreamKind;
    use mav_model::time::DeviceTime;

    /// Enough to read every admitted field; respiration at `body[73:75]` is the highest.
    pub const MIN_BODY_LEN: usize = 75;

    pub fn decode(body: &[u8]) -> Result<Vec<RawSample>> {
        if body.len() < MIN_BODY_LEN {
            return Err(truncated("gen4_v24", MIN_BODY_LEN, body.len()));
        }
        let time = DeviceTime::from_nanos(seconds_to_nanos(u32_le(body, 4)));
        let sample = |kind, seq, value| RawSample {
            kind,
            device_time: time,
            seq,
            value,
        };
        let mut samples = Vec::new();

        let hr = body[14];
        if hr != 0 {
            samples.push(sample(StreamKind::HeartRate, 0, RawValue::U8(hr)));
        }

        for (i, rr) in rr_values(body, 15, 16, RR_MAX_SLOTS)
            .into_iter()
            .enumerate()
        {
            samples.push(sample(StreamKind::RrInterval, i as u16, RawValue::U16(rr)));
        }

        let g = [
            f32::from_le_bytes([body[33], body[34], body[35], body[36]]),
            f32::from_le_bytes([body[37], body[38], body[39], body[40]]),
            f32::from_le_bytes([body[41], body[42], body[43], body[44]]),
        ];
        if let Some(g) = accept_gravity(g) {
            for (axis, component) in g.iter().enumerate() {
                samples.push(sample(
                    StreamKind::Gravity,
                    axis as u16,
                    RawValue::Converted(f64::from(*component)),
                ));
            }
        }

        // SpO2 as the raw red/IR ADC pair (seq 0 red, seq 1 IR); the ratio-of-ratios lives later.
        samples.push(sample(
            StreamKind::Spo2Raw,
            0,
            RawValue::U16(u16_le(body, 61)),
        ));
        samples.push(sample(
            StreamKind::Spo2Raw,
            1,
            RawValue::U16(u16_le(body, 63)),
        ));
        // Raw skin-temp register: no °C scale claimed here (per-device learned anchor, deferred).
        samples.push(sample(
            StreamKind::SkinTemp,
            0,
            RawValue::U16(u16_le(body, 65)),
        ));
        samples.push(sample(
            StreamKind::RespRaw,
            0,
            RawValue::U16(u16_le(body, 73)),
        ));

        Ok(samples)
    }
}

/// The WHOOP 4.0 generic record (v5, also v7/v9): HR at `body[14]` and the R-R block at
/// `body[15]`/`body[16]`, with no DSP block. `[PROV]`/`[WRS]`: offsets carried over from the v24
/// header, exercised by an invariant round-trip test (no real v5 capture exists yet).
mod gen4_v5 {
    use super::{rr_values, seconds_to_nanos, truncated, u32_le, RR_MAX_SLOTS};
    use mav_model::error::Result;
    use mav_model::raw::{RawSample, RawValue};
    use mav_model::stream::StreamKind;
    use mav_model::time::DeviceTime;

    /// Enough for the timestamp, HR, and the R-R count; the R-R values are read bounds-safe.
    pub const MIN_BODY_LEN: usize = 16;

    pub fn decode(body: &[u8]) -> Result<Vec<RawSample>> {
        if body.len() < MIN_BODY_LEN {
            return Err(truncated("gen4_v5", MIN_BODY_LEN, body.len()));
        }
        let time = DeviceTime::from_nanos(seconds_to_nanos(u32_le(body, 4)));
        let sample = |kind, seq, value| RawSample {
            kind,
            device_time: time,
            seq,
            value,
        };
        let mut samples = Vec::new();

        let hr = body[14];
        if hr != 0 {
            samples.push(sample(StreamKind::HeartRate, 0, RawValue::U8(hr)));
        }
        for (i, rr) in rr_values(body, 15, 16, RR_MAX_SLOTS)
            .into_iter()
            .enumerate()
        {
            samples.push(sample(StreamKind::RrInterval, i as u16, RawValue::U16(rr)));
        }

        Ok(samples)
    }
}

/// The WHOOP 4.0 v25 record (PPG-waveform buffer): no per-second HR, only a gravity triplet stored
/// as `i16 / 16384` at `body[66]`/`body[68]`/`body[70]`, gated to `|g|` near 1. `[PROV]`/`[WRS]`:
/// three real 4.0 captures pin the decode; not yet exercised on our own hardware.
mod gen4_v25 {
    use super::{accept_gravity, seconds_to_nanos, truncated, u32_le};
    use mav_model::error::Result;
    use mav_model::raw::{RawSample, RawValue};
    use mav_model::stream::StreamKind;
    use mav_model::time::DeviceTime;

    /// Enough to read the gravity triplet at `body[66:72]`.
    pub const MIN_BODY_LEN: usize = 72;

    pub fn decode(body: &[u8]) -> Result<Vec<RawSample>> {
        if body.len() < MIN_BODY_LEN {
            return Err(truncated("gen4_v25", MIN_BODY_LEN, body.len()));
        }
        let time = DeviceTime::from_nanos(seconds_to_nanos(u32_le(body, 4)));
        let axis = |at: usize| f32::from(i16::from_le_bytes([body[at], body[at + 1]])) / 16384.0;
        let g = [axis(66), axis(68), axis(70)];
        let mut samples = Vec::new();
        if let Some(g) = accept_gravity(g) {
            for (index, component) in g.iter().enumerate() {
                samples.push(RawSample {
                    kind: StreamKind::Gravity,
                    device_time: time,
                    seq: index as u16,
                    value: RawValue::Converted(f64::from(*component)),
                });
            }
        }
        Ok(samples)
    }
}

// Re-exported so tests can pin the exact boundary lengths.
pub use gen4_v24::MIN_BODY_LEN as GEN4_V24_MIN_BODY_LEN;
pub use gen4_v25::MIN_BODY_LEN as GEN4_V25_MIN_BODY_LEN;
pub use gen4_v5::MIN_BODY_LEN as GEN4_V5_MIN_BODY_LEN;
pub use r20_k18::MIN_BODY_LEN as R20_K18_MIN_BODY_LEN;
pub use r20_k26::MIN_BODY_LEN as R20_K26_MIN_BODY_LEN;
