#![allow(clippy::expect_used, clippy::unwrap_used)]

mod common;

use common::{signed_artifact, trust};
use mav_connector_runtime::{KeyStatus, Revocation};
use mav_connector_store::{
    ConnectorRepository, ConnectorSource, InstallRequest, RemovalMode, SourceKind, StateNamespace,
    StoredState,
};
use mav_model::error::{codes, MavError};
use sha2::{Digest, Sha256};
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

const CONNECTOR: &str = "org.example.store";
const PUBLISHER: &str = "store-test-key";

fn source(label: &str) -> ConnectorSource {
    ConnectorSource {
        kind: SourceKind::Imported,
        display_name: label.to_owned(),
        locator_digest: Sha256::digest(label.as_bytes()).into(),
    }
}

fn inspect(
    repository: &ConnectorRepository,
    bytes: Vec<u8>,
    source: ConnectorSource,
    revision: u64,
    now_ms: i64,
) -> mav_connector_store::InspectionApproval {
    let (policy, revocations) = trust(revision);
    repository
        .inspect_connector(bytes, source, &policy, &revocations, now_ms, 1_000)
        .expect("inspection succeeds")
}

fn install(
    repository: &mut ConnectorRepository,
    bytes: Vec<u8>,
    label: &str,
    activate: bool,
    now_ms: i64,
) {
    let source = source(label);
    let approval = inspect(repository, bytes.clone(), source.clone(), 1, now_ms);
    let (policy, revocations) = trust(1);
    repository
        .install_connector(
            InstallRequest {
                bytes,
                source,
                approval: approval.approval,
                activate,
            },
            &policy,
            &revocations,
            now_ms,
        )
        .expect("install succeeds");
}

fn state(device: &str, schema: u32, bytes: &[u8], at: i64) -> StoredState {
    StoredState {
        namespace: StateNamespace {
            connector_id: CONNECTOR.to_owned(),
            publisher_key_id: PUBLISHER.to_owned(),
            device_id: device.to_owned(),
            state_schema: schema,
        },
        bytes: bytes.to_vec(),
        digest: Sha256::digest(bytes).into(),
        updated_at_ms: at,
    }
}

#[test]
fn inspection_install_and_restart_preserve_activation_source_and_state() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mav-connector-store-{nonce}.sqlite"));
    {
        let mut repository = ConnectorRepository::open(&path).expect("open repository");
        install(
            &mut repository,
            signed_artifact("1.0.0", 1, true),
            "Imported file",
            true,
            10,
        );
        repository
            .save_state(&state("device-a", 1, b"learned", 11))
            .expect("save state");
    }
    {
        let repository = ConnectorRepository::open(&path).expect("reopen repository");
        let installed = repository.list_connectors().expect("list");
        assert_eq!(installed.len(), 1);
        assert!(installed[0].active);
        assert_eq!(installed[0].source.display_name, "Imported file");
        // The imported file's name and the publisher's name for the connector are different
        // facts, and a list that shows a wearer the connector id is showing them an address.
        assert_eq!(installed[0].display_name, "Store Test");
        let loaded = repository
            .load_state(&state("device-a", 1, b"", 0).namespace)
            .expect("load state")
            .expect("state exists");
        assert_eq!(loaded.bytes, b"learned");
    }
    fs::remove_file(path).expect("remove test database");
}

