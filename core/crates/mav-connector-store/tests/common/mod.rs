#![allow(clippy::expect_used)]

use ed25519_dalek::{Signer, SigningKey};
use mav_connector_abi::*;
use mav_connector_runtime::{
    signature_digest, KeyScope, KeyStatus, PublisherKey, RevocationSet, TrustPolicy,
};
use sha2::{Digest, Sha256};

const KEY_ID: &str = "store-test-key";
const SIGNING_SEED: [u8; 32] = [23; 32];

pub fn signed_artifact(version: &str, state_schema: u32, valid_fixture: bool) -> Vec<u8> {
    signed_artifact_with_capture(version, state_schema, valid_fixture, false)
}

#[allow(dead_code)]
pub fn signed_capture_artifact(version: &str, state_schema: u32) -> Vec<u8> {
    signed_artifact_with_capture(version, state_schema, true, true)
}

/// A connector that commits state while it activates, and whose snapshot is `snapshot`.
///
/// The plain test connector returns an empty batch for everything, so it never commits and never
/// exercises the durable-state path at all. This one answers `Activate` — event sequence 2, the
/// second and last event `ConnectorHost::start` dispatches — with a put and a commit, which is the
/// shape a real connector uses to make a session resumable.
#[allow(dead_code)]
pub fn signed_committing_artifact(
    version: &str,
    state_schema: u32,
    session_id: u64,
    snapshot: &[u8],
) -> Vec<u8> {
    let empty = encode_canonical(&ActionBatch {
        actions: Vec::new(),
    })
    .expect("batch encode");
    let committing = encode_canonical(&ActionBatch {
        actions: vec![
            action(
                session_id,
                1,
                ActionBody::StatePut {
                    key: "session".to_owned(),
                    value: snapshot.to_vec(),
                },
            ),
            action(session_id, 2, ActionBody::StateCommit),
        ],
    })
    .expect("batch encode");
    signed_artifact_with_batches(version, state_schema, snapshot, &empty, &committing)
}

#[allow(dead_code)]
fn action(session_id: u64, operation: u64, body: ActionBody) -> ConnectorAction {
    ConnectorAction {
        connector_id: ConnectorId::new("org.example.store").expect("connector id"),
        session_id: SessionId(session_id),
        caused_by: EventSequence(2),
        cancellation_generation: CancellationGeneration(0),
        operation_id: OperationId(operation),
        deadline_token: TimerToken(operation + 100),
        body,
    }
}

fn signed_artifact_with_capture(
    version: &str,
    state_schema: u32,
    valid_fixture: bool,
    capture: bool,
) -> Vec<u8> {
    let empty = encode_canonical(&ActionBatch {
        actions: Vec::new(),
    })
    .expect("batch encode");
    build_artifact(
        version,
        state_schema,
        &[1, 2, 3],
        &empty,
        &empty,
        valid_fixture,
        capture,
    )
}

#[allow(dead_code)]
fn signed_artifact_with_batches(
    version: &str,
    state_schema: u32,
    snapshot: &[u8],
    init: &[u8],
    handle: &[u8],
) -> Vec<u8> {
    build_artifact(version, state_schema, snapshot, init, handle, true, false)
}

