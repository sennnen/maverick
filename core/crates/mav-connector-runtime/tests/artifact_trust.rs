#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use ed25519_dalek::{Signer, SigningKey};
use mav_connector_abi::*;
use mav_connector_runtime::{
    Artifact, KeyScope, KeyStatus, PublisherKey, Revocation, RevocationSet, TrustPolicy,
};
use mav_model::error::codes;
use sha2::Digest;

const SIGNING_SEED: [u8; 32] = [7; 32];

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

fn valid_records() -> (Manifest, AbiDescriptor, FixtureSet) {
    let connector_id = ConnectorId::new("org.example.band").expect("connector id");
    let manifest = Manifest {
        schema: MANIFEST_SCHEMA.to_owned(),
        connector_id: connector_id.clone(),
        version: "1.0.0".to_owned(),
        display_name: "Example Band".to_owned(),
        description: "Test connector".to_owned(),
        publisher_key_id: "publisher-1".to_owned(),
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
            id: "example".to_owned(),
            name_prefixes: vec!["Band".to_owned()],
            service_uuids: vec!["180d".to_owned()],
            manufacturer_id: None,
            manufacturer_mask: Vec::new(),
            manufacturer_value: Vec::new(),
        }],
        services: vec![ServiceDecl {
            id: "health".to_owned(),
            uuid: "180d".to_owned(),
            characteristics: vec![CharacteristicDecl {
                id: "data".to_owned(),
                uuid: "2a37".to_owned(),
                properties: vec![CharacteristicProperty::Notify],
                sensitive: true,
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
            events: vec![ConnectorEvent {
                connector_id,
                session_id: SessionId(1),
                sequence: EventSequence(1),
                cancellation_generation: CancellationGeneration(0),
                wall_time_ms: None,
                body: EventBody::Activate,
            }],
            expected: vec![ActionBatch {
                actions: Vec::new(),
            }],
            expected_state_hash: [0; 32],
            max_fuel: 1_000,
            expected_samples: None,
            expected_diagnostics: None,
        }],
    };
    (manifest, abi, fixtures)
}

fn unsigned_module() -> Vec<u8> {
    let (mut manifest, abi, fixtures) = valid_records();
    let fixture_bytes = encode_canonical(&fixtures).expect("fixtures encode");
    manifest.fixture_set_hash = sha2::Sha256::digest(&fixture_bytes).into();
    let mut module = b"\0asm\x01\0\0\0".to_vec();
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
    module
}

fn signed_module() -> (Vec<u8>, SigningKey) {
    let signing_key = SigningKey::from_bytes(&SIGNING_SEED);
    let mut module = unsigned_module();
    let digest = mav_connector_runtime::signature_digest([module.as_slice()]);
    let signature = signing_key.sign(&digest).to_bytes();
    let record = SignatureRecord {
        schema: SIGNATURE_SCHEMA.to_owned(),
        algorithm: SignatureAlgorithm::Ed25519,
        publisher_key_id: "publisher-1".to_owned(),
        digest,
        signature,
    };
    append_custom(
        &mut module,
        "mav:signature",
        &encode_canonical(&record).expect("signature record encode"),
    );
    (module, signing_key)
}

fn publisher(signing_key: &SigningKey, status: KeyStatus) -> PublisherKey {
    PublisherKey {
        id: "publisher-1".to_owned(),
        public_key: signing_key.verifying_key().to_bytes(),
        scope: KeyScope::ThirdParty,
        valid_from_ms: 100,
        valid_until_ms: Some(1_000),
        status,
    }
}

fn policy(key: PublisherKey) -> TrustPolicy {
    TrustPolicy {
        revision: 1,
        allow_third_party: true,
        allow_development: false,
        keys: vec![key],
    }
}

fn empty_revocations() -> RevocationSet {
    RevocationSet {
        revision: 1,
        generated_at_ms: 100,
        valid_until_ms: 1_000,
        entries: Vec::new(),
    }
}

