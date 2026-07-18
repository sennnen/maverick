#![allow(clippy::expect_used, clippy::unwrap_used)]

use mav_ffi::{
    ConnectorApplyOutcome, ConnectorCancelReason, ConnectorInspection, ConnectorInstallRequest,
    ConnectorKeyScope, ConnectorKeyStatus, ConnectorLifecycleState, ConnectorPublisherKey,
    ConnectorRemovalMode, ConnectorRevocationRecord, ConnectorSessionConfig, ConnectorSourceKind,
    ConnectorSourceMetadata, ConnectorTransportEvent, ConnectorTransportRequest,
    ConnectorTrustPolicy, ConnectorTrustRevocations, InstalledConnectorRecord, MavRuntime,
    RuntimeConfig,
};
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
