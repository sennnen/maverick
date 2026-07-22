#![allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]

use ed25519_dalek::{Signer, SigningKey};
use mav_ffi::{
    ConnectorApplyOutcome, ConnectorCancelReason, ConnectorInspection, ConnectorInstallRequest,
    ConnectorKeyScope, ConnectorKeyStatus, ConnectorLifecycleState, ConnectorPublisherKey,
    ConnectorRegistryRoot, ConnectorRemovalMode, ConnectorRevocationRecord, ConnectorSessionConfig,
    ConnectorSourceKind, ConnectorSourceMetadata, ConnectorTransportEvent,
    ConnectorTransportRequest, ConnectorTrustPolicy, ConnectorTrustRevocations,
    InstalledConnectorRecord, MavRuntime, RuntimeConfig,
};
use mav_model::ids::{DeviceId, MetadataId};
use mav_model::raw::RawValue;
use mav_model::stream::{Quality, Sample, StreamKind};
use mav_model::time::{DeviceTime, WallTime};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "../../mav-connector-store/tests/common/mod.rs"]
mod common;

static NEXT_DB: AtomicU64 = AtomicU64::new(1);
const CONNECTOR: &str = "org.example.store";

fn db_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "mav-ffi-connector-{}-{}.sqlite",
        std::process::id(),
        NEXT_DB.fetch_add(1, Ordering::Relaxed)
    ))
}

fn runtime() -> (std::sync::Arc<MavRuntime>, std::path::PathBuf) {
    let path = db_path();
    let _ = std::fs::remove_file(&path);
    let runtime = MavRuntime::new(RuntimeConfig {
        database_path: path.to_string_lossy().into_owned(),
        timezone_id: "Europe/London".to_owned(),
        transport_capacity: 16,
        app_version: "0.1.0".to_owned(),
        app_build: "test".to_owned(),
    })
    .expect("runtime");
    (runtime, path)
}

fn source() -> ConnectorSourceMetadata {
    ConnectorSourceMetadata {
        kind: ConnectorSourceKind::Imported,
        display_name: "Document import".to_owned(),
        locator_digest: Sha256::digest(b"opaque document bookmark").to_vec(),
    }
}

fn trust(revision: u64) -> (ConnectorTrustPolicy, ConnectorTrustRevocations) {
    let (policy, revocations) = common::trust(revision);
    let keys = policy
        .keys
        .into_iter()
        .map(|key| ConnectorPublisherKey {
            id: key.id,
            public_key: key.public_key.to_vec(),
            scope: match key.scope {
                mav_connector_runtime::KeyScope::Official => ConnectorKeyScope::Official,
                mav_connector_runtime::KeyScope::ThirdParty => ConnectorKeyScope::ThirdParty,
                mav_connector_runtime::KeyScope::Development => ConnectorKeyScope::Development,
            },
            valid_from_ms: key.valid_from_ms,
            valid_until_ms: key.valid_until_ms,
            status: match key.status {
                mav_connector_runtime::KeyStatus::Active => ConnectorKeyStatus::Active,
                mav_connector_runtime::KeyStatus::Revoked { .. } => ConnectorKeyStatus::Revoked,
                mav_connector_runtime::KeyStatus::Rotated { .. } => ConnectorKeyStatus::Rotated,
            },
            status_at_ms: None,
            status_detail: None,
        })
        .collect();
    (
        ConnectorTrustPolicy {
            revision: policy.revision,
            allow_third_party: policy.allow_third_party,
            allow_development: policy.allow_development,
            keys,
        },
        ConnectorTrustRevocations {
            revision: revocations.revision,
            generated_at_ms: revocations.generated_at_ms,
            valid_until_ms: revocations.valid_until_ms,
            entries: revocations
                .entries
                .into_iter()
                .map(|entry| ConnectorRevocationRecord {
                    publisher_key_id: entry.publisher_key_id,
                    revoked_at_ms: entry.revoked_at_ms,
                    reason: entry.reason,
                })
                .collect(),
        },
    )
}

