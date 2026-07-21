//! Admitted decoders for Bluetooth SIG standard profiles (PL-P8). Each profile is a reviewed
//! module whose flag-driven offsets come from the published SIG specification, not a capture;
//! fixtures/standard/README.md names it.
//!
//! Standard characteristics carry no device clock, so samples leave here with a zero device time
//! and a session-monotonic sequence; the pipeline stamps the capture wall on them (the honest
//! time of a clockless reading) before scoring.

use mav_model::error::{codes, MavError, Result};
use mav_model::raw::{RawSample, RawValue};
use mav_model::stream::StreamKind;
use mav_model::time::DeviceTime;

/// The standard profiles this build decodes. Any other selector fails validation.
pub const ADMITTED_PROFILES: &[&str] = &["heart_rate"];

/// Decode one notification value of the named standard profile. `seq` is the session-monotonic
/// sample counter that keeps equal readings distinct for dedup; the caller owns it per session.
pub fn decode_standard_profile(
    profile: &str,
    payload: &[u8],
    seq: &mut u16,
) -> Result<Vec<RawSample>> {
    match profile {
        "heart_rate" => heart_rate_measurement(payload, seq),
        other => Err(MavError::new(
            codes::DECODE_UNKNOWN_PACKET_TYPE,
            "standard profile is not admitted by this build",
        )
        .context(other.to_owned())),
    }
}

/// Heart Rate Measurement (`0x2A37`): flags bit0 selects u8/u16 heart rate, bits 1–2 are sensor
/// contact (ignored), bit3 adds a u16 energy-expended field (skipped), bit4 appends u16
/// RR-intervals in 1/1024 s. A zero heart rate is the no-reading sentinel and emits no sample;
/// RR intervals convert to exact milliseconds (`units * 125 / 128` is a dyadic division).
fn heart_rate_measurement(payload: &[u8], seq: &mut u16) -> Result<Vec<RawSample>> {
    let flags = *payload.first().ok_or_else(|| truncated("flags", 0))?;
    let mut at = 1usize;

    let heart_rate = if flags & 0x01 != 0 {
        let bytes = payload
            .get(at..at + 2)
            .ok_or_else(|| truncated("u16 heart rate", at))?;
        at += 2;
        u16::from_le_bytes([bytes[0], bytes[1]])
    } else {
        let byte = *payload.get(at).ok_or_else(|| truncated("heart rate", at))?;
        at += 1;
        u16::from(byte)
    };

    if flags & 0x08 != 0 {
        payload
            .get(at..at + 2)
            .ok_or_else(|| truncated("energy expended", at))?;
        at += 2;
    }

    let mut samples = Vec::new();
    if heart_rate > 0 {
        samples.push(sample(
            StreamKind::HeartRate,
            RawValue::U16(heart_rate),
            seq,
        ));
    }

    if flags & 0x10 != 0 {
        let rr_bytes = &payload[at.min(payload.len())..];
        if rr_bytes.is_empty() || !rr_bytes.len().is_multiple_of(2) {
            return Err(truncated("rr intervals", at));
        }
        for pair in rr_bytes.chunks_exact(2) {
            let units = u16::from_le_bytes([pair[0], pair[1]]);
            let ms = f64::from(units) * 125.0 / 128.0;
            samples.push(sample(StreamKind::RrInterval, RawValue::Converted(ms), seq));
        }
    }
    Ok(samples)
}

fn sample(kind: StreamKind, value: RawValue, seq: &mut u16) -> RawSample {
    let this = *seq;
    *seq = seq.wrapping_add(1);
    RawSample {
        kind,
        device_time: DeviceTime::from_nanos(0),
        seq: this,
        value,
    }
}

fn truncated(field: &str, at: usize) -> MavError {
    MavError::new(
        codes::DECODE_FIELD_UNREADABLE,
        "standard heart-rate payload ends before a promised field",
    )
    .context(format!("{field} at byte {at}"))
}
