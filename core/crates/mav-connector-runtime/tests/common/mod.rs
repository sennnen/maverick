#![allow(clippy::expect_used)]

use ed25519_dalek::{Signer, SigningKey};
use mav_connector_abi::*;
use mav_connector_runtime::{signature_digest, Artifact};
use sha2::{Digest, Sha256};

pub fn artifact(module: Vec<u8>) -> Artifact {
    let (mut manifest, abi, fixtures) = records();
    let fixture_bytes = encode_canonical(&fixtures).expect("fixtures encode");
    manifest.fixture_set_hash = Sha256::digest(&fixture_bytes).into();
    let mut module = module;
    append_custom(
        &mut module,
        "mav:manifest",
        &encode_canonical(&manifest).expect("manifest encode"),
    );
    append_custom(
        &mut module,
        "mav:abi",
        &encode_canonical(&abi).expect("ABI encode"),
    );
    append_custom(&mut module, "mav:fixtures", &fixture_bytes);
    let digest = signature_digest([module.as_slice()]);
    let key = SigningKey::from_bytes(&[19; 32]);
    let record = SignatureRecord {
        schema: SIGNATURE_SCHEMA.to_owned(),
        algorithm: SignatureAlgorithm::Ed25519,
        publisher_key_id: "runtime-test-key".to_owned(),
        digest,
        signature: key.sign(&digest).to_bytes(),
    };
    append_custom(
        &mut module,
        "mav:signature",
        &encode_canonical(&record).expect("signature encode"),
    );
    Artifact::inspect(module).expect("artifact inspection")
}

pub fn event() -> ConnectorEvent {
    ConnectorEvent {
        connector_id: ConnectorId::new("org.example.runtime").expect("connector id"),
        session_id: SessionId(7),
        sequence: EventSequence(8),
        cancellation_generation: CancellationGeneration(0),
        wall_time_ms: None,
        body: EventBody::Activate,
    }
}

pub fn module(handle: &str, output: &[u8], state: &[u8]) -> Vec<u8> {
    module_with_snapshot(handle, output, state, None)
}

/// `snapshot_packed` overrides what `mav_snapshot` returns, so a test can exercise the sentinel
/// values the ABI defines: 0 for a legally empty snapshot, -1 for a guest that could not build one.
pub fn module_with_snapshot(
    handle: &str,
    output: &[u8],
    state: &[u8],
    snapshot_packed: Option<i64>,
) -> Vec<u8> {
    let output_packed = ((1_024_u64) << 32) | output.len() as u64;
    let state_packed = match snapshot_packed {
        Some(value) => value as u64,
        None => ((2_048_u64) << 32) | state.len() as u64,
    };
    let wat = format!(
        r#"(module
            (memory (export "memory") 2 100)
            (data (i32.const 1024) "{}")
            (data (i32.const 2048) "{}")
            (func (export "mav_abi_version") (result i64) i64.const 4294967296)
            (func (export "mav_alloc") (param i32) (result i32) i32.const 4096)
            (func (export "mav_dealloc") (param i32 i32))
            (func (export "mav_init") (param i32 i32) (result i64) i64.const {output_packed})
            (func $handle (export "mav_handle") (param i32 i32) (result i64) {handle})
            (func (export "mav_snapshot") (result i64) i64.const {state_packed})
        )"#,
        wat_bytes(output),
        wat_bytes(state),
    );
    wat::parse_str(wat).expect("valid test WAT")
}

pub fn valid_module() -> Vec<u8> {
    let output = encode_canonical(&ActionBatch {
        actions: Vec::new(),
    })
    .expect("batch encode");
    let packed = ((1_024_u64) << 32) | output.len() as u64;
    module(&format!("i64.const {packed}"), &output, &[1, 2, 3])
}

fn wat_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("\\{byte:02x}")).collect()
}

fn append_custom(module: &mut Vec<u8>, name: &str, data: &[u8]) {
    let mut payload = Vec::new();
    push_leb(&mut payload, name.len() as u32);
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);
    module.push(0);
    push_leb(module, payload.len() as u32);
    module.extend_from_slice(&payload);
}

fn push_leb(bytes: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            break;
        }
    }
}

fn records() -> (Manifest, AbiDescriptor, FixtureSet) {
    let connector_id = ConnectorId::new("org.example.runtime").expect("connector id");
    let manifest = Manifest {
        schema: MANIFEST_SCHEMA.to_owned(),
        connector_id: connector_id.clone(),
        version: "1.0.0".to_owned(),
        display_name: "Runtime Test".to_owned(),
        description: "Hostile runtime test connector".to_owned(),
        publisher_key_id: "runtime-test-key".to_owned(),
        abi: AbiRange {
            major: 1,
            min_minor: 0,
            max_minor: 0,
        },
        core: CoreRange {
            min_version: "0.1.0".to_owned(),
            max_version: None,
        },
        state_schema: 1,
        artifact_limits_profile: LimitsProfileId::new("mobile-v1").expect("profile"),
        device_families: vec![DeviceFamily {
            id: "runtime".to_owned(),
            name_prefixes: vec!["Runtime".to_owned()],
            service_uuids: vec!["180d".to_owned()],
            manufacturer_id: None,
            manufacturer_mask: Vec::new(),
            manufacturer_value: Vec::new(),
        }],
        services: vec![ServiceDecl {
            id: "service".to_owned(),
            uuid: "180d".to_owned(),
            characteristics: vec![CharacteristicDecl {
                id: "data".to_owned(),
                uuid: "2a37".to_owned(),
                properties: vec![CharacteristicProperty::Notify],
                sensitive: false,
                confirmed_write_required: false,
            }],
        }],
        capabilities: vec![CapabilityDecl {
            stream: "heart-rate".to_owned(),
            transport: vec![TransportCapability::Subscribe],
        }],
        captures: None,
        permissions: vec![Permission::Ble],
        entrypoints: Entrypoints::default(),
        fixture_set_hash: [0; 32],
        update: UpdatePolicy {
            channel: "stable".to_owned(),
            downgrade: DowngradePolicy::Reject,
        },
    };
    let abi = AbiDescriptor {
        schema: ABI_SCHEMA.to_owned(),
        version: AbiVersion { major: 1, minor: 0 },
        schema_hash: ABI_V1_SCHEMA_HASH,
        required_exports: [
            "memory",
            "mav_abi_version",
            "mav_alloc",
            "mav_dealloc",
            "mav_init",
            "mav_handle",
            "mav_snapshot",
        ]
        .map(str::to_owned)
        .to_vec(),
        required_imports: Vec::new(),
        wasm_features: Vec::new(),
        sdk_version: "0.1.0".to_owned(),
    };
    let fixtures = FixtureSet {
        schema: FIXTURES_SCHEMA.to_owned(),
        cases: vec![FixtureCase {
            name: "activate".to_owned(),
            initial_state: Vec::new(),
            events: vec![event()],
            expected: vec![ActionBatch {
                actions: Vec::new(),
            }],
            expected_state_hash: Sha256::digest([1, 2, 3]).into(),
            max_fuel: 10_000,
            expected_samples: None,
            expected_diagnostics: None,
        }],
    };
    (manifest, abi, fixtures)
}