fn packaged_trust(public_key_hex: &str) -> (ConnectorTrustPolicy, ConnectorTrustRevocations) {
    let public_key = (0..public_key_hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&public_key_hex[index..index + 2], 16).unwrap())
        .collect();
    (
        ConnectorTrustPolicy {
            revision: 1,
            allow_third_party: false,
            allow_development: true,
            keys: vec![ConnectorPublisherKey {
                id: PUBLISHER_KEY_ID.to_owned(),
                public_key,
                scope: ConnectorKeyScope::Development,
                valid_from_ms: 0,
                valid_until_ms: Some(10_000),
                status: ConnectorKeyStatus::Active,
                status_at_ms: None,
                status_detail: None,
            }],
        },
        ConnectorTrustRevocations {
            revision: 1,
            generated_at_ms: 0,
            valid_until_ms: 10_000,
            entries: Vec::new(),
        },
    )
}

/// The development publisher identity the packaged fixtures are signed under.
const PUBLISHER_KEY_ID: &str = "maverick-whoop-test";
const PUBLISHER_PUBLIC_KEY: &str =
    "dfef1d92a685c9df623b8a321740b0a59de0de538bbfea9ddb703394a1e0f5bd";

fn packaged_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/connectors")
}

/// Every packaged artifact the fixture directory carries, discovered rather than named, so the
/// host never encodes which devices exist.
fn packaged_artifacts() -> Vec<Vec<u8>> {
    let mut names: Vec<_> = std::fs::read_dir(packaged_dir())
        .unwrap()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension()? == "mavconn").then_some(path)
        })
        .collect();
    names.sort();
    assert!(!names.is_empty(), "no packaged artifacts to exercise");
    names
        .into_iter()
        .map(|p| std::fs::read(p).unwrap())
        .collect()
}

#[test]
fn platform_neutral_connector_boundary_types_exist() {
    assert_ne!(ConnectorSourceKind::Bundled, ConnectorSourceKind::Imported);
    assert_ne!(ConnectorKeyScope::Official, ConnectorKeyScope::ThirdParty);
    assert_ne!(ConnectorKeyStatus::Active, ConnectorKeyStatus::Revoked);
    assert_ne!(
        ConnectorRemovalMode::DeleteState,
        ConnectorRemovalMode::QuarantineState
    );
    assert_ne!(ConnectorCancelReason::User, ConnectorCancelReason::Platform);
    assert_ne!(
        ConnectorApplyOutcome::Applied,
        ConnectorApplyOutcome::IgnoredLate
    );
    assert_ne!(
        ConnectorLifecycleState::Installed,
        ConnectorLifecycleState::Failed
    );
    let _ = std::mem::size_of::<ConnectorSourceMetadata>();
    let _ = std::mem::size_of::<ConnectorTrustPolicy>();
    let _ = std::mem::size_of::<ConnectorTrustRevocations>();
    let _ = std::mem::size_of::<ConnectorInspection>();
    let _ = std::mem::size_of::<InstalledConnectorRecord>();
    let _ = std::mem::size_of::<ConnectorTransportEvent>();
    let _ = std::mem::size_of::<ConnectorTransportRequest>();
}

