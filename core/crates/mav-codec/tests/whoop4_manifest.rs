//! The whoop4 manifest is real configuration and gets the same treatment as whoop5: parse, declare
//! the M1 capability set, and decode a realtime frame at the gen4 offsets (the gen5 offsets minus
//! four). The gen4 skin-temp codec is deliberately absent until Milestone 5; see the manifest's
//! confidence_note and docs/connectors.md.
// Tests are allowed to panic; the workspace-level denies apply to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_codec::{DeviceCodec, Manifest, ManifestCodec, MemoryKv, Registry};
use mav_frame::frame::RawFrame;
use mav_model::raw::RawValue;
use mav_model::stream::StreamKind;
use std::path::PathBuf;

fn read(rel: &str) -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel);
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    Manifest::from_json(&json).unwrap()
}

fn whoop4() -> Manifest {
    read("../../../connectors/whoop4/manifest.json")
}

#[test]
fn whoop4_manifest_loads_as_gen4() {
    let manifest = whoop4();
    assert_eq!(manifest.identity.family, "whoop4");
    assert!(!manifest.identity.fallback_for_unknown_models);
    assert_eq!(manifest.frame.wire_format, "gen4");
    assert_eq!(
        manifest.gatt.service,
        "61080001-8d6d-82b8-614a-1c8cb0f8dcc6"
    );
    assert_eq!(
        manifest.gatt.notify.len(),
        3,
        "the 4.0 has no fd4b0007-equivalent notify char"
    );
    assert_eq!(
        manifest.capabilities,
        vec![StreamKind::HeartRate, StreamKind::RrInterval]
    );
}

#[test]
fn whoop4_heart_rate_answers_unbonded() {
    // This is the property that let the 4.0 be a pushover: 2A37 works with no bond.
    let sg = whoop4().standard_gatt.unwrap();
    assert!(sg.heart_rate_unbonded);
    assert_eq!(sg.heart_rate.unwrap().characteristic, "2A37");
}

#[test]
fn whoop4_realtime_decodes_at_gen4_offsets() {
    let manifest = whoop4();
    // Inner payload with gen4 offsets: ts@6, subsec@10, HR@12, rr_count@13, rr@14.
    let mut payload = vec![0u8; 18];
    payload[0] = 40;
    payload[1] = 3;
    payload[6..10].copy_from_slice(&1_752_600_000u32.to_le_bytes());
    payload[10..12].copy_from_slice(&250u16.to_le_bytes());
    payload[12] = 58;
    payload[13] = 2;
    payload[14..16].copy_from_slice(&930u16.to_le_bytes());
    payload[16..18].copy_from_slice(&0u16.to_le_bytes());

    let mut codec = ManifestCodec::new();
    let mut kv = MemoryKv::new();
    let samples = codec
        .decode(&RawFrame { payload }, &manifest, &mut kv)
        .unwrap();

    assert_eq!(
        samples.len(),
        2,
        "HR plus one RR; the zero RR slot is dropped"
    );
    assert_eq!(samples[0].kind, StreamKind::HeartRate);
    assert_eq!(samples[0].value, RawValue::U8(58));
    assert_eq!(samples[1].value, RawValue::U16(930));
}

#[test]
fn whoop4_and_whoop5_resolve_distinctly_but_bare_whoop_is_gen5() {
    let mut registry = Registry::new();
    registry.register(whoop4()).unwrap();
    registry
        .register(read("../../../connectors/whoop5/manifest.json"))
        .unwrap();

    assert_eq!(
        registry
            .resolve("WHOOP 4.0")
            .unwrap()
            .manifest()
            .identity
            .family,
        "whoop4"
    );
    assert_eq!(
        registry
            .resolve("WHOOP MG")
            .unwrap()
            .manifest()
            .identity
            .family,
        "whoop5"
    );
    // The legacy bare "WHOOP" row must fall through to gen5, not to the 4.0.
    assert_eq!(
        registry
            .resolve("WHOOP")
            .unwrap()
            .manifest()
            .identity
            .family,
        "whoop5"
    );
}
