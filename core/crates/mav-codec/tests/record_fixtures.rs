//! Golden record fixtures (M5-P4). Frames and expected samples were built to the corpus-pinned
//! offsets with an independent Python implementation, never with the code under test. See
//! fixtures/records/README.md.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_codec::codec::{DeviceCodec, ManifestCodec};
use mav_codec::kv::MemoryKv;
use mav_codec::manifest::Manifest;
use mav_codec::records::{decode_record, R20_K18_MIN_BODY_LEN, R20_K26_MIN_BODY_LEN};
use mav_frame::reassembler::{Reassembler, ReassemblyEvent};
use mav_frame::WireFormat;
use mav_model::error::codes;
use mav_model::raw::RawSample;
use mav_model::stream::StreamKind;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct RecordFixture {
    schema: String,
    confidence: String,
    input_hex: String,
    expected_samples: serde_json::Value,
}

fn load(name: &str) -> RecordFixture {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures/records")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture {}: {e}", path.display()));
    let fixture: RecordFixture = serde_json::from_str(&text).unwrap();
    assert_eq!(fixture.schema, "record/v1");
    assert!(!fixture.confidence.is_empty());
    fixture
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn manifest() -> Manifest {
    Manifest::from_json(
        r#"{
            "schema": "connector-manifest/v1",
            "identity": {
                "family": "fixture-mg",
                "display_name": "Fixture MG history",
                "models": ["FIXTURE"]
            },
            "gatt": { "service": "s", "command": "c", "notify": ["n"] },
            "frame": { "wire_format": "gen5", "max_frame_bytes": 8192 },
            "packets": { "47": "historical_data" },
            "record_versions": { "18": "r20_k18", "26": "r20_k26" },
            "capabilities": ["heart_rate", "skin_temp", "sleep_state_raw", "ppg"]
        }"#,
    )
    .unwrap()
}

/// Reassemble the fixture frame and decode it through the same ManifestCodec path the engine
/// uses, so the packet-47 routing is part of what the fixture pins.
fn decode_fixture(name: &str) -> Vec<RawSample> {
    let fixture = load(name);
    let mut reassembler = Reassembler::new(WireFormat::Gen5);
    let frames: Vec<_> = reassembler
        .push(&unhex(&fixture.input_hex))
        .into_iter()
        .filter_map(|event| match event {
            ReassemblyEvent::Frame(frame) => Some(frame),
            _ => None,
        })
        .collect();
    assert_eq!(frames.len(), 1, "{name} must reassemble to one frame");
    let manifest = manifest();
    let samples = ManifestCodec::new()
        .decode(&frames[0], &manifest, &mut MemoryKv::new())
        .unwrap();
    assert_eq!(
        serde_json::to_value(&samples).unwrap(),
        fixture.expected_samples,
        "{name} decode mismatch"
    );
    samples
}

#[test]
fn k18_decodes_exactly_the_admitted_fields() {
    let samples = decode_fixture("r20_k18_v1.json");
    let kinds: Vec<_> = samples.iter().map(|s| s.kind).collect();
    // Exactly these streams and no other: the SpO2 diagnostic byte, secondary HR, and every
    // residual/refuted byte must produce nothing.
    assert_eq!(
        kinds,
        vec![
            StreamKind::HeartRate,
            StreamKind::SkinTemp,
            StreamKind::SleepStateRaw
        ]
    );
}

#[test]
fn k26_decodes_all_twenty_four_raw_samples() {
    let samples = decode_fixture("r20_k26_v1.json");
    assert_eq!(samples.len(), 24);
    assert!(samples.iter().all(|s| s.kind == StreamKind::Ppg));
    let seqs: Vec<_> = samples.iter().map(|s| s.seq).collect();
    assert_eq!(seqs, (0u16..24).collect::<Vec<_>>());
}

#[test]
fn k18_zero_heart_rate_is_a_sentinel_not_a_sample() {
    let fixture = load("r20_k18_v1.json");
    let mut reassembler = Reassembler::new(WireFormat::Gen5);
    let frame = reassembler
        .push(&unhex(&fixture.input_hex))
        .into_iter()
        .find_map(|event| match event {
            ReassemblyEvent::Frame(frame) => Some(frame),
            _ => None,
        })
        .unwrap();
    // Rewrite body[11] (payload[14]) to the no-optical-lock sentinel and decode the raw payload.
    let mut payload = frame.payload.clone();
    payload[14] = 0;
    let samples = decode_record(&manifest(), &payload).unwrap();
    assert!(samples.iter().all(|s| s.kind != StreamKind::HeartRate));
    assert_eq!(samples.len(), 2);
}

#[test]
fn negative_skin_temperature_keeps_its_sign() {
    let fixture = load("r20_k18_v1.json");
    let mut reassembler = Reassembler::new(WireFormat::Gen5);
    let frame = reassembler
        .push(&unhex(&fixture.input_hex))
        .into_iter()
        .find_map(|event| match event {
            ReassemblyEvent::Frame(frame) => Some(frame),
            _ => None,
        })
        .unwrap();
    // body[62:64] is payload[65:67]: -150 centidegrees must come back as -150, not 65386.
    let mut payload = frame.payload.clone();
    let raw = (-150i16).to_le_bytes();
    payload[65] = raw[0];
    payload[66] = raw[1];
    let samples = decode_record(&manifest(), &payload).unwrap();
    let temp = samples
        .iter()
        .find(|s| s.kind == StreamKind::SkinTemp)
        .unwrap();
    assert_eq!(
        serde_json::to_value(temp.value).unwrap(),
        serde_json::json!({"i16": -150})
    );
}

#[test]
fn truncated_records_fail_with_the_exact_boundary() {
    let manifest = manifest();
    // One byte short of each pinned body length fails; the pinned length succeeds.
    let short_k18 = [&[0x2F, 0x01, 18][..], &[0u8; R20_K18_MIN_BODY_LEN - 1][..]].concat();
    let error = decode_record(&manifest, &short_k18).unwrap_err();
    assert_eq!(error.code, codes::DECODE_FIELD_UNREADABLE);
    let exact_k18 = [&[0x2F, 0x01, 18][..], &[0u8; R20_K18_MIN_BODY_LEN][..]].concat();
    assert!(decode_record(&manifest, &exact_k18).is_ok());

    let short_k26 = [&[0x2F, 0x01, 26][..], &[0u8; R20_K26_MIN_BODY_LEN - 1][..]].concat();
    let error = decode_record(&manifest, &short_k26).unwrap_err();
    assert_eq!(error.code, codes::DECODE_FIELD_UNREADABLE);
    let exact_k26 = [&[0x2F, 0x01, 26][..], &[0u8; R20_K26_MIN_BODY_LEN][..]].concat();
    assert!(decode_record(&manifest, &exact_k26).is_ok());
}

#[test]
fn unknown_versions_produce_no_samples_and_a_typed_error() {
    // v20 is deliberately unadmitted: the ledger marks its optical layout unknown.
    let payload = [&[0x2F, 0x01, 20][..], &[0u8; 200][..]].concat();
    let error = decode_record(&manifest(), &payload).unwrap_err();
    assert_eq!(error.code, codes::DECODE_UNKNOWN_RECORD_VERSION);
    assert!(error.context.iter().any(|c| c.contains("20")));
}