#[test]
fn approval_binds_bytes_source_policy_revocations_and_expiry() {
    let bytes = signed_artifact("1.0.0", 1, true);
    let original_source = source("original");
    let mut repository = ConnectorRepository::open_in_memory().expect("repository");
    let approval = inspect(&repository, bytes.clone(), original_source.clone(), 1, 10);
    let (policy, revocations) = trust(1);
    let changed_source = source("changed");
    let error = repository
        .install_connector(
            InstallRequest {
                bytes: bytes.clone(),
                source: changed_source,
                approval: approval.approval.clone(),
                activate: true,
            },
            &policy,
            &revocations,
            11,
        )
        .expect_err("source change rejected");
    assert_eq!(error.code, codes::CONNECTOR_INSTALL_APPROVAL_INVALID);

    let (policy2, revocations2) = trust(2);
    let error = repository
        .install_connector(
            InstallRequest {
                bytes: bytes.clone(),
                source: original_source.clone(),
                approval: approval.approval.clone(),
                activate: true,
            },
            &policy2,
            &revocations2,
            11,
        )
        .expect_err("policy revision change rejected");
    assert_eq!(error.code, codes::CONNECTOR_INSTALL_APPROVAL_INVALID);

    let error = repository
        .install_connector(
            InstallRequest {
                bytes,
                source: original_source,
                approval: approval.approval,
                activate: true,
            },
            &policy,
            &revocations,
            1_011,
        )
        .expect_err("expired approval rejected");
    assert_eq!(error.code, codes::CONNECTOR_INSTALL_APPROVAL_INVALID);
    assert!(repository.list_connectors().expect("list").is_empty());
}

#[test]
fn approval_is_one_time_and_cannot_be_replayed() {
    let bytes = signed_artifact("1.0.0", 1, true);
    let source = source("one time");
    let mut repository = ConnectorRepository::open_in_memory().expect("repository");
    let approval = inspect(&repository, bytes.clone(), source.clone(), 1, 10);
    let token = approval.approval;
    let (policy, revocations) = trust(1);
    repository
        .install_connector(
            InstallRequest {
                bytes: bytes.clone(),
                source: source.clone(),
                approval: token.clone(),
                activate: true,
            },
            &policy,
            &revocations,
            10,
        )
        .expect("first use succeeds");
    let error = repository
        .install_connector(
            InstallRequest {
                bytes,
                source,
                approval: token,
                activate: true,
            },
            &policy,
            &revocations,
            11,
        )
        .expect_err("replay rejected");
    assert_eq!(error.code, codes::CONNECTOR_INSTALL_APPROVAL_INVALID);
    assert_eq!(repository.list_connectors().unwrap().len(), 1);
}

#[test]
fn failed_self_test_and_downgrade_leave_active_version_unchanged() {
    let mut repository = ConnectorRepository::open_in_memory().expect("repository");
    install(
        &mut repository,
        signed_artifact("1.0.0", 1, true),
        "v1",
        true,
        10,
    );

    let good_v2 = signed_artifact("2.0.0", 2, true);
    let approval = inspect(&repository, good_v2, source("v2"), 1, 20);
    let (policy, revocations) = trust(1);
    let error = repository
        .install_connector(
            InstallRequest {
                bytes: signed_artifact("2.0.0", 2, false),
                source: source("v2"),
                approval: approval.approval,
                activate: true,
            },
            &policy,
            &revocations,
            20,
        )
        .expect_err("fixture mismatch rejected");
    assert_eq!(error.code, codes::CONNECTOR_RUNTIME_FIXTURE_MISMATCH);
    assert_eq!(repository.list_connectors().expect("list").len(), 1);
    assert!(repository.list_connectors().expect("list")[0].active);

    let verified_v2 = signed_artifact("2.0.0", 2, true);
    let approval = inspect(
        &repository,
        verified_v2.clone(),
        source("verified v2"),
        1,
        25,
    );
    let (mut untrusted_policy, untrusted_revocations) = trust(1);
    untrusted_policy.keys.clear();
    let error = repository
        .install_connector(
            InstallRequest {
                bytes: verified_v2,
                source: source("verified v2"),
                approval: approval.approval,
                activate: true,
            },
            &untrusted_policy,
            &untrusted_revocations,
            25,
        )
        .expect_err("failed verification rejected");
    assert_eq!(error.code, codes::CONNECTOR_TRUST_UNKNOWN_PUBLISHER);
    assert_eq!(repository.list_connectors().expect("list").len(), 1);

    let old = signed_artifact("0.9.0", 1, true);
    let approval = inspect(&repository, old.clone(), source("old"), 1, 30);
    let error = repository
        .install_connector(
            InstallRequest {
                bytes: old,
                source: source("old"),
                approval: approval.approval,
                activate: true,
            },
            &policy,
            &revocations,
            30,
        )
        .expect_err("downgrade rejected");
    assert_eq!(error.code, codes::CONNECTOR_INSTALL_DOWNGRADE);
    assert_eq!(repository.list_connectors().expect("list").len(), 1);
}

