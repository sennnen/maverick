//! The mock connector lives in the core repo (it is a fixture for the abstraction, not a
//! distributable device), so its manifest is tested here. M2-P1: it loads through the mav-codec
//! types, declares HeartRate and not RrInterval, and its custom frame spec resolves.
// Tests are allowed to panic; the workspace-level denies apply to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_codec::Manifest;
use mav_frame::spec::{CrcKind, Endian};
use mav_model::stream::StreamKind;
use std::path::PathBuf;

fn mock() -> Manifest {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../connectors/mock/manifest.json");
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    Manifest::from_json(&json).unwrap()
}

#[test]
fn mock_manifest_loads() {
    let manifest = mock();
    assert_eq!(manifest.identity.family, "mock");
    assert_eq!(manifest.capabilities, vec![StreamKind::HeartRate]);
    assert!(
        !manifest.capabilities.contains(&StreamKind::RrInterval),
        "the mock deliberately lacks RR so M3 can prove capability negotiation"
    );
}

#[test]
fn mock_custom_frame_spec_resolves() {
    let spec = mock().frame.to_spec().unwrap();
    assert_eq!(spec.sof, 0x5A);
    assert_eq!(spec.header_len, 3);
    assert_eq!(spec.length.endian, Endian::Be);
    assert!(!spec.length_includes_trailer);
    assert!(spec.header_crc.is_none());
    assert_eq!(spec.trailer.kind, CrcKind::Crc8);
}
