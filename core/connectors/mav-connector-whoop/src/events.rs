//! Admitted event-vocabulary decoders (WHOOP-P5). An event packet carries a number byte that
//! selects a per-event body layout — a device-family fact the manifest layout DSL cannot express —
//! so each vocabulary is a reviewed module here and the manifest only names which one applies
//! (`event_vocabulary`). Event numbers without an admitted stream mapping decode to no samples and
//! no error, like any control packet; their bytes stay raw evidence.

use mav_model::error::{codes, MavError, Result};
use mav_model::raw::{RawSample, RawValue};
use mav_model::stream::StreamKind;
use mav_model::time::DeviceTime;

/// Every vocabulary id a manifest may name in `event_vocabulary`.
pub const ADMITTED_EVENT_VOCABULARIES: &[&str] = &["whoop"];

/// Dispatch one event payload through the vocabulary the manifest admits.
pub fn decode_event(vocabulary: &str, payload: &[u8]) -> Result<Vec<RawSample>> {
    match vocabulary {
        "whoop" => whoop::decode(payload),
        other => Err(MavError::new(
            codes::DECODE_LAYOUT_INVALID,
            "manifest names an event vocabulary this build does not carry",
        )
        .context(other.to_owned())),
    }
}

/// The WHOOP event vocabulary, identical across generations once counted from the inner record
/// (docs/protocol/whoop.md, `[WRS]` byte positions): the event number is the inner command byte
/// `[2]`, and the timestamp `u32` at inner 4 is a real RTC value. Admitted stream mappings:
///
/// - BATTERY_LEVEL (3): state of charge `u16` LE at inner 13 in deci-percent, emitted as
///   `BatterySoc` converted to percent and gated to `0..=100`; the millivolts at inner 17 and the
///   charging bit at inner 22 have no stream kind yet and stay unemitted.
/// - WRIST_ON (9) / WRIST_OFF (10): emitted as `WristState` 1/0.
///
/// The rest of the known numbers (charging on/off 7/8, double tap 14, temperature level 17, BLE
/// bonded 23, realtime HR on/off 33/34, alarms 57/58, haptics 60) are state transitions with no
/// sample stream; they decode to nothing here and belong to the transport/event journal.
mod whoop {
    use super::*;

    const BATTERY_LEVEL: u8 = 3;
    const WRIST_ON: u8 = 9;
    const WRIST_OFF: u8 = 10;

    /// The fixed event header: `[0]` packet type, `[1]` sequence, `[2]` event number, unix `u32`
    /// at `[4..8]`.
    const HEADER_LEN: usize = 8;
    const TIMESTAMP: usize = 4;
    const SOC_DECI: usize = 13;

    pub fn decode(payload: &[u8]) -> Result<Vec<RawSample>> {
        if payload.len() < HEADER_LEN {
            return Err(too_short("event header", HEADER_LEN, payload.len()));
        }
        let number = payload[2];
        let seconds = u32::from_le_bytes([
            payload[TIMESTAMP],
            payload[TIMESTAMP + 1],
            payload[TIMESTAMP + 2],
            payload[TIMESTAMP + 3],
        ]);
        let time = DeviceTime::from_nanos(i64::from(seconds) * 1_000_000_000);
        let sample = |kind, value| RawSample {
            kind,
            device_time: time,
            seq: 0,
            value,
        };
        Ok(match number {
            BATTERY_LEVEL => {
                let Some(bytes) = payload.get(SOC_DECI..SOC_DECI + 2) else {
                    return Err(too_short("battery event", SOC_DECI + 2, payload.len()));
                };
                let soc_deci = u16::from_le_bytes([bytes[0], bytes[1]]);
                if soc_deci > 1000 {
                    Vec::new()
                } else {
                    vec![sample(
                        StreamKind::BatterySoc,
                        RawValue::Converted(f64::from(soc_deci) / 10.0),
                    )]
                }
            }
            WRIST_ON => vec![sample(StreamKind::WristState, RawValue::U8(1))],
            WRIST_OFF => vec![sample(StreamKind::WristState, RawValue::U8(0))],
            _ => Vec::new(),
        })
    }

    fn too_short(what: &str, need: usize, got: usize) -> MavError {
        MavError::new(
            codes::DECODE_FIELD_UNREADABLE,
            "event packet shorter than its documented layout",
        )
        .context(format!("{what}: need {need} bytes, got {got}"))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    /// payload = inner: `[0]` type 48, `[1]` seq, `[2]` number, unix at `[4..8]`, then the body.
    fn event_payload(number: u8, len: usize) -> Vec<u8> {
        let mut p = vec![0u8; len];
        p[0] = 48;
        p[1] = 5;
        p[2] = number;
        p[4..8].copy_from_slice(&1_752_600_000u32.to_le_bytes());
        p
    }

    #[test]
    fn battery_event_converts_deci_percent_at_the_event_time() {
        let mut p = event_payload(3, 24);
        p[13..15].copy_from_slice(&812u16.to_le_bytes());
        let samples = decode_event("whoop", &p).unwrap();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].kind, StreamKind::BatterySoc);
        assert_eq!(samples[0].value, RawValue::Converted(81.2));
        assert_eq!(
            samples[0].device_time,
            DeviceTime::from_nanos(1_752_600_000 * 1_000_000_000)
        );
    }

    #[test]
    fn battery_soc_above_one_hundred_percent_is_dropped() {
        let mut p = event_payload(3, 24);
        p[13..15].copy_from_slice(&1001u16.to_le_bytes());
        assert_eq!(decode_event("whoop", &p).unwrap(), Vec::new());
    }

    #[test]
    fn wrist_events_emit_the_state_transition() {
        let on = decode_event("whoop", &event_payload(9, 8)).unwrap();
        assert_eq!(on[0].kind, StreamKind::WristState);
        assert_eq!(on[0].value, RawValue::U8(1));
        let off = decode_event("whoop", &event_payload(10, 8)).unwrap();
        assert_eq!(off[0].value, RawValue::U8(0));
    }

    #[test]
    fn an_unmapped_event_number_produces_no_samples_and_no_error() {
        // 14 = double tap: a real event with no sample stream.
        assert_eq!(
            decode_event("whoop", &event_payload(14, 12)).unwrap(),
            Vec::new()
        );
    }

    #[test]
    fn truncated_events_are_typed_errors() {
        let error = decode_event("whoop", &[48, 5, 3]).unwrap_err();
        assert_eq!(error.code, codes::DECODE_FIELD_UNREADABLE);
        // A battery event cut off before its state-of-charge bytes.
        let error = decode_event("whoop", &event_payload(3, 12)).unwrap_err();
        assert_eq!(error.code, codes::DECODE_FIELD_UNREADABLE);
        assert!(error.to_string().contains("battery event"), "{error}");
    }

    #[test]
    fn an_unadmitted_vocabulary_is_a_typed_error() {
        let error = decode_event("acme", &event_payload(3, 24)).unwrap_err();
        assert_eq!(error.code, codes::DECODE_LAYOUT_INVALID);
    }
}
