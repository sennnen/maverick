#![allow(clippy::expect_used)]

use ed25519_dalek::{Signer, SigningKey};
use mav_connector_runtime::{
    encode_signed_registry, ingest_registry, registry_rotation_digest, registry_signing_digest,
    restore_registry, KeyScope, KeyStatus, PublisherKey, RegistryAbiRange, RegistryCoreRange,
    RegistryEntry, RegistryIndex, RegistryRevocation, RegistryRoot, RegistryRotation, TrustPolicy,
};
use mav_model::error::codes;
use sha2::{Digest, Sha256};

mod common;

const NOW: i64 = 1_000_000;

fn root() -> (RegistryRoot, SigningKey) {
    let key = SigningKey::from_bytes(&[41; 32]);
    (
        RegistryRoot {
            registry_id: "org.maverick.test".to_owned(),
            key_id: "registry-root-v1".to_owned(),
            public_key: key.verifying_key().to_bytes(),
        },
        key,
    )
}

fn publisher(seed: u8, id: &str) -> (PublisherKey, SigningKey) {
    let key = SigningKey::from_bytes(&[seed; 32]);
    (
        PublisherKey {
            id: id.to_owned(),
            public_key: key.verifying_key().to_bytes(),
            scope: KeyScope::ThirdParty,
            valid_from_ms: 0,
            valid_until_ms: None,
            status: KeyStatus::Active,
        },
        key,
    )
}

fn entry(version: &str, channel: &str, bytes: &[u8]) -> RegistryEntry {
    RegistryEntry {
        connector_id: "org.example.device".to_owned(),
        version: version.to_owned(),
        artifact_sha256: Sha256::digest(bytes).into(),
        artifact_url: format!("https://registry.example/{version}.mavconn"),
        artifact_size: bytes.len() as u64,
        publisher_key_id: "publisher-v1".to_owned(),
        abi: RegistryAbiRange {
            major: 1,
            min_minor: 0,
            max_minor: 0,
        },
        core: RegistryCoreRange {
            min_version: "0.1.0".to_owned(),
            max_version: None,
        },
        channel: channel.to_owned(),
        supersedes: None,
        revoked: false,
    }
}

fn index(revision: u64, previous: Option<[u8; 32]>) -> RegistryIndex {
    RegistryIndex {
        schema: "mavconn-registry-index/v1".to_owned(),
        registry_id: "org.maverick.test".to_owned(),
        revision,
        generated_at_ms: NOW - 1_000,
        valid_until_ms: NOW + 60_000,
        previous_index_sha256: previous,
        revocation_revision: revision,
        entries: vec![entry("1.0.0", "stable", b"artifact-v1")],
        revocations: Vec::new(),
        rotations: Vec::new(),
    }
}

fn signed(index: RegistryIndex, key: &SigningKey) -> Vec<u8> {
    let digest = registry_signing_digest(&index).expect("signing digest");
    encode_signed_registry(
        index,
        "registry-root-v1".to_owned(),
        key.sign(&digest).to_bytes(),
    )
    .expect("signed registry")
}

fn policy(key: PublisherKey) -> TrustPolicy {
    TrustPolicy {
        revision: 7,
        allow_third_party: true,
        allow_development: false,
        keys: vec![key],
    }
}

#[test]
fn deterministic_index_vector_is_byte_identical() {
    let (_, key) = root();
    let first = signed(index(1, None), &key);
    let second = signed(index(1, None), &key);
    assert_eq!(first, second);
    assert_eq!(Sha256::digest(first), Sha256::digest(second));
}

#[test]
fn exact_signed_bytes_restore_from_offline_checkpoint() {
    let (root, key) = root();
    let (publisher, _) = publisher(51, "publisher-v1");
    let bytes = signed(index(1, None), &key);
    let first = ingest_registry(&bytes, &root, None, &policy(publisher.clone()), NOW)
        .expect("online refresh");
    let restored = restore_registry(
        &bytes,
        &root,
        &first.checkpoint(),
        &policy(publisher),
        NOW + 1,
    )
    .expect("offline restore");
    assert_eq!(restored.digest, first.digest);
    assert_eq!(restored.revocations, first.revocations);
}