#[test]
fn inspect_and_verify_resolve_without_instantiating() {
    let (bytes, signing_key) = signed_module();
    let artifact = Artifact::inspect(bytes.clone()).expect("artifact inspects");
    let expected_artifact_digest: [u8; 32] = sha2::Sha256::digest(&bytes).into();
    assert_eq!(
        artifact.report().manifest.connector_id.as_str(),
        "org.example.band"
    );
    assert_eq!(artifact.report().artifact_digest, expected_artifact_digest);
    assert_eq!(
        artifact.verify(
            &policy(publisher(&signing_key, KeyStatus::Active)),
            &empty_revocations(),
            500
        ),
        Ok(())
    );
    let reconstructed: Vec<u8> = artifact
        .canonical_unsigned_chunks()
        .flatten()
        .copied()
        .collect();
    assert_eq!(reconstructed, unsigned_module());
}

#[test]
fn malformed_duplicate_unknown_critical_and_noncanonical_sections_are_typed() {
    let error = Artifact::inspect(vec![0, 1, 2]).expect_err("truncated module accepted");
    assert_eq!(error.code, codes::CONNECTOR_ARTIFACT_MALFORMED_WASM);

    let (bytes, _) = signed_module();
    let mut duplicate = unsigned_module();
    let (_, abi, _) = valid_records();
    append_custom(
        &mut duplicate,
        "mav:abi",
        &encode_canonical(&abi).expect("ABI encode"),
    );
    assert_eq!(
        Artifact::inspect(duplicate)
            .expect_err("duplicate accepted")
            .code,
        codes::CONNECTOR_ARTIFACT_SECTION_DUPLICATE
    );

    let mut duplicate_optional = b"\0asm\x01\0\0\0".to_vec();
    append_custom(&mut duplicate_optional, "mav:optional", &[]);
    append_custom(&mut duplicate_optional, "mav:optional", &[]);
    assert_eq!(
        Artifact::inspect(duplicate_optional)
            .expect_err("duplicate optional mav section accepted")
            .code,
        codes::CONNECTOR_ARTIFACT_SECTION_DUPLICATE
    );

    let mut critical = b"\0asm\x01\0\0\0".to_vec();
    append_custom(&mut critical, "mav:critical:future", &[]);
    assert_eq!(
        Artifact::inspect(critical)
            .expect_err("unknown critical accepted")
            .code,
        codes::CONNECTOR_ARTIFACT_UNKNOWN_CRITICAL_SECTION
    );

    let manifest_name = b"mav:manifest";
    let manifest_offset = bytes
        .windows(manifest_name.len())
        .position(|window| window == manifest_name)
        .expect("manifest section");
    let mut noncanonical = bytes;
    let data_start = manifest_offset + manifest_name.len();
    noncanonical[data_start] = 0xbf;
    assert_eq!(
        Artifact::inspect(noncanonical)
            .expect_err("noncanonical CBOR accepted")
            .code,
        codes::CONNECTOR_ARTIFACT_NONCANONICAL_CBOR
    );
}

#[test]
fn digest_mutation_and_signature_self_exclusion_are_exact() {
    let (bytes, signing_key) = signed_module();
    let original = Artifact::inspect(bytes.clone()).expect("artifact inspects");
    let signature_marker = b"mav:signature";
    let signature_offset = bytes
        .windows(signature_marker.len())
        .position(|window| window == signature_marker)
        .expect("signature section");
    let mut signature_mutated = bytes.clone();
    let last = signature_mutated.len() - 1;
    signature_mutated[last] ^= 1;
    let changed = Artifact::inspect(signature_mutated).expect("signature mutation still parses");
    assert_eq!(
        original.report().signed_digest,
        changed.report().signed_digest
    );
    assert!(signature_offset < last);
    assert_eq!(
        changed
            .verify(
                &policy(publisher(&signing_key, KeyStatus::Active)),
                &empty_revocations(),
                500
            )
            .expect_err("bad signature accepted")
            .code,
        codes::CONNECTOR_TRUST_SIGNATURE_INVALID
    );

    let manifest_marker = b"Example Band";
    let manifest_offset = bytes
        .windows(manifest_marker.len())
        .position(|window| window == manifest_marker)
        .expect("manifest text");
    let mut content_mutated = bytes;
    content_mutated[manifest_offset] = b'F';
    assert_eq!(
        Artifact::inspect(content_mutated)
            .expect_err("digest mutation accepted")
            .code,
        codes::CONNECTOR_ARTIFACT_DIGEST_MISMATCH
    );
}