#[test]
fn activation_that_skips_required_state_migration_is_fully_rolled_back() {
    let mut repository = ConnectorRepository::open_in_memory().expect("repository");
    install(
        &mut repository,
        signed_artifact("1.0.0", 1, true),
        "v1",
        true,
        10,
    );
    repository
        .save_state(&state("device-a", 1, b"before", 11))
        .expect("save state");
    let bytes = signed_artifact("2.0.0", 2, true);
    let approval = inspect(&repository, bytes.clone(), source("v2"), 1, 20);
    let (policy, revocations) = trust(1);
    let error = repository
        .install_connector(
            InstallRequest {
                bytes,
                source: source("v2"),
                approval: approval.approval,
                activate: true,
            },
            &policy,
            &revocations,
            20,
        )
        .expect_err("migration-skipping activation rejected");
    assert_eq!(error.code, codes::CONNECTOR_INSTALL_MIGRATION);
    let installed = repository.list_connectors().expect("list");
    assert_eq!(
        installed.len(),
        1,
        "failed install transaction left no v2 rows"
    );
    assert!(installed[0].active);
    assert_eq!(
        repository
            .load_state(&state("device-a", 1, b"", 0).namespace)
            .unwrap()
            .unwrap()
            .bytes,
        b"before"
    );
}

#[test]
fn migration_is_atomic_and_rollback_restores_exact_state() {
    let mut repository = ConnectorRepository::open_in_memory().expect("repository");
    install(
        &mut repository,
        signed_artifact("1.0.0", 1, true),
        "v1",
        true,
        10,
    );
    repository
        .save_state(&state("device-a", 1, b"before", 11))
        .expect("save state");
    install(
        &mut repository,
        signed_artifact("2.0.0", 2, true),
        "v2",
        false,
        20,
    );
    let (policy, revocations) = trust(1);

    let error = repository
        .migrate_and_activate(
            CONNECTOR,
            "2.0.0",
            &policy,
            &revocations,
            21,
            |_old, _schema| {
                Err(MavError::new(
                    codes::CONNECTOR_INSTALL_MIGRATION,
                    "injected",
                ))
            },
        )
        .expect_err("failed migration rejected");
    assert_eq!(error.code, codes::CONNECTOR_INSTALL_MIGRATION);
    assert_eq!(
        repository
            .load_state(&state("device-a", 1, b"", 0).namespace)
            .unwrap()
            .unwrap()
            .bytes,
        b"before"
    );
    assert!(repository
        .list_connectors()
        .unwrap()
        .iter()
        .any(|item| item.version == "1.0.0" && item.active));

    repository
        .migrate_and_activate(
            CONNECTOR,
            "2.0.0",
            &policy,
            &revocations,
            22,
            |old, schema| {
                assert_eq!(schema, 2);
                let mut bytes = old.bytes.clone();
                bytes.extend_from_slice(b"-after");
                Ok(bytes)
            },
        )
        .expect("migration succeeds");
    assert_eq!(
        repository
            .load_state(&state("device-a", 2, b"", 0).namespace)
            .unwrap()
            .unwrap()
            .bytes,
        b"before-after"
    );
    assert!(repository
        .list_connectors()
        .unwrap()
        .iter()
        .any(|item| item.version == "2.0.0" && item.active));

    repository
        .rollback_connector(CONNECTOR, &policy, &revocations, 23)
        .expect("rollback");
    assert_eq!(
        repository
            .load_state(&state("device-a", 1, b"", 0).namespace)
            .unwrap()
            .unwrap()
            .bytes,
        b"before"
    );
    assert!(repository
        .load_state(&state("device-a", 2, b"", 0).namespace)
        .unwrap()
        .is_none());
    assert!(repository
        .list_connectors()
        .unwrap()
        .iter()
        .any(|item| item.version == "1.0.0" && item.active));
}

