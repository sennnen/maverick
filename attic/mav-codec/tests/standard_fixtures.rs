//! Standard-profile fixtures (PL-P8). Every case is constructed from the published Bluetooth SIG
//! specification named in the fixture's `source` field, never from the code under test. See
//! fixtures/standard/README.md.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_codec::standard::{decode_standard_profile, ADMITTED_PROFILES};
use mav_model::error::codes;
use mav_model::raw::{RawSample, RawValue};
use mav_model::stream::StreamKind;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct StandardFixture {
    schema: String,
    characteristic: String,
    source: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    payload_hex: String,
    #[serde(default)]
    expected_samples: Vec<ExpectedSample>,
    #[serde(default)]
    expected_error_code: Option<u16>,
}

#[derive(Deserialize)]
struct ExpectedSample {
    kind: String,
    #[serde(default)]
    value: Option<u16>,
    #[serde(default)]
    value_ms: Option<f64>,
}

fn load() -> StandardFixture {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures/standard/hr_measurement_v1.json");
    let fixture: StandardFixture =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(fixture.schema, "standard-record/v1");
    assert_eq!(fixture.characteristic, "2A37");
    assert!(fixture.source.contains("[PROV]"));
    fixture
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn heart_rate_is_the_admitted_profile() {
    assert!(ADMITTED_PROFILES.contains(&"heart_rate"));
}

#[test]
fn every_fixture_case_decodes_or_fails_exactly() {
    let fixture = load();
    for case in &fixture.cases {
        let payload = unhex(&case.payload_hex);
        let mut seq = 0u16;
        let result = decode_standard_profile("heart_rate", &payload, &mut seq);
        match case.expected_error_code {
            Some(code) => {
                let error = result.expect_err(&case.name);
                assert_eq!(error.code, code, "case {}", case.name);
            }
            None => {
                let samples = result.expect(&case.name);
                assert_expected(&case.name, &samples, &case.expected_samples);
            }
        }
    }
}

fn assert_expected(name: &str, samples: &[RawSample], expected: &[ExpectedSample]) {
    assert_eq!(samples.len(), expected.len(), "case {name}");
    for (sample, want) in samples.iter().zip(expected) {
        let kind = match want.kind.as_str() {
            "heart_rate" => StreamKind::HeartRate,
            "rr_interval" => StreamKind::RrInterval,
            other => panic!("case {name}: unexpected kind {other}"),
        };
        assert_eq!(sample.kind, kind, "case {name}");
        match (want.value, want.value_ms) {
            (Some(value), None) => assert_eq!(sample.value, RawValue::U16(value), "case {name}"),
            (None, Some(ms)) => match sample.value {
                RawValue::Converted(got) => assert_eq!(got, ms, "case {name}"),
                other => panic!("case {name}: rr carried {other:?}, want Converted"),
            },
            _ => panic!("case {name}: expected sample needs exactly one of value/value_ms"),
        }
    }
}

#[test]
fn the_sample_sequence_is_session_monotonic() {
    let fixture = load();
    let case = &fixture.cases[0];
    let mut seq = 0u16;
    let first = decode_standard_profile("heart_rate", &unhex(&case.payload_hex), &mut seq).unwrap();
    let second =
        decode_standard_profile("heart_rate", &unhex(&case.payload_hex), &mut seq).unwrap();
    assert_eq!(first[0].seq, 0);
    assert_eq!(second[0].seq, 1);
    assert_eq!(seq, 2);
}

#[test]
fn an_unadmitted_profile_is_a_typed_error() {
    let mut seq = 0u16;
    let error = decode_standard_profile("blood_pressure", &[0x00], &mut seq).unwrap_err();
    assert_eq!(error.code, codes::DECODE_UNKNOWN_PACKET_TYPE);
}