#[test]
fn wrong_expired_revoked_and_rotated_keys_are_typed() {
    let (bytes, signing_key) = signed_module();
    let artifact = Artifact::inspect(bytes).expect("artifact inspects");
    let wrong = SigningKey::from_bytes(&[8; 32]);
    assert_eq!(
        artifact
            .verify(
                &policy(publisher(&wrong, KeyStatus::Active)),
                &empty_revocations(),
                500
            )
            .expect_err("wrong key accepted")
            .code,
        codes::CONNECTOR_TRUST_SIGNATURE_INVALID
    );
    assert_eq!(
        artifact
            .verify(
                &policy(publisher(&signing_key, KeyStatus::Active)),
                &empty_revocations(),
                1_001
            )
            .expect_err("expired key accepted")
            .code,
        codes::CONNECTOR_TRUST_KEY_EXPIRED
    );
    let mut revoked = empty_revocations();
    revoked.entries.push(Revocation {
        publisher_key_id: "publisher-1".to_owned(),
        revoked_at_ms: 400,
        reason: "compromised".to_owned(),
    });
    assert_eq!(
        artifact
            .verify(
                &policy(publisher(&signing_key, KeyStatus::Active)),
                &revoked,
                500
            )
            .expect_err("revoked key accepted")
            .code,
        codes::CONNECTOR_TRUST_KEY_REVOKED
    );
    assert_eq!(
        artifact
            .verify(
                &policy(publisher(
                    &signing_key,
                    KeyStatus::Rotated {
                        at_ms: 400,
                        replacement_id: "publisher-2".to_owned(),
                    },
                )),
                &empty_revocations(),
                500,
            )
            .expect_err("rotated key accepted")
            .code,
        codes::CONNECTOR_TRUST_KEY_ROTATED
    );
}

#[test]
fn artifact_and_section_size_missing_and_order_fail_before_decode() {
    assert_eq!(
        Artifact::inspect(vec![0; 4 * 1024 * 1024 + 1])
            .expect_err("oversized artifact accepted")
            .code,
        codes::CONNECTOR_ARTIFACT_OVERSIZED
    );

    let mut oversized_section = b"\0asm\x01\0\0\0".to_vec();
    append_custom(&mut oversized_section, "debug", &vec![0; 1024 * 1024 + 1]);
    assert_eq!(
        Artifact::inspect(oversized_section)
            .expect_err("oversized custom section accepted")
            .code,
        codes::CONNECTOR_ARTIFACT_SECTION_OVERSIZED
    );

    let mut too_many_sections = b"\0asm\x01\0\0\0".to_vec();
    for _ in 0..129 {
        append_custom(&mut too_many_sections, "debug", &[]);
    }
    assert_eq!(
        Artifact::inspect(too_many_sections)
            .expect_err("section-count bomb accepted")
            .code,
        codes::CONNECTOR_ARTIFACT_SECTION_OVERSIZED
    );

    assert_eq!(
        Artifact::inspect(unsigned_module())
            .expect_err("missing signature accepted")
            .code,
        codes::CONNECTOR_ARTIFACT_SECTION_MISSING
    );

    let (manifest, abi, _) = valid_records();
    let mut wrong_order = b"\0asm\x01\0\0\0".to_vec();
    append_custom(
        &mut wrong_order,
        "mav:abi",
        &encode_canonical(&abi).expect("ABI encode"),
    );
    append_custom(
        &mut wrong_order,
        "mav:manifest",
        &encode_canonical(&manifest).expect("manifest encode"),
    );
    assert_eq!(
        Artifact::inspect(wrong_order)
            .expect_err("wrong section order accepted")
            .code,
        codes::CONNECTOR_ARTIFACT_SECTION_ORDER
    );
}