/// One test connector: a snapshot it always returns, one batch for `mav_init` and one for
/// `mav_handle`. Static batches are enough because a host session dispatches exactly two events
/// before a test takes over, and their sequences are therefore known in advance.
fn build_artifact(
    version: &str,
    state_schema: u32,
    state: &[u8],
    init: &[u8],
    handle: &[u8],
    valid_fixture: bool,
    capture: bool,
) -> Vec<u8> {
    let empty = encode_canonical(&ActionBatch {
        actions: Vec::new(),
    })
    .expect("batch encode");
    let init_packed = ((1_024_u64) << 32) | init.len() as u64;
    let handle_packed = ((4_096_u64) << 32) | handle.len() as u64;
    let empty_packed = ((6_144_u64) << 32) | empty.len() as u64;
    let state_packed = ((2_048_u64) << 32) | state.len() as u64;
    // `mav_handle` answers the first event with `handle` and everything after it with an empty
    // batch. A batch names the event sequence that caused it, so a connector that returned the
    // same one twice would be refused the second time — including for the `StateCommitted` the
    // host chains straight back after a commit.
    let wat = format!(
        r#"(module
            (memory (export "memory") 2 100)
            (data (i32.const 1024) "{}")
            (data (i32.const 2048) "{}")
            (data (i32.const 4096) "{}")
            (data (i32.const 6144) "{}")
            (global $answered (mut i32) (i32.const 0))
            (func (export "mav_abi_version") (result i64) i64.const 4294967296)
            (func (export "mav_alloc") (param i32) (result i32) i32.const 8192)
            (func (export "mav_dealloc") (param i32 i32))
            (func (export "mav_init") (param i32 i32) (result i64) i64.const {init_packed})
            (func (export "mav_handle") (param i32 i32) (result i64)
                (local $result i64)
                (if (global.get $answered)
                    (then (local.set $result (i64.const {empty_packed})))
                    (else
                        (global.set $answered (i32.const 1))
                        (local.set $result (i64.const {handle_packed}))))
                (local.get $result))
            (func (export "mav_snapshot") (result i64) i64.const {state_packed})
        )"#,
        wat_bytes(init),
        wat_bytes(state),
        wat_bytes(handle),
        wat_bytes(&empty),
    );
    let mut module = wat::parse_str(wat).expect("valid test WAT");
    let fixtures = fixture_set(if valid_fixture {
        Sha256::digest(state).into()
    } else {
        [0; 32]
    });
    let fixture_bytes = encode_canonical(&fixtures).expect("fixtures encode");
    let mut manifest = manifest(version, state_schema, Sha256::digest(&fixture_bytes).into());
    if capture {
        manifest.schema = MANIFEST_SCHEMA_V2.to_owned();
        manifest.capabilities.push(CapabilityDecl {
            stream: "ecg".to_owned(),
            transport: vec![TransportCapability::Subscribe, TransportCapability::Write],
        });
        manifest.captures = Some(vec![CaptureDecl {
            stream: "ecg".to_owned(),
            unit: "counts".to_owned(),
            minimum_sample_rate_hz: 100,
            maximum_sample_rate_hz: 100,
        }]);
    }
    append_custom(
        &mut module,
        "mav:manifest",
        &encode_canonical(&manifest).expect("manifest encode"),
    );
    append_custom(
        &mut module,
        "mav:abi",
        &encode_canonical(&abi()).expect("ABI encode"),
    );
    append_custom(&mut module, "mav:fixtures", &fixture_bytes);
    let digest = signature_digest([module.as_slice()]);
    let key = SigningKey::from_bytes(&SIGNING_SEED);
    let signature = SignatureRecord {
        schema: SIGNATURE_SCHEMA.to_owned(),
        algorithm: SignatureAlgorithm::Ed25519,
        publisher_key_id: KEY_ID.to_owned(),
        digest,
        signature: key.sign(&digest).to_bytes(),
    };
    append_custom(
        &mut module,
        "mav:signature",
        &encode_canonical(&signature).expect("signature encode"),
    );
    module
}

pub fn trust(revision: u64) -> (TrustPolicy, RevocationSet) {
    let signing_key = SigningKey::from_bytes(&SIGNING_SEED);
    (
        TrustPolicy {
            revision,
            allow_third_party: true,
            allow_development: false,
            keys: vec![PublisherKey {
                id: KEY_ID.to_owned(),
                public_key: signing_key.verifying_key().to_bytes(),
                scope: KeyScope::ThirdParty,
                valid_from_ms: 0,
                valid_until_ms: None,
                status: KeyStatus::Active,
            }],
        },
        RevocationSet {
            revision,
            generated_at_ms: 0,
            valid_until_ms: 1_000_000,
            entries: Vec::new(),
        },
    )
}

fn manifest(version: &str, state_schema: u32, fixture_set_hash: [u8; 32]) -> Manifest {
    Manifest {
        schema: MANIFEST_SCHEMA.to_owned(),
        connector_id: ConnectorId::new("org.example.store").expect("connector id"),
        version: version.to_owned(),
        display_name: "Store Test".to_owned(),
        description: "Durable lifecycle test connector".to_owned(),
        publisher_key_id: KEY_ID.to_owned(),
        abi: AbiRange {
            major: 1,
            min_minor: 0,
            max_minor: 0,
        },
        core: CoreRange {
            min_version: "0.1.0".to_owned(),
            max_version: None,
        },
        state_schema,
        artifact_limits_profile: LimitsProfileId::new("mobile-v1").expect("profile"),
        device_families: vec![DeviceFamily {
            id: "store".to_owned(),
            name_prefixes: vec!["Store".to_owned()],
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
        fixture_set_hash,
        update: UpdatePolicy {
            channel: "stable".to_owned(),
            downgrade: DowngradePolicy::Reject,
        },
    }
}

fn abi() -> AbiDescriptor {
    AbiDescriptor {
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
    }
}

fn fixture_set(expected_state_hash: [u8; 32]) -> FixtureSet {
    FixtureSet {
        schema: FIXTURES_SCHEMA.to_owned(),
        cases: vec![FixtureCase {
            name: "activate".to_owned(),
            initial_state: Vec::new(),
            events: vec![ConnectorEvent {
                connector_id: ConnectorId::new("org.example.store").expect("connector id"),
                session_id: SessionId(1),
                sequence: EventSequence(1),
                cancellation_generation: CancellationGeneration(0),
                wall_time_ms: None,
                body: EventBody::Activate,
            }],
            expected: vec![ActionBatch {
                actions: Vec::new(),
            }],
            expected_state_hash,
            max_fuel: 10_000,
            expected_samples: None,
            expected_diagnostics: None,
        }],
    }
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