#[test]
fn compromised_index_cannot_replace_a_publisher_key() {
    let (root, root_key) = root();
    let (publisher, _) = publisher(51, "publisher-v1");
    let before = publisher.public_key;
    let snapshot = ingest_registry(
        &signed(index(1, None), &root_key),
        &root,
        None,
        &policy(publisher),
        NOW,
    )
    .expect("valid discovery index");
    assert_eq!(snapshot.trust.keys[0].public_key, before);
    assert_eq!(snapshot.trust.keys.len(), 1);
}

#[test]
fn rollback_replay_and_frozen_indexes_fail_closed() {
    let (root, key) = root();
    let (publisher, _) = publisher(51, "publisher-v1");
    let first = ingest_registry(
        &signed(index(1, None), &key),
        &root,
        None,
        &policy(publisher.clone()),
        NOW,
    )
    .expect("first index");
    let replay = ingest_registry(
        &signed(index(1, None), &key),
        &root,
        Some(&first.checkpoint()),
        &policy(publisher.clone()),
        NOW,
    )
    .expect_err("same revision is replay");
    assert_eq!(replay.code, codes::CONNECTOR_REGISTRY_ROLLBACK);

    let rollback = ingest_registry(
        &signed(index(0, Some(first.digest)), &key),
        &root,
        Some(&first.checkpoint()),
        &policy(publisher.clone()),
        NOW,
    )
    .expect_err("lower revision is rollback");
    assert_eq!(rollback.code, codes::CONNECTOR_REGISTRY_ROLLBACK);

    let mut frozen = index(2, Some(first.digest));
    frozen.valid_until_ms = NOW - 1;
    let stale = ingest_registry(
        &signed(frozen, &key),
        &root,
        Some(&first.checkpoint()),
        &policy(publisher),
        NOW,
    )
    .expect_err("expired index is stale");
    assert_eq!(stale.code, codes::CONNECTOR_REGISTRY_STALE);
}

#[test]
fn predecessor_digest_must_link_the_index_chain() {
    let (root, key) = root();
    let (publisher, _) = publisher(51, "publisher-v1");
    let first = ingest_registry(
        &signed(index(1, None), &key),
        &root,
        None,
        &policy(publisher.clone()),
        NOW,
    )
    .expect("first index");
    let error = ingest_registry(
        &signed(index(2, Some([9; 32])), &key),
        &root,
        Some(&first.checkpoint()),
        &policy(publisher),
        NOW,
    )
    .expect_err("forked predecessor rejected");
    assert_eq!(error.code, codes::CONNECTOR_REGISTRY_CHAIN_INVALID);
}

#[test]
fn rotation_requires_the_old_publisher_cross_signature_and_caches_revocation() {
    let (root, root_key) = root();
    let (old, old_key) = publisher(51, "publisher-v1");
    let (new, _) = publisher(52, "publisher-v2");
    let mut rotation = RegistryRotation {
        from_key_id: old.id.clone(),
        to_key_id: new.id.clone(),
        to_public_key: new.public_key,
        effective_at_ms: NOW,
        cross_signature: [0; 64],
    };
    rotation.cross_signature = old_key
        .sign(&registry_rotation_digest(&rotation))
        .to_bytes();
    let mut next = index(1, None);
    next.rotations.push(rotation.clone());
    next.revocations.push(RegistryRevocation {
        publisher_key_id: "publisher-v0".to_owned(),
        revoked_at_ms: NOW - 10,
        reason: "compromised".to_owned(),
    });
    let snapshot = ingest_registry(&signed(next, &root_key), &root, None, &policy(old), NOW)
        .expect("cross-signed rotation");
    assert_eq!(snapshot.trust.keys.len(), 2);
    assert!(matches!(
        snapshot.trust.keys[0].status,
        KeyStatus::Rotated { .. }
    ));
    assert_eq!(snapshot.revocations.entries[0].reason, "compromised");
    assert_eq!(snapshot.checkpoint().revocation_revision, 1);

    let mut refreshed = index(2, Some(snapshot.digest));
    refreshed.revocation_revision = 2;
    refreshed.revocations = snapshot.index.revocations.clone();
    refreshed.rotations.push(rotation);
    let repeated = ingest_registry(
        &signed(refreshed, &root_key),
        &root,
        Some(&snapshot.checkpoint()),
        &snapshot.trust,
        NOW,
    )
    .expect("cumulative rotation is idempotent");
    assert_eq!(repeated.trust.keys.len(), 2);
}