#[test]
fn unknown_not_yet_valid_and_disallowed_scope_are_typed() {
    let (bytes, signing_key) = signed_module();
    let artifact = Artifact::inspect(bytes).expect("artifact inspects");
    let empty_policy = TrustPolicy {
        revision: 1,
        allow_third_party: true,
        allow_development: false,
        keys: Vec::new(),
    };
    assert_eq!(
        artifact
            .verify(&empty_policy, &empty_revocations(), 500)
            .expect_err("unknown publisher accepted")
            .code,
        codes::CONNECTOR_TRUST_UNKNOWN_PUBLISHER
    );
    assert_eq!(
        artifact
            .verify(
                &policy(publisher(&signing_key, KeyStatus::Active)),
                &empty_revocations(),
                99,
            )
            .expect_err("early key accepted")
            .code,
        codes::CONNECTOR_TRUST_KEY_NOT_YET_VALID
    );
    let mut rejected_policy = policy(publisher(&signing_key, KeyStatus::Active));
    rejected_policy.allow_third_party = false;
    assert_eq!(
        artifact
            .verify(&rejected_policy, &empty_revocations(), 500)
            .expect_err("disallowed scope accepted")
            .code,
        codes::CONNECTOR_TRUST_SCOPE_REJECTED
    );
    assert_eq!(
        artifact
            .verify(
                &policy(publisher(
                    &signing_key,
                    KeyStatus::Revoked {
                        at_ms: 400,
                        reason: "compromised".to_owned(),
                    },
                )),
                &empty_revocations(),
                500,
            )
            .expect_err("status-revoked key accepted")
            .code,
        codes::CONNECTOR_TRUST_KEY_REVOKED
    );
}

#[test]
fn ambiguous_policy_and_stale_revocations_fail_closed() {
    let (bytes, signing_key) = signed_module();
    let artifact = Artifact::inspect(bytes).expect("artifact inspects");
    let key = publisher(&signing_key, KeyStatus::Active);
    let duplicate_policy = TrustPolicy {
        revision: 2,
        allow_third_party: true,
        allow_development: false,
        keys: vec![key.clone(), key.clone()],
    };
    assert_eq!(
        artifact
            .verify(&duplicate_policy, &empty_revocations(), 500)
            .expect_err("duplicate policy accepted")
            .code,
        codes::CONNECTOR_TRUST_POLICY_INVALID
    );
    let stale = RevocationSet {
        revision: 2,
        generated_at_ms: 100,
        valid_until_ms: 499,
        entries: Vec::new(),
    };
    assert_eq!(
        artifact
            .verify(&policy(key), &stale, 500)
            .expect_err("stale revocations accepted")
            .code,
        codes::CONNECTOR_TRUST_REVOCATION_STALE
    );
}

#[test]
fn signature_digest_has_a_frozen_independent_vector() {
    assert_eq!(
        mav_connector_runtime::signature_digest([unsigned_module().as_slice()]),
        [
            0xa5, 0x12, 0x0b, 0x03, 0x01, 0xd3, 0xec, 0x5c, 0x7d, 0xf0, 0xa2, 0xd4, 0x85, 0x53,
            0x5c, 0xa6, 0x09, 0x5f, 0xaa, 0xac, 0xe6, 0xaa, 0xa7, 0x0c, 0x02, 0x3f, 0x6c, 0xe4,
            0x1d, 0xaf, 0x6c, 0x74,
        ]
    );
}

#[test]
fn parser_and_signature_mutation_corpus_never_escape_typed_failures() {
    let (bytes, signing_key) = signed_module();
    for cut in 0..bytes.len() {
        let error = Artifact::inspect(bytes[..cut].to_vec()).expect_err("truncation accepted");
        assert!((11_001..=11_018).contains(&error.code));
    }
    for index in (0..bytes.len()).step_by(7) {
        let mut mutated = bytes.clone();
        mutated[index] ^= 1;
        match Artifact::inspect(mutated) {
            Err(error) => assert!((11_001..=11_018).contains(&error.code)),
            Ok(artifact) => {
                let error = artifact
                    .verify(
                        &policy(publisher(&signing_key, KeyStatus::Active)),
                        &empty_revocations(),
                        500,
                    )
                    .expect_err("mutated artifact verified");
                assert_eq!(error.code, codes::CONNECTOR_TRUST_SIGNATURE_INVALID);
            }
        }
    }
}
