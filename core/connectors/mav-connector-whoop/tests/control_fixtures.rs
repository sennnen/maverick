//! Golden historical-control fixtures (M5-P2). Frames were built with an independent Python
//! implementation of the documented envelopes, never with mav-frame, so a decode here can
//! genuinely fail. See fixtures/control/README.md.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_connector_whoop::control::decode_control;
use mav_frame::reassembler::{Reassembler, ReassemblyEvent};
use mav_frame::WireFormat;
use mav_model::error::codes;
use mav_model::historical::HistoricalControl;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct ControlFixture {
    schema: String,
    wire_format: String,
    confidence: String,
    input_hex: String,
    expected: serde_json::Value,
}

fn load(name: &str) -> ControlFixture {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures/control")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture {}: {e}", path.display()));
    let fixture: ControlFixture = serde_json::from_str(&text).unwrap();
    assert_eq!(fixture.schema, "control/v1");
    assert!(!fixture.confidence.is_empty());
    fixture
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn decode_fixture(name: &str) -> HistoricalControl {
    let fixture = load(name);
    let format = match fixture.wire_format.as_str() {
        "gen4" => WireFormat::Gen4,
        "gen5" => WireFormat::Gen5,
        other => panic!("unknown wire_format {other}"),
    };
    let mut reassembler = Reassembler::new(format);
    let frames: Vec<_> = reassembler
        .push(&unhex(&fixture.input_hex))
        .into_iter()
        .filter_map(|event| match event {
            ReassemblyEvent::Frame(frame) => Some(frame),
            _ => None,
        })
        .collect();
    assert_eq!(
        frames.len(),
        1,
        "{name} must reassemble to exactly one frame"
    );
    let control = decode_control(&frames[0].payload)
        .unwrap()
        .unwrap_or_else(|| panic!("{name} must decode to a control value"));
    assert_eq!(
        serde_json::to_value(&control).unwrap(),
        fixture.expected,
        "{name} decode mismatch"
    );
    control
}

#[test]
fn gen4_command_response_decodes_exactly() {
    let control = decode_fixture("gen4_command_response_v1.json");
    assert!(matches!(
        control,
        HistoricalControl::Response {
            to_opcode: 22,
            origin_seq: 7,
            ..
        }
    ));
}

#[test]
fn gen5_command_response_decodes_exactly() {
    decode_fixture("gen5_command_response_v1.json");
}

#[test]
fn history_start_decodes_exactly() {
    decode_fixture("gen5_history_start_v1.json");
}

#[test]
fn history_end_extracts_the_eight_end_data_bytes() {
    let control = decode_fixture("gen5_history_end_v2.json");
    let HistoricalControl::MetadataEnd {
        ack_payload,
        record_count,
        ..
    } = control
    else {
        panic!("END must decode as MetadataEnd");
    };
    // Exactly the 8-byte end_data at inner 13..21 (trim cursor 113405, next 16) from the real
    // 5.0/MG capture — never the whole body, whose leading bytes are the record unix.
    assert_eq!(
        ack_payload,
        vec![0xFD, 0xBA, 0x01, 0x00, 0x10, 0x00, 0x00, 0x00]
    );
    assert_eq!(record_count, None);
}

#[test]
fn history_end_shorter_than_its_end_data_is_a_typed_error() {
    // A METADATA END whose body stops before inner 21 cannot yield a cursor to echo.
    let mut payload = vec![0x31, 0x09, 0x02];
    payload.extend_from_slice(&[0u8; 17]);
    let error = decode_control(&payload).unwrap_err();
    assert_eq!(error.code, codes::DECODE_FIELD_UNREADABLE);
    assert!(error.to_string().contains("end_data"), "{error}");
}

#[test]
fn history_complete_decodes_exactly() {
    decode_fixture("gen5_history_complete_v1.json");
}

#[test]
fn truncated_metadata_fails_with_a_stable_error() {
    let error = decode_control(&[0x31, 0x09]).unwrap_err();
    assert_eq!(error.code, codes::DECODE_FIELD_UNREADABLE);
    let error = decode_control(&[0x24, 0x03, 0x22]).unwrap_err();
    assert_eq!(error.code, codes::DECODE_FIELD_UNREADABLE);
}

#[test]
fn unknown_metadata_is_not_mistaken_for_completion() {
    let control = decode_control(&[0x31, 0x05, 0x07]).unwrap().unwrap();
    assert_eq!(
        control,
        HistoricalControl::MetadataUnknown { kind: 7, seq: 5 }
    );
    assert!(!matches!(
        control,
        HistoricalControl::MetadataComplete { .. }
    ));
}

#[test]
fn non_control_packets_decode_to_none() {
    assert_eq!(decode_control(&[0x28, 0x01, 0x00, 0x00]).unwrap(), None);
    assert_eq!(decode_control(&[0x10, 0x01]).unwrap(), None);
}