#[test]
fn signed_registry_ingestion_and_artifact_binding_cross_ffi() {
    let (runtime, path) = runtime();
    let root_key = SigningKey::from_bytes(&[71; 32]);
    let artifact = common::signed_artifact("1.0.0", 1, true);
    let index = mav_connector_runtime::RegistryIndex {
        schema: "mavconn-registry-index/v1".to_owned(),
        registry_id: "org.maverick.ffi".to_owned(),
        revision: 1,
        generated_at_ms: 9,
        valid_until_ms: 100,
        previous_index_sha256: None,
        revocation_revision: 1,
        entries: vec![mav_connector_runtime::RegistryEntry {
            connector_id: CONNECTOR.to_owned(),
            version: "1.0.0".to_owned(),
            artifact_sha256: Sha256::digest(&artifact).into(),
            artifact_url: "https://registry.example/ffi.mavconn".to_owned(),
            artifact_size: artifact.len() as u64,
            publisher_key_id: "store-test-key".to_owned(),
            abi: mav_connector_runtime::RegistryAbiRange {
                major: 1,
                min_minor: 0,
                max_minor: 0,
            },
            core: mav_connector_runtime::RegistryCoreRange {
                min_version: "0.1.0".to_owned(),
                max_version: None,
            },
            channel: "stable".to_owned(),
            supersedes: None,
            revoked: false,
        }],
        revocations: Vec::new(),
        rotations: Vec::new(),
    };
    let signing_digest =
        mav_connector_runtime::registry_signing_digest(&index).expect("registry digest");
    let bytes = mav_connector_runtime::encode_signed_registry(
        index,
        "ffi-root-v1".to_owned(),
        root_key.sign(&signing_digest).to_bytes(),
    )
    .expect("registry bytes");
    let (policy, _) = trust(1);
    let snapshot = runtime
        .ingest_connector_registry(
            bytes,
            ConnectorRegistryRoot {
                registry_id: "org.maverick.ffi".to_owned(),
                key_id: "ffi-root-v1".to_owned(),
                public_key: root_key.verifying_key().to_bytes().to_vec(),
            },
            None,
            policy,
            10,
        )
        .expect("verified registry");
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.entries.len(), 1);
    runtime
        .verify_connector_registry_artifact(snapshot.entries[0].clone(), artifact)
        .expect("registry artifact binding");
    let error = runtime
        .verify_connector_registry_artifact(snapshot.entries[0].clone(), b"wrong".to_vec())
        .expect_err("wrong download rejected");
    let mav_ffi::FfiError::Core { code, .. } = error;
    assert_eq!(
        code,
        mav_model::error::codes::CONNECTOR_REGISTRY_ARTIFACT_MISMATCH
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn runtime_starts_empty_and_each_packaged_connector_enables_only_after_install() {
    for bytes in packaged_artifacts() {
        let (runtime, path) = runtime();
        assert!(runtime.list_installed_connectors().unwrap().is_empty());
        let (policy, revocations) = packaged_trust(PUBLISHER_PUBLIC_KEY);
        let connector_id = runtime
            .inspect_connector_bytes(
                bytes.clone(),
                source(),
                policy.clone(),
                revocations.clone(),
                1,
                1_000,
            )
            .unwrap()
            .connector_id;
        let missing = runtime
            .open_connector_session(
                ConnectorSessionConfig {
                    connector_id: connector_id.clone(),
                    session_id: 1,
                    device_id: 1,
                    transport_capacity: 16,
                    now_ms: 1,
                },
                policy.clone(),
                revocations.clone(),
            )
            .expect_err("not linked or installed");
        let mav_ffi::FfiError::Core { code, .. } = missing;
        assert_eq!(code, mav_model::error::codes::CONNECTOR_INSTALL_NOT_FOUND);

        let inspection = runtime
            .inspect_connector_bytes(
                bytes.clone(),
                source(),
                policy.clone(),
                revocations.clone(),
                2,
                1_000,
            )
            .unwrap();
        runtime
            .install_connector_bytes(
                ConnectorInstallRequest {
                    bytes,
                    source: source(),
                    approval_token: inspection.approval_token,
                    activate: true,
                    now_ms: 3,
                },
                policy.clone(),
                revocations.clone(),
            )
            .unwrap();
        runtime
            .open_connector_session(
                ConnectorSessionConfig {
                    connector_id: connector_id.to_owned(),
                    session_id: 1,
                    device_id: 1,
                    transport_capacity: 16,
                    now_ms: 4,
                },
                policy,
                revocations,
            )
            .unwrap();
        assert!(!runtime.drain_connector_actions(16).unwrap().is_empty());
        let _ = std::fs::remove_file(path);
    }
}

/// The host names no device. Everything this test needs to impersonate the hardware — the service
/// to advertise and the name to advertise it under — is read back out of the artifact's own
/// manifest, so the same test drives any packaged connector.
#[test]
fn subscription_actions_carry_real_gatt_addresses() {
    for bytes in packaged_artifacts() {
        drive_one_packaged_connector(bytes);
    }
}

fn drive_one_packaged_connector(bytes: Vec<u8>) {
    let (runtime, path) = runtime();
    let (policy, revocations) = packaged_trust(PUBLISHER_PUBLIC_KEY);
    let inspection = runtime
        .inspect_connector_bytes(
            bytes.clone(),
            source(),
            policy.clone(),
            revocations.clone(),
            1,
            1_000,
        )
        .expect("inspect");
    runtime
        .install_connector_bytes(
            ConnectorInstallRequest {
                bytes,
                source: source(),
                approval_token: inspection.approval_token,
                activate: true,
                now_ms: 2,
            },
            policy.clone(),
            revocations.clone(),
        )
        .expect("install");
    let family = inspection
        .device_families
        .first()
        .expect("manifest declares a device family")
        .clone();
    let advertised_service = family
        .service_uuids
        .first()
        .expect("device family declares a service")
        .clone();
    let advertised_name = family
        .name_prefixes
        .iter()
        .min_by_key(|prefix| prefix.len())
        .expect("device family declares a name prefix")
        .clone();
    let connector_id = inspection.connector_id.clone();
    runtime
        .open_connector_session(
            ConnectorSessionConfig {
                connector_id,
                session_id: 7,
                device_id: 1,
                transport_capacity: 32,
                now_ms: 3,
            },
            policy,
            revocations,
        )
        .expect("open");
    runtime.drain_connector_actions(32).expect("start scan");

    // Drive by what the connector asks for, not by a fixed script: pairing is a generation-specific
    // step and the host must not assume it. Every reply is echoed from the connector's own request.
    let mut pending = vec![
        ConnectorTransportEvent::Connected { mtu: 247 },
        ConnectorTransportEvent::Advertisement {
            address: "device".to_owned(),
            rssi: -30,
            service_uuids: vec![advertised_service.clone()],
            manufacturer_data: Vec::new(),
            name: Some(advertised_name.clone()),
        },
    ];
    let mut subscriptions = Vec::new();
    while let Some(event) = pending.pop() {
        runtime
            .apply_connector_event(event, Some(4))
            .expect("apply transport event");
        for action in runtime.drain_connector_actions(32).expect("drain setup") {
            match action.request {
                ConnectorTransportRequest::EnsurePaired => {
                    pending.push(ConnectorTransportEvent::PairingResult {
                        success: true,
                        error_code: None,
                    });
                }
                ConnectorTransportRequest::DiscoverServices => {
                    pending.push(ConnectorTransportEvent::ServicesDiscovered {
                        service_uuids: vec![advertised_service.clone(), "180d".to_owned()],
                    });
                }
                request @ ConnectorTransportRequest::Subscribe { .. } => {
                    subscriptions.push(request);
                }
                _ => {}
            }
        }
    }

    assert!(
        !subscriptions.is_empty(),
        "connector subscribed to nothing after discovery"
    );
    assert!(subscriptions.iter().all(|request| match request {
        ConnectorTransportRequest::Subscribe {
            service_uuid,
            characteristic_uuid,
            ..
        } => !service_uuid.is_empty() && !characteristic_uuid.is_empty(),
        _ => false,
    }));
    for request in subscriptions {
        let ConnectorTransportRequest::Subscribe {
            characteristic_id, ..
        } = request
        else {
            panic!("expected subscribe action");
        };
        runtime
            .apply_connector_event(
                ConnectorTransportEvent::Subscribed { characteristic_id },
                Some(5),
            )
            .expect("every subscription callback is valid");
    }
    assert_eq!(
        runtime.connector_telemetry().expect("telemetry").lifecycle,
        ConnectorLifecycleState::Configuring
    );
    let mut configured_writes = 0;
    for sequence in 0..16 {
        let actions = runtime
            .drain_connector_actions(32)
            .expect("configuration actions");
        let writes: Vec<_> = actions
            .into_iter()
            .filter_map(|action| match action.request {
                ConnectorTransportRequest::Write {
                    characteristic_id, ..
                } => Some((action.operation_id, characteristic_id)),
                _ => None,
            })
            .collect();
        if writes.is_empty() {
            break;
        }
        for (operation_id, characteristic_id) in writes {
            configured_writes += 1;
            runtime
                .apply_connector_event(
                    ConnectorTransportEvent::WriteResult {
                        operation_id,
                        characteristic_id,
                    },
                    Some(10 + sequence),
                )
                .expect("configuration write callback is valid");
        }
    }
    // How many writes a connector's configuration takes is its own business; the host contract is
    // that every write callback is delivered and the connector then reports Streaming.
    assert!(
        configured_writes >= 1,
        "connector issued no configuration write"
    );
    assert_eq!(
        runtime.connector_telemetry().expect("telemetry").lifecycle,
        ConnectorLifecycleState::Streaming
    );
    runtime
        .cancel_connector_session(ConnectorCancelReason::User, Some(6))
        .expect("user disconnect accepts connector cleanup actions");
    let disconnect_actions = runtime
        .drain_connector_actions(32)
        .expect("disconnect actions");
    assert!(disconnect_actions
        .iter()
        .any(|action| matches!(action.request, ConnectorTransportRequest::Disconnect)));
    runtime
        .apply_connector_event(
            ConnectorTransportEvent::Disconnected { reason_code: 0 },
            Some(7),
        )
        .expect("native disconnect completes lifecycle");
    assert_eq!(
        runtime.connector_telemetry().expect("telemetry").lifecycle,
        ConnectorLifecycleState::Disconnected
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn packaged_connectors_share_one_publisher_identity_and_trust_policy() {
    let (runtime, path) = runtime();
    let (policy, revocations) = packaged_trust(PUBLISHER_PUBLIC_KEY);

    for bytes in packaged_artifacts() {
        let inspection = runtime
            .inspect_connector_bytes(
                bytes.clone(),
                source(),
                policy.clone(),
                revocations.clone(),
                2,
                1_000,
            )
            .expect("shared publisher trust accepts packaged connector");
        runtime
            .install_connector_bytes(
                ConnectorInstallRequest {
                    bytes,
                    source: source(),
                    approval_token: inspection.approval_token,
                    activate: true,
                    now_ms: 3,
                },
                policy.clone(),
                revocations.clone(),
            )
            .expect("packaged connector installs under shared policy");
    }

    assert_eq!(runtime.list_installed_connectors().unwrap().len(), 2);
    let _ = std::fs::remove_file(path);
}

#[test]
fn active_session_exposes_exact_persisted_connector_telemetry() {
    let (runtime, path) = runtime();
    let (policy, revocations) = trust(1);
    let bytes = common::signed_artifact("1.0.0", 1, true);
    let inspection = runtime
        .inspect_connector_bytes(
            bytes.clone(),
            source(),
            policy.clone(),
            revocations.clone(),
            2,
            1_000,
        )
        .expect("inspect connector");
    runtime
        .install_connector_bytes(
            ConnectorInstallRequest {
                bytes,
                source: source(),
                approval_token: inspection.approval_token,
                activate: true,
                now_ms: 3,
            },
            policy.clone(),
            revocations.clone(),
        )
        .expect("install connector");
    runtime
        .open_connector_session(
            ConnectorSessionConfig {
                connector_id: CONNECTOR.to_owned(),
                session_id: 41,
                device_id: 7,
                transport_capacity: 16,
                now_ms: 4,
            },
            policy,
            revocations,
        )
        .expect("open session");

    let store = mav_engine::Store::open(&path).expect("open evidence store");
    for (kind, value, sequence) in [
        (StreamKind::HeartRate, RawValue::U8(73), 1),
        (StreamKind::BatterySoc, RawValue::Converted(82.0), 2),
        (StreamKind::WristState, RawValue::U8(1), 3),
    ] {
        store
            .insert_sample(
                DeviceId::new(7),
                &Sample {
                    kind,
                    device_time: DeviceTime::from_nanos(sequence * 1_000_000),
                    wall_time: Some(WallTime::from_nanos(1_700_000_000_123_000_000)),
                    seq: sequence as u16,
                    value,
                    quality: Quality::exact(),
                    provenance: MetadataId::new(9),
                },
            )
            .expect("persist sample");
    }

    let telemetry = runtime
        .connector_telemetry()
        .expect("read connector telemetry");
    assert_eq!(telemetry.connector_id, CONNECTOR);
    assert_eq!(telemetry.device_id, 7);
    assert_eq!(telemetry.session_id, 41);
    assert_eq!(telemetry.heart_rate_bpm, Some(73));
    assert_eq!(telemetry.battery_percent, Some(82));
    assert_eq!(telemetry.on_wrist, Some(true));
    assert_eq!(telemetry.last_sample_wall_time_ms, Some(1_700_000_000_123));
    let _ = std::fs::remove_file(path);
}

/// The seam the earlier telemetry test could not see: that test wrote `Quality::exact()` samples
/// straight into the store, so it passed while SQI was scoring every non-cardiac kind zero and the
/// FFI was dropping every zero-scored sample. Here the samples go through `score_batch` first, so
/// battery and wrist reach the app only if the scoring stage lets them.
#[test]
fn telemetry_survives_the_quality_stage_it_actually_passes_through() {
    let (runtime, path) = runtime();
    let (policy, revocations) = trust(1);
    let bytes = common::signed_artifact("1.0.0", 1, true);
    let inspection = runtime
        .inspect_connector_bytes(
            bytes.clone(),
            source(),
            policy.clone(),
            revocations.clone(),
            2,
            1_000,
        )
        .expect("inspect connector");
    runtime
        .install_connector_bytes(
            ConnectorInstallRequest {
                bytes,
                source: source(),
                approval_token: inspection.approval_token,
                activate: true,
                now_ms: 3,
            },
            policy.clone(),
            revocations.clone(),
        )
        .expect("install connector");
    runtime
        .open_connector_session(
            ConnectorSessionConfig {
                connector_id: CONNECTOR.to_owned(),
                session_id: 41,
                device_id: 7,
                transport_capacity: 16,
                now_ms: 4,
            },
            policy,
            revocations,
        )
        .expect("open session");

    let batch = mav_model::raw::RawSampleBatch {
        device: DeviceId::new(7),
        samples: vec![
            mav_model::raw::RawSample {
                kind: StreamKind::HeartRate,
                device_time: DeviceTime::from_nanos(1_000_000),
                seq: 1,
                value: RawValue::U8(73),
            },
            mav_model::raw::RawSample {
                kind: StreamKind::BatterySoc,
                device_time: DeviceTime::from_nanos(2_000_000),
                seq: 2,
                value: RawValue::U8(82),
            },
            mav_model::raw::RawSample {
                kind: StreamKind::WristState,
                device_time: DeviceTime::from_nanos(3_000_000),
                seq: 3,
                value: RawValue::U8(1),
            },
        ],
    };
    let scored = mav_sqi::score_batch(&batch, MetadataId::new(9));
    assert_eq!(scored.len(), 3);
    let store = mav_engine::Store::open(&path).expect("open evidence store");
    for mut sample in scored {
        sample.wall_time = Some(WallTime::from_nanos(1_700_000_000_123_000_000));
        store
            .insert_sample(DeviceId::new(7), &sample)
            .expect("persist sample");
    }

    let telemetry = runtime
        .connector_telemetry()
        .expect("read connector telemetry");
    assert_eq!(telemetry.heart_rate_bpm, Some(73));
    assert_eq!(
        telemetry.battery_percent,
        Some(82),
        "a scored battery sample must reach the app"
    );
    assert_eq!(
        telemetry.on_wrist,
        Some(true),
        "a scored wrist sample must reach the app"
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn exact_bytes_inspect_install_list_and_stale_token_errors_round_trip() {
    let (runtime, path) = runtime();
    let bytes = common::signed_artifact("1.0.0", 1, true);
    let (policy, revocations) = trust(1);
    let inspection = runtime
        .inspect_connector_bytes(
            bytes.clone(),
            source(),
            policy.clone(),
            revocations.clone(),
            10,
            1_000,
        )
        .expect("inspect");
    assert_eq!(inspection.permissions, ["Bluetooth device access"]);
    assert!(!inspection.capabilities.is_empty());
    assert_eq!(inspection.connector_id, CONNECTOR);
    assert_eq!(inspection.artifact_digest, Sha256::digest(&bytes).to_vec());
    assert_eq!(inspection.approval_token.len(), 40);
    let installed = runtime
        .install_connector_bytes(
            ConnectorInstallRequest {
                bytes: bytes.clone(),
                source: source(),
                approval_token: inspection.approval_token.clone(),
                activate: true,
                now_ms: 11,
            },
            policy.clone(),
            revocations.clone(),
        )
        .expect("install");
    assert!(installed.active);
    assert_eq!(installed.artifact_digest, inspection.artifact_digest);
    assert_eq!(
        runtime.list_installed_connectors().unwrap(),
        vec![installed]
    );

    let error = runtime
        .install_connector_bytes(
            ConnectorInstallRequest {
                bytes,
                source: source(),
                approval_token: inspection.approval_token,
                activate: true,
                now_ms: 12,
            },
            policy,
            revocations,
        )
        .expect_err("consumed token rejected");
    let mav_ffi::FfiError::Core {
        code,
        category,
        safe_message,
        ..
    } = error;
    assert_eq!(
        code,
        mav_model::error::codes::CONNECTOR_INSTALL_APPROVAL_INVALID
    );
    assert_eq!(category, "connector");
    assert!(!safe_message.contains(path.to_string_lossy().as_ref()));
    let _ = std::fs::remove_file(path);
}

#[test]
fn concurrent_calls_serialize_and_cancellation_is_generation_safe() {
    let (runtime, path) = runtime();
    let bytes = common::signed_artifact("1.0.0", 1, true);
    let (policy, revocations) = trust(1);
    let inspection = runtime
        .inspect_connector_bytes(
            bytes.clone(),
            source(),
            policy.clone(),
            revocations.clone(),
            10,
            1_000,
        )
        .expect("inspect");
    runtime
        .install_connector_bytes(
            ConnectorInstallRequest {
                bytes,
                source: source(),
                approval_token: inspection.approval_token,
                activate: true,
                now_ms: 11,
            },
            policy.clone(),
            revocations.clone(),
        )
        .expect("install");

    let threads: Vec<_> = (0..8)
        .map(|_| {
            let runtime = runtime.clone();
            std::thread::spawn(move || runtime.list_installed_connectors().expect("list").len())
        })
        .collect();
    for thread in threads {
        assert_eq!(thread.join().expect("join"), 1);
    }

    let opened = runtime
        .open_connector_session(
            ConnectorSessionConfig {
                connector_id: CONNECTOR.to_owned(),
                session_id: 7,
                device_id: 9,
                transport_capacity: 16,
                now_ms: 12,
            },
            policy,
            revocations,
        )
        .expect("open session");
    assert_eq!(opened.lifecycle, ConnectorLifecycleState::Selected);
    let cancelled = runtime
        .cancel_connector_session(ConnectorCancelReason::User, Some(13))
        .expect("cancel");
    assert_eq!(cancelled.lifecycle, ConnectorLifecycleState::Disconnected);
    assert_eq!(cancelled.cancellation_generation, 1);
    assert!(runtime.drain_connector_actions(16).unwrap().is_empty());
    assert_eq!(
        runtime
            .apply_connector_event(
                ConnectorTransportEvent::TransportError {
                    operation_id: Some(999),
                    code: 77,
                    safe_message: "late".to_owned(),
                },
                Some(14),
            )
            .expect("late result is typed"),
        ConnectorApplyOutcome::IgnoredLate
    );
    let _ = std::fs::remove_file(path);
}

#[test]
fn management_methods_activate_rollback_remove_and_disable_without_protocol_data() {
    let (runtime, path) = runtime();
    let (policy, revocations) = trust(1);
    for (version, activate, now_ms) in [("1.0.0", true, 10), ("1.1.0", false, 20)] {
        let bytes = common::signed_artifact(version, 1, true);
        let inspection = runtime
            .inspect_connector_bytes(
                bytes.clone(),
                source(),
                policy.clone(),
                revocations.clone(),
                now_ms,
                1_000,
            )
            .expect("inspect");
        runtime
            .install_connector_bytes(
                ConnectorInstallRequest {
                    bytes,
                    source: source(),
                    approval_token: inspection.approval_token,
                    activate,
                    now_ms,
                },
                policy.clone(),
                revocations.clone(),
            )
            .expect("install");
    }
    runtime
        .activate_installed_connector(
            CONNECTOR.to_owned(),
            "1.1.0".to_owned(),
            policy.clone(),
            revocations.clone(),
            21,
        )
        .expect("activate");
    assert!(runtime
        .list_installed_connectors()
        .unwrap()
        .iter()
        .any(|item| item.version == "1.1.0" && item.active));
    runtime
        .rollback_installed_connector(
            CONNECTOR.to_owned(),
            policy.clone(),
            revocations.clone(),
            22,
        )
        .expect("rollback");
    runtime
        .remove_installed_connector(
            CONNECTOR.to_owned(),
            "1.1.0".to_owned(),
            ConnectorRemovalMode::DeleteState,
            policy.clone(),
            revocations.clone(),
            23,
        )
        .expect("remove");

    let mut rotated = policy;
    runtime
        .open_connector_session(
            ConnectorSessionConfig {
                connector_id: CONNECTOR.to_owned(),
                session_id: 8,
                device_id: 10,
                transport_capacity: 16,
                now_ms: 24,
            },
            rotated.clone(),
            revocations.clone(),
        )
        .expect("open active session");
    rotated.keys[0].status = ConnectorKeyStatus::Rotated;
    rotated.keys[0].status_at_ms = Some(24);
    rotated.keys[0].status_detail = Some("replacement-key".to_owned());
    assert_eq!(
        runtime
            .enforce_connector_trust(rotated, revocations, 24)
            .expect("enforce"),
        [CONNECTOR]
    );
    let remaining = runtime.list_installed_connectors().unwrap();
    assert_eq!(remaining.len(), 1);
    assert!(!remaining[0].active);
    assert_eq!(remaining[0].disabled_reason.as_deref(), Some("MAV-11014"));
    let error = runtime
        .connector_lifecycle()
        .expect_err("disabled connector session was retired");
    let mav_ffi::FfiError::Core { code, .. } = error;
    assert_eq!(code, mav_model::error::codes::CONNECTOR_HOST_STATE);
    let _ = std::fs::remove_file(path);
}

#[test]
fn malformed_public_key_maps_to_a_safe_structured_error() {
    let (runtime, path) = runtime();
    let bytes = common::signed_artifact("1.0.0", 1, true);
    let (mut policy, revocations) = trust(1);
    policy.keys[0].public_key = vec![0; 31];
    let error = runtime
        .inspect_connector_bytes(bytes, source(), policy, revocations, 10, 1_000)
        .expect_err("short key rejected");
    let mav_ffi::FfiError::Core {
        code,
        category,
        safe_message,
        ..
    } = error;
    assert_eq!(
        code,
        mav_model::error::codes::CONNECTOR_TRUST_POLICY_INVALID
    );
    assert_eq!(category, "connector");
    assert_eq!(
        safe_message,
        "connector publisher public key must be exactly 32 bytes"
    );
    assert!(!safe_message.contains(path.to_string_lossy().as_ref()));
    let _ = std::fs::remove_file(path);
}