#[test]
fn namespaces_do_not_cross_and_removal_deletes_or_quarantines() {
    let mut repository = ConnectorRepository::open_in_memory().expect("repository");
    let (policy, revocations) = trust(1);
    install(
        &mut repository,
        signed_artifact("1.0.0", 1, true),
        "v1",
        true,
        10,
    );
    repository
        .save_state(&state("device-a", 1, b"a", 11))
        .expect("save a");
    repository
        .save_state(&state("device-b", 1, b"b", 12))
        .expect("save b");
    assert_eq!(
        repository
            .load_state(&state("device-a", 1, b"", 0).namespace)
            .unwrap()
            .unwrap()
            .bytes,
        b"a"
    );
    assert!(repository
        .load_state(&StateNamespace {
            connector_id: CONNECTOR.to_owned(),
            publisher_key_id: "other-publisher".to_owned(),
            device_id: "device-a".to_owned(),
            state_schema: 1,
        })
        .unwrap()
        .is_none());
    repository
        .remove_connector(
            CONNECTOR,
            "1.0.0",
            RemovalMode::QuarantineState,
            &policy,
            &revocations,
            20,
        )
        .expect("remove");
    assert!(repository.list_connectors().unwrap().is_empty());
    assert!(repository
        .load_state(&state("device-a", 1, b"", 0).namespace)
        .unwrap()
        .is_none());

    install(
        &mut repository,
        signed_artifact("1.0.0", 1, true),
        "v1 again",
        true,
        30,
    );
    repository
        .save_state(&state("device-a", 1, b"new", 31))
        .expect("save new");
    repository
        .remove_connector(
            CONNECTOR,
            "1.0.0",
            RemovalMode::DeleteState,
            &policy,
            &revocations,
            32,
        )
        .expect("delete");
    assert!(repository
        .load_state(&state("device-a", 1, b"", 0).namespace)
        .unwrap()
        .is_none());
}

#[test]
fn removing_active_update_restores_previous_activation_and_state() {
    let mut repository = ConnectorRepository::open_in_memory().expect("repository");
    install(
        &mut repository,
        signed_artifact("1.0.0", 1, true),
        "v1",
        true,
        10,
    );
    repository
        .save_state(&state("device-a", 1, b"before", 11))
        .expect("save state");
    install(
        &mut repository,
        signed_artifact("2.0.0", 2, true),
        "v2",
        false,
        20,
    );
    let (policy, revocations) = trust(1);
    repository
        .migrate_and_activate(CONNECTOR, "2.0.0", &policy, &revocations, 21, |old, _| {
            let mut bytes = old.bytes.clone();
            bytes.extend_from_slice(b"-after");
            Ok(bytes)
        })
        .expect("migrate");
    repository
        .remove_connector(
            CONNECTOR,
            "2.0.0",
            RemovalMode::DeleteState,
            &policy,
            &revocations,
            22,
        )
        .expect("remove active update");
    let installed = repository.list_connectors().expect("list");
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].version, "1.0.0");
    assert!(installed[0].active);
    assert_eq!(
        repository
            .load_state(&state("device-a", 1, b"", 0).namespace)
            .unwrap()
            .unwrap()
            .bytes,
        b"before"
    );
}

