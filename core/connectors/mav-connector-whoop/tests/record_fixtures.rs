//! Golden record fixtures. The input frames are real WHOOP 5.0/MG captures (imported from
//! tanarchytan/whoop-rs) and the expected samples were computed by an independent decode, never the
//! code under test. The inner record is `[type][version][command][body..]`: version is inner[1] and
//! the command (0x80 on-wrist r22) is inner[2]. See fixtures/records/README.md.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_codec::codec::DeviceCodec;
use mav_codec::kv::MemoryKv;
use mav_codec::manifest::Manifest;
use mav_connector_whoop::records::{
    decode_record, GEN4_V24_MIN_BODY_LEN, GEN5_V20_MIN_BODY_LEN, GEN5_V21_MIN_BODY_LEN,
    R20_K18_MIN_BODY_LEN, R20_K26_MIN_BODY_LEN,
};
use mav_connector_whoop::WhoopCodec;
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
    wire_format: String,
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
            "codec": "whoop",
            "packets": { "47": "historical_data" },
            "record_versions": { "18": "r20_k18", "26": "r20_k26", "20": "gen5_v20", "21": "gen5_v21" },
            "capabilities": ["heart_rate", "rr_interval", "gravity", "skin_temp", "spo2_percent", "step_count", "activity_class", "sleep_state_raw", "signal_quality", "ppg", "imu", "gyro", "optical_raw"]
        }"#,
    )
    .unwrap()
}

fn gen4_manifest() -> Manifest {
    Manifest::from_json(
        r#"{
            "schema": "connector-manifest/v1",
            "identity": {
                "family": "fixture-4",
                "display_name": "Fixture 4.0 history",
                "models": ["FIXTURE4"]
            },
            "gatt": { "service": "s", "command": "c", "notify": ["n"] },
            "frame": { "wire_format": "gen4", "max_frame_bytes": 8192 },
            "codec": "whoop",
            "packets": { "47": "historical_data" },
            "record_versions": { "5": "gen4_v5", "7": "gen4_v5", "9": "gen4_v5", "12": "gen4_v24", "24": "gen4_v24", "25": "gen4_v25" },
            "capabilities": ["heart_rate", "rr_interval", "gravity", "spo2_raw", "skin_temp", "resp_raw"]
        }"#,
    )
    .unwrap()
}

