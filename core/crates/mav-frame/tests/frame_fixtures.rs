//! Golden frame fixtures. The fixture is the arbiter: its bytes come from a real capture or an
//! independent implementation, never from the code under test, so these tests can actually catch
//! the code being wrong. See fixtures/README.md.
// Tests are allowed to panic; the workspace-level denies apply to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_frame::frame::{build_frame, WireFormat};
use mav_frame::reassembler::{Reassembler, ReassemblyEvent};
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Deserialize)]
struct FrameFixture {
    schema: String,
    wire_format: String,
    confidence: String,
    input_hex: String,
    expected_payload_hex: String,
}

fn load(name: &str) -> FrameFixture {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures/frame")
        .join(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read fixture {}: {e}", path.display()));
    let fixture: FrameFixture = serde_json::from_str(&text).unwrap();
    assert_eq!(fixture.schema, "frame/v1");
    assert!(!fixture.confidence.is_empty());
    fixture
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn wire_format(name: &str) -> WireFormat {
    match name {
        "gen4" => WireFormat::Gen4,
        "gen5" => WireFormat::Gen5,
        other => panic!("unknown wire_format {other}"),
    }
}

fn assert_fixture_reassembles(name: &str) {
    let fixture = load(name);
    let format = wire_format(&fixture.wire_format);
    let mut r = Reassembler::new(format);
    let events = r.push(&unhex(&fixture.input_hex));

    let frames: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ReassemblyEvent::Frame(f) => Some(f.payload.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        frames.len(),
        1,
        "expected exactly one frame, got {events:?}"
    );
    assert_eq!(frames[0], unhex(&fixture.expected_payload_hex));
    assert_eq!(r.pending(), 0);
}

#[test]
fn gen5_hello_fixture_reassembles() {
    assert_fixture_reassembles("gen5_hello_v1.json");
}

#[test]
fn gen4_frame_fixture_reassembles() {
    assert_fixture_reassembles("gen4_frame_v1.json");
}

#[test]
fn builders_reproduce_fixture_bytes() {
    for name in ["gen5_hello_v1.json", "gen4_frame_v1.json"] {
        let fixture = load(name);
        let built = build_frame(
            wire_format(&fixture.wire_format),
            &unhex(&fixture.expected_payload_hex),
        )
        .unwrap();
        assert_eq!(
            built,
            unhex(&fixture.input_hex),
            "builder diverged from {name}"
        );
    }
}
