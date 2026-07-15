//! The whoop5 manifest under connectors/ is real configuration, not test data, so it gets its own
//! tests: it must parse through the manifest types, declare exactly the M1 capability set, and
//! decode a realtime frame at the documented offsets.
// Tests are allowed to panic; the workspace-level denies apply to library code.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_codec::{DeviceCodec, Manifest, ManifestCodec, MemoryKv};
use mav_frame::frame::RawFrame;
use mav_model::raw::RawValue;
use mav_model::stream::StreamKind;
use std::path::PathBuf;

fn whoop5() -> Manifest {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../connectors/whoop5/manifest.json");
    let json = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    Manifest::from_json(&json).unwrap()
}

#[test]
fn whoop5_manifest_loads_with_the_documented_identity() {
    let manifest = whoop5();
    assert_eq!(manifest.identity.family, "whoop5");
    assert!(manifest.identity.fallback_for_unknown_models);
    assert_eq!(manifest.frame.wire_format, "gen5");
    assert_eq!(
        manifest.gatt.service,
        "fd4b0001-cce1-4033-93ce-002d5875f58a"
    );
    assert_eq!(manifest.gatt.notify.len(), 4);
}

#[test]
fn whoop5_manifest_declares_exactly_hr_and_rr() {
    assert_eq!(
        whoop5().capabilities,
        vec![StreamKind::HeartRate, StreamKind::RrInterval]
    );
}

#[test]
fn whoop5_realtime_frame_decodes_at_the_documented_offsets() {
    let manifest = whoop5();
    let mut payload = vec![0u8; 22];
    payload[0] = 40;
    payload[1] = 7;
    payload[10..14].copy_from_slice(&1_752_600_000u32.to_le_bytes());
    payload[14..16].copy_from_slice(&500u16.to_le_bytes());
    payload[16] = 71;
    payload[17] = 2;
    payload[18..20].copy_from_slice(&845u16.to_le_bytes());
    payload[20..22].copy_from_slice(&0u16.to_le_bytes());

    let mut codec = ManifestCodec::new();
    let mut kv = MemoryKv::new();
    let samples = codec
        .decode(&RawFrame { payload }, &manifest, &mut kv)
        .unwrap();

    assert_eq!(
        samples.len(),
        2,
        "one HR sample plus one RR; the zero RR slot is dropped"
    );
    assert_eq!(samples[0].kind, StreamKind::HeartRate);
    assert_eq!(samples[0].value, RawValue::U8(71));
    assert_eq!(samples[1].kind, StreamKind::RrInterval);
    assert_eq!(samples[1].value, RawValue::U16(845));
}

#[test]
fn whoop5_known_control_packets_decode_to_nothing() {
    let manifest = whoop5();
    let mut codec = ManifestCodec::new();
    let mut kv = MemoryKv::new();
    for packet_type in [35u8, 36, 47, 48, 49, 50] {
        let samples = codec
            .decode(
                &RawFrame {
                    payload: vec![packet_type, 0, 0],
                },
                &manifest,
                &mut kv,
            )
            .unwrap();
        assert!(
            samples.is_empty(),
            "packet type {packet_type} should skip, not decode"
        );
    }
}
