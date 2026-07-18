#![allow(clippy::expect_used, clippy::panic)]

use ed25519_dalek::{Signer, SigningKey};
use mav_connector_abi::*;
use mav_connector_tool::{finalize, prepare, validate, ToolError};
use sha2::{Digest, Sha256};

fn module(exports: bool) -> Vec<u8> {
    if !exports {
        return b"\0asm\x01\0\0\0".to_vec();
    }
    wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "mav_abi_version") (result i64) i64.const 4294967296)
            (func (export "mav_alloc") (param i32) (result i32) i32.const 0)
            (func (export "mav_dealloc") (param i32 i32))
            (func (export "mav_init") (param i32 i32) (result i64) i64.const 0)
            (func (export "mav_handle") (param i32 i32) (result i64) i64.const 0)
            (func (export "mav_snapshot") (result i64) i64.const 0)
        )"#,
    )
    .expect("valid WAT")
}

fn malformed_export_module() -> Vec<u8> {
    wat::parse_str(
        r#"(module
            (memory (export "memory") 1)
            (func (export "mav_abi_version") (result i64) i64.const 4294967296)
            (func (export "mav_alloc") (result i32) i32.const 0)
            (func (export "mav_dealloc") (param i32 i32))
            (func (export "mav_init") (param i32 i32) (result i64) i64.const 0)
            (func (export "mav_handle") (param i32 i32) (result i64) i64.const 0)
            (func (export "mav_snapshot") (result i64) i64.const 0)
        )"#,
    )
    .expect("valid WAT")
}

fn records() -> (Manifest, AbiDescriptor, FixtureSet) {
    let connector_id = ConnectorId::new("org.example.template").expect("connector id");
    let fixtures = FixtureSet {
        schema: FIXTURES_SCHEMA.to_owned(),
        cases: vec![FixtureCase {
            name: "activate".to_owned(),
            initial_state: Vec::new(),
            events: vec![ConnectorEvent {
                connector_id: connector_id.clone(),
                session_id: SessionId(1),
                sequence: EventSequence(1),
                cancellation_generation: CancellationGeneration(0),
                wall_time_ms: None,
                body: EventBody::Activate,
            }],
            expected: vec![ActionBatch {
                actions: Vec::new(),
            }],
            expected_state_hash: Sha256::digest([]).into(),
            max_fuel: 10_000,
            expected_samples: None,
            expected_diagnostics: None,
        }],
    };
    let fixture_bytes = encode_canonical(&fixtures).expect("fixture encoding");
    let manifest = Manifest {
        schema: MANIFEST_SCHEMA.to_owned(),
        connector_id,
        version: "1.0.0".to_owned(),
        display_name: "Template".to_owned(),
        description: "Device-neutral SDK template".to_owned(),
        publisher_key_id: "template-test-key".to_owned(),
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
            id: "template".to_owned(),
            name_prefixes: vec!["Template".to_owned()],
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
        permissions: vec![Permission::Ble],
        entrypoints: Entrypoints::default(),
        fixture_set_hash: Sha256::digest(fixture_bytes).into(),
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
    (manifest, abi, fixtures)
}

#[test]
fn deterministic_pack_inspect_verify_round_trip() {
    let (manifest, abi, fixtures) = records();
    let first = prepare(&module(true), &manifest, &abi, &fixtures).expect("prepare");
    let second = prepare(&module(true), &manifest, &abi, &fixtures).expect("prepare again");
    assert_eq!(first, second);
    let key = SigningKey::from_bytes(&[11; 32]);
    let signature = key.sign(&first.digest).to_bytes();
    let artifact = finalize(first, signature, key.verifying_key().to_bytes()).expect("finalize");
    assert_eq!(validate(&artifact, key.verifying_key().to_bytes()), Ok(()));
}

#[test]
fn malformed_exports_and_oversized_fixtures_reject() {
    let (manifest, abi, mut fixtures) = records();
    assert_eq!(
        prepare(&module(false), &manifest, &abi, &fixtures),
        Err(ToolError::MissingExport("memory".to_owned()))
    );
    assert_eq!(
        prepare(&malformed_export_module(), &manifest, &abi, &fixtures),
        Err(ToolError::InvalidExport("mav_alloc".to_owned()))
    );
    fixtures.cases[0].initial_state = vec![0; MAX_STATE_BYTES + 1];
    assert_eq!(
        prepare(&module(true), &manifest, &abi, &fixtures),
        Err(ToolError::InvalidMetadata("fixtures"))
    );
}