#[test]
fn unsigned_rotation_is_rejected_even_under_a_valid_registry_signature() {
    let (root, root_key) = root();
    let (old, _) = publisher(51, "publisher-v1");
    let (new, _) = publisher(52, "publisher-v2");
    let mut next = index(1, None);
    next.rotations.push(RegistryRotation {
        from_key_id: old.id.clone(),
        to_key_id: new.id,
        to_public_key: new.public_key,
        effective_at_ms: NOW,
        cross_signature: [7; 64],
    });
    let error = ingest_registry(&signed(next, &root_key), &root, None, &policy(old), NOW)
        .expect_err("registry root cannot authorize publisher rotation");
    assert_eq!(error.code, codes::CONNECTOR_REGISTRY_ROTATION_INVALID);
}

#[test]
fn downloaded_artifact_must_match_registry_digest_and_size() {
    let artifact = common::artifact(common::valid_module());
    let bytes = artifact.bytes();
    let expected = RegistryEntry {
        connector_id: "org.example.runtime".to_owned(),
        version: "1.0.0".to_owned(),
        artifact_sha256: Sha256::digest(bytes).into(),
        artifact_url: "https://registry.example/runtime.mavconn".to_owned(),
        artifact_size: bytes.len() as u64,
        publisher_key_id: "runtime-test-key".to_owned(),
        abi: RegistryAbiRange {
            major: 1,
            min_minor: 0,
            max_minor: 0,
        },
        core: RegistryCoreRange {
            min_version: "0.1.0".to_owned(),
            max_version: None,
        },
        channel: "stable".to_owned(),
        supersedes: None,
        revoked: false,
    };
    assert_eq!(expected.verify_artifact(bytes), Ok(()));
    let error = expected
        .verify_artifact(b"artifact-v2")
        .expect_err("digest mismatch");
    assert_eq!(error.code, codes::CONNECTOR_REGISTRY_ARTIFACT_MISMATCH);
}

#[test]
fn update_selection_enforces_channel_and_downgrade_policy() {
    let (root, key) = root();
    let (publisher, _) = publisher(51, "publisher-v1");
    let mut catalog = index(1, None);
    catalog.entries = vec![
        entry("1.0.0", "stable", b"one"),
        entry("1.2.0", "stable", b"two"),
        entry("2.0.0", "beta", b"three"),
    ];
    let snapshot = ingest_registry(&signed(catalog, &key), &root, None, &policy(publisher), NOW)
        .expect("catalog");
    assert_eq!(
        snapshot
            .select_update("org.example.device", "1.0.0", "stable", false)
            .expect("selection")
            .expect("new version")
            .version,
        "1.2.0"
    );
    assert!(snapshot
        .select_update("org.example.device", "2.0.0", "stable", false)
        .expect("no downgrade")
        .is_none());
    assert_eq!(
        snapshot
            .select_update("org.example.device", "2.0.0", "stable", true)
            .expect("explicit downgrade")
            .expect("stable target")
            .version,
        "1.2.0"
    );
}