#[test]
fn same_schema_activation_still_snapshots_state_for_rollback() {
    let mut repository = ConnectorRepository::open_in_memory().expect("repository");
    let (policy, revocations) = trust(1);
    install(
        &mut repository,
        signed_artifact("1.0.0", 1, true),
        "v1",
        true,
        10,
    );
    repository
        .save_state(&state("device-a", 1, b"v1-state", 11))
        .expect("save v1 state");
    install(
        &mut repository,
        signed_artifact("1.1.0", 1, true),
        "v1.1",
        true,
        20,
    );
    repository
        .save_state(&state("device-a", 1, b"v1.1-state", 21))
        .expect("save v1.1 state");
    let (mut rotated_policy, rotated_revocations) = trust(2);
    rotated_policy.keys[0].status = KeyStatus::Rotated {
        at_ms: 22,
        replacement_id: "next-key".to_owned(),
    };
    let error = repository
        .rollback_connector(CONNECTOR, &rotated_policy, &rotated_revocations, 22)
        .expect_err("untrusted rollback target rejected");
    assert_eq!(error.code, codes::CONNECTOR_TRUST_KEY_ROTATED);
    assert_eq!(
        repository
            .load_state(&state("device-a", 1, b"", 0).namespace)
            .unwrap()
            .unwrap()
            .bytes,
        b"v1.1-state"
    );
    repository
        .rollback_connector(CONNECTOR, &policy, &revocations, 23)
        .expect("rollback");
    assert_eq!(
        repository
            .load_state(&state("device-a", 1, b"", 0).namespace)
            .unwrap()
            .unwrap()
            .bytes,
        b"v1-state"
    );
    assert!(repository
        .list_connectors()
        .unwrap()
        .iter()
        .any(|item| item.version == "1.0.0" && item.active));
}

#[test]
fn key_rotation_and_revocation_disable_active_connector() {
    let mut repository = ConnectorRepository::open_in_memory().expect("repository");
    install(
        &mut repository,
        signed_artifact("1.0.0", 1, true),
        "v1",
        true,
        10,
    );
    let (mut policy, revocations) = trust(2);
    policy.keys[0].status = KeyStatus::Rotated {
        at_ms: 20,
        replacement_id: "next-key".to_owned(),
    };
    assert_eq!(
        repository
            .enforce_policy(&policy, &revocations, 20)
            .expect("enforce"),
        [CONNECTOR]
    );
    let installed = repository.list_connectors().expect("list");
    assert!(!installed[0].active);
    assert_eq!(installed[0].disabled_reason.as_deref(), Some("MAV-11014"));

    let mut repository = ConnectorRepository::open_in_memory().expect("repository");
    install(
        &mut repository,
        signed_artifact("1.0.0", 1, true),
        "v1",
        true,
        10,
    );
    let (policy, mut revocations) = trust(3);
    revocations.entries.push(Revocation {
        publisher_key_id: PUBLISHER.to_owned(),
        revoked_at_ms: 20,
        reason: "compromised".to_owned(),
    });
    assert_eq!(
        repository
            .enforce_policy(&policy, &revocations, 20)
            .expect("enforce"),
        [CONNECTOR]
    );
    assert!(!repository.list_connectors().unwrap()[0].active);
}

/// A store that recorded the display-name column but never filled it must still recover the names.
/// Adding a column and populating it are two pieces of work, and a database that already stamped
/// itself at the version that only did the first would otherwise never see the second.
#[test]
fn display_names_are_recovered_for_artifacts_installed_before_the_column() {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mav-connector-backfill-{nonce}.sqlite"));
    {
        let mut repository = ConnectorRepository::open(&path).expect("open repository");
        install(
            &mut repository,
            signed_artifact("1.0.0", 1, true),
            "Imported file",
            true,
            10,
        );
    }
    {
        // Rewind to the state the column-only migration left behind.
        let connection = rusqlite::Connection::open(&path).expect("open raw");
        connection
            .execute_batch(
                "UPDATE connector_artifact SET display_name = NULL;
                 UPDATE connector_store_meta SET value = 2 WHERE key = 'schema_version';",
            )
            .expect("rewind");
    }
    {
        let repository = ConnectorRepository::open(&path).expect("reopen repository");
        assert_eq!(
            repository.list_connectors().expect("list")[0].display_name,
            "Store Test"
        );
    }
    fs::remove_file(path).expect("remove test database");
}