/// Reassemble the fixture frame and decode it through the same WhoopCodec path the engine
/// uses, so the packet-47 routing is part of what the fixture pins.
fn decode_fixture(name: &str) -> Vec<RawSample> {
    let fixture = load(name);
    let (wire, manifest) = match fixture.wire_format.as_str() {
        "gen5" => (WireFormat::Gen5, manifest()),
        "gen4" => (WireFormat::Gen4, gen4_manifest()),
        other => panic!("{name}: unknown wire_format {other}"),
    };
    let mut reassembler = Reassembler::new(wire);
    let frames: Vec<_> = reassembler
        .push(&unhex(&fixture.input_hex))
        .into_iter()
        .filter_map(|event| match event {
            ReassemblyEvent::Frame(frame) => Some(frame),
            _ => None,
        })
        .collect();
    assert_eq!(frames.len(), 1, "{name} must reassemble to one frame");
    let samples = WhoopCodec::new()
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
    // Exactly these streams and no other on this real awake frame: the sleep-only SpO2 byte is 0
    // and stays absent, the secondary HR and empirical signal_flags bitfield are unadmitted, and
    // every residual/refuted byte must produce nothing.
    assert_eq!(
        kinds,
        vec![
            StreamKind::HeartRate,
            StreamKind::RrInterval,
            StreamKind::RrInterval,
            StreamKind::Gravity,
            StreamKind::Gravity,
            StreamKind::Gravity,
            StreamKind::SkinTemp,
            StreamKind::StepCount,
            StreamKind::ActivityClass,
            StreamKind::SleepStateRaw,
            StreamKind::SignalQuality,
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
    // The other ten admitted streams on this frame are untouched; only HR drops.
    assert_eq!(samples.len(), 10);
}

/// Build a synthetic v18 payload (`[type][version][command] + body`) with `set` applied to the body,
/// so an invariant can exercise a field the real awake fixture does not (e.g. a valid SpO2).
fn v18_payload(set: impl Fn(&mut [u8])) -> Vec<u8> {
    let mut body = vec![0u8; R20_K18_MIN_BODY_LEN];
    set(&mut body);
    [&[0x2F, 18, 0x00][..], &body].concat()
}

fn decode_v18(set: impl Fn(&mut [u8])) -> Vec<RawSample> {
    decode_record(&manifest(), &v18_payload(set)).unwrap()
}

fn value_of(samples: &[RawSample], kind: StreamKind) -> Option<serde_json::Value> {
    samples
        .iter()
        .find(|s| s.kind == kind)
        .map(|s| serde_json::to_value(s.value).unwrap())
}

#[test]
fn k18_skin_temp_kept_in_band_dropped_out_of_band() {
    // body[62:64] is the raw u16 register; °C = raw/100 is admitted only in [5, 45).
    let worn = decode_v18(|b| b[62..64].copy_from_slice(&3345u16.to_le_bytes()));
    assert_eq!(
        value_of(&worn, StreamKind::SkinTemp),
        Some(serde_json::json!({ "u16": 3345 }))
    );
    // 3.0 °C is below the floor and 50.0 °C above the ceiling: both drop rather than store garbage.
    let cold = decode_v18(|b| b[62..64].copy_from_slice(&300u16.to_le_bytes()));
    assert_eq!(value_of(&cold, StreamKind::SkinTemp), None);
    let hot = decode_v18(|b| b[62..64].copy_from_slice(&5000u16.to_le_bytes()));
    assert_eq!(value_of(&hot, StreamKind::SkinTemp), None);
}

#[test]
fn k18_spo2_tri_mode_gates_sentinels_and_codes() {
    // The sleep-only byte at body[71]: a %-range value is a real SpO2; bit-7 sentinels and sub-70
    // diagnostic codes are not readings and must not be stored.
    let spo2 = |v: u8| value_of(&decode_v18(|b| b[71] = v), StreamKind::Spo2Percent);
    assert_eq!(spo2(98), Some(serde_json::json!({ "u8": 98 }))); // real percentage
    assert_eq!(spo2(8), None); // low diagnostic code
    assert_eq!(spo2(0xA8), None); // bit-7 saturation sentinel
    assert_eq!(spo2(0), None); // no reading
}

#[test]
fn k18_activity_class_gates_out_invalid_codes() {
    let act = |v: u8| value_of(&decode_v18(|b| b[52] = v), StreamKind::ActivityClass);
    assert_eq!(act(0), Some(serde_json::json!({ "u8": 0 }))); // still
    assert_eq!(act(2), Some(serde_json::json!({ "u8": 2 }))); // run
    assert_eq!(act(0xFF), None); // invalid sentinel
    assert_eq!(act(7), None); // unmapped code
}

#[test]
fn k18_gravity_rejects_implausible_magnitude() {
    // An all-zero body has |g| = 0, outside [0.5, 1.5): no gravity samples at all.
    let zeroed = decode_v18(|_| {});
    assert!(zeroed.iter().all(|s| s.kind != StreamKind::Gravity));
    // A unit vector down the x axis is |g| = 1: exactly three gravity samples, seq 0/1/2.
    let unit = decode_v18(|b| b[34..38].copy_from_slice(&1.0f32.to_le_bytes()));
    let gravity: Vec<_> = unit
        .iter()
        .filter(|s| s.kind == StreamKind::Gravity)
        .collect();
    assert_eq!(gravity.len(), 3);
    assert_eq!(
        gravity.iter().map(|s| s.seq).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(
        serde_json::to_value(gravity[0].value).unwrap(),
        serde_json::json!({ "converted": 1.0 })
    );
}

#[test]
fn truncated_records_fail_with_the_exact_boundary() {
    let manifest = manifest();
    // One byte short of each pinned body length fails; the pinned length succeeds.
    let short_k18 = [&[0x2F, 18, 0x00][..], &[0u8; R20_K18_MIN_BODY_LEN - 1][..]].concat();
    let error = decode_record(&manifest, &short_k18).unwrap_err();
    assert_eq!(error.code, codes::DECODE_FIELD_UNREADABLE);
    let exact_k18 = [&[0x2F, 18, 0x00][..], &[0u8; R20_K18_MIN_BODY_LEN][..]].concat();
    assert!(decode_record(&manifest, &exact_k18).is_ok());

    let short_k26 = [&[0x2F, 26, 0x00][..], &[0u8; R20_K26_MIN_BODY_LEN - 1][..]].concat();
    let error = decode_record(&manifest, &short_k26).unwrap_err();
    assert_eq!(error.code, codes::DECODE_FIELD_UNREADABLE);
    let exact_k26 = [&[0x2F, 26, 0x00][..], &[0u8; R20_K26_MIN_BODY_LEN][..]].concat();
    assert!(decode_record(&manifest, &exact_k26).is_ok());
}

#[test]
fn unknown_versions_produce_no_samples_and_a_typed_error() {
    // Version 99 is in no manifest's record_versions: no fallback decode, a typed error instead.
    let payload = [&[0x2F, 99, 0x00][..], &[0u8; 200][..]].concat();
    let error = decode_record(&manifest(), &payload).unwrap_err();
    assert_eq!(error.code, codes::DECODE_UNKNOWN_RECORD_VERSION);
    assert!(error.context.iter().any(|c| c.contains("99")));
}

#[test]
fn gen4_v24_decodes_the_real_worn_record() {
    let samples = decode_fixture("gen4_v24_v1.json");
    let kinds: Vec<_> = samples.iter().map(|s| s.kind).collect();
    // HR, two R-R, the gravity triplet, the SpO2 red/IR raw pair, the skin-temp register, and
    // respiration — the full DSP block and nothing else.
    assert_eq!(
        kinds,
        vec![
            StreamKind::HeartRate,
            StreamKind::RrInterval,
            StreamKind::RrInterval,
            StreamKind::Gravity,
            StreamKind::Gravity,
            StreamKind::Gravity,
            StreamKind::Spo2Raw,
            StreamKind::Spo2Raw,
            StreamKind::SkinTemp,
            StreamKind::RespRaw,
        ]
    );
    // The SpO2 pair keeps red (seq 0) and IR (seq 1) distinct.
    let spo2: Vec<_> = samples
        .iter()
        .filter(|s| s.kind == StreamKind::Spo2Raw)
        .map(|s| (s.seq, serde_json::to_value(s.value).unwrap()))
        .collect();
    assert_eq!(
        spo2,
        vec![
            (0, serde_json::json!({ "u16": 592 })),
            (1, serde_json::json!({ "u16": 612 })),
        ]
    );
}

#[test]
fn gen4_v25_decodes_gravity_only() {
    let samples = decode_fixture("gen4_v25_v1.json");
    assert_eq!(samples.len(), 3);
    assert!(samples.iter().all(|s| s.kind == StreamKind::Gravity));
    assert_eq!(
        samples.iter().map(|s| s.seq).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
}

#[test]
fn gen4_v5_round_trips_hr_and_rr() {
    // No real v5 capture exists, so pin the offsets with an invariant round-trip: place HR and two
    // R-R intervals at the header offsets and require the decoder to recover exactly them.
    let mut body = vec![0u8; 24];
    body[14] = 88; // HR
    body[15] = 2; // R-R slot count
    body[16..18].copy_from_slice(&600u16.to_le_bytes());
    body[18..20].copy_from_slice(&610u16.to_le_bytes());
    let payload = [&[0x2F, 5, 0x00][..], &body].concat();
    let samples = decode_record(&gen4_manifest(), &payload).unwrap();
    let decoded: Vec<_> = samples
        .iter()
        .map(|s| (s.kind, serde_json::to_value(s.value).unwrap()))
        .collect();
    assert_eq!(
        decoded,
        vec![
            (StreamKind::HeartRate, serde_json::json!({ "u8": 88 })),
            (StreamKind::RrInterval, serde_json::json!({ "u16": 600 })),
            (StreamKind::RrInterval, serde_json::json!({ "u16": 610 })),
        ]
    );
}

#[test]
fn gen4_v24_truncated_fails_with_the_exact_boundary() {
    let manifest = gen4_manifest();
    let short = [
        &[0x2F, 24, 0x00][..],
        &vec![0u8; GEN4_V24_MIN_BODY_LEN - 1][..],
    ]
    .concat();
    let error = decode_record(&manifest, &short).unwrap_err();
    assert_eq!(error.code, codes::DECODE_FIELD_UNREADABLE);
    let exact = [&[0x2F, 24, 0x00][..], &vec![0u8; GEN4_V24_MIN_BODY_LEN][..]].concat();
    assert!(decode_record(&manifest, &exact).is_ok());
}

// The v20/v21 deep buffers have no real capture, so they are pinned by synthetic invariant tests
// (ADR-015): a constructed buffer decodes to exactly its planted samples, and a buffer that fails
// the structural gate decodes to nothing rather than to plausible garbage.

fn gen5_v21_payload(set: impl Fn(&mut [u8])) -> Vec<u8> {
    let mut body = vec![0u8; GEN5_V21_MIN_BODY_LEN];
    set(&mut body);
    [&[0x2F, 21, 0x00][..], &body].concat()
}

#[test]
fn gen5_v21_imu_decodes_100_sample_6axis() {
    let payload = gen5_v21_payload(|b| {
        b[13..15].copy_from_slice(&100u16.to_le_bytes()); // accel sample count
        b[619..621].copy_from_slice(&100u16.to_le_bytes()); // gyro sample count
        b[17..19].copy_from_slice(&4096i16.to_le_bytes()); // ax[0] (= 1 g)
        b[629..631].copy_from_slice(&250i16.to_le_bytes()); // gx[0]
    });
    let samples = decode_record(&manifest(), &payload).unwrap();
    let accel: Vec<_> = samples
        .iter()
        .filter(|s| s.kind == StreamKind::Imu)
        .collect();
    let gyro: Vec<_> = samples
        .iter()
        .filter(|s| s.kind == StreamKind::Gyro)
        .collect();
    assert_eq!(accel.len(), 300); // 100 samples x 3 axes
    assert_eq!(gyro.len(), 300);
    // seq = sample*3 + axis, so ax[0] is seq 0 and ay[0]/az[0] are seq 1/2 (zero here).
    assert_eq!(accel[0].seq, 0);
    assert_eq!(
        serde_json::to_value(accel[0].value).unwrap(),
        serde_json::json!({ "i16": 4096 })
    );
    assert_eq!(
        serde_json::to_value(accel[1].value).unwrap(),
        serde_json::json!({ "i16": 0 })
    );
    assert_eq!(
        serde_json::to_value(gyro[0].value).unwrap(),
        serde_json::json!({ "i16": 250 })
    );
}

#[test]
fn gen5_v21_imu_rejects_wrong_sample_count() {
    // Accel count 99, not 100: the gate fails and the buffer decodes to nothing.
    let payload = gen5_v21_payload(|b| {
        b[13..15].copy_from_slice(&99u16.to_le_bytes());
        b[619..621].copy_from_slice(&100u16.to_le_bytes());
    });
    assert!(decode_record(&manifest(), &payload).unwrap().is_empty());
}

fn gen5_v20_payload(set: impl Fn(&mut [u8])) -> Vec<u8> {
    let mut body = vec![0u8; GEN5_V20_MIN_BODY_LEN];
    set(&mut body);
    [&[0x2F, 20, 0x00][..], &body].concat()
}

#[test]
fn gen5_v20_optical_decodes_6_channels_25_samples() {
    let payload = gen5_v20_payload(|b| {
        b[17..19].copy_from_slice(&1400u16.to_le_bytes()); // green LED
        b[20..22].copy_from_slice(&2800u16.to_le_bytes()); // 2x-green echo anchor
        b[36..40].copy_from_slice(&12345u32.to_le_bytes()); // ch0[0]
        b[40..44].copy_from_slice(&0x000F_FFFBu32.to_le_bytes()); // ch0[1] = -5 (20-bit signed)
    });
    let optical: Vec<_> = decode_record(&manifest(), &payload)
        .unwrap()
        .into_iter()
        .filter(|s| s.kind == StreamKind::OpticalRaw)
        .collect();
    assert_eq!(optical.len(), 150); // 6 channels x 25 samples
    assert_eq!(
        serde_json::to_value(optical[0].value).unwrap(),
        serde_json::json!({ "i32": 12345 })
    );
    // The 20-bit sign extension makes 0x000FFFFB come back as -5, not a large positive.
    assert_eq!(
        serde_json::to_value(optical[1].value).unwrap(),
        serde_json::json!({ "i32": -5 })
    );
}

#[test]
fn gen5_v20_optical_rejects_without_led_anchor() {
    // Echo is not 2x green: the anchor gate fails and the buffer decodes to nothing.
    let payload = gen5_v20_payload(|b| {
        b[17..19].copy_from_slice(&1400u16.to_le_bytes());
        b[20..22].copy_from_slice(&999u16.to_le_bytes());
    });
    assert!(decode_record(&manifest(), &payload).unwrap().is_empty());
}
