#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use mav_connector_abi::*;
use sha2::{Digest, Sha256};

fn connector_id() -> ConnectorId {
    ConnectorId::new("org.example.band").expect("valid connector id")
}

fn event(body: EventBody) -> ConnectorEvent {
    ConnectorEvent {
        connector_id: connector_id(),
        session_id: SessionId(1),
        sequence: EventSequence(2),
        cancellation_generation: CancellationGeneration(3),
        wall_time_ms: None,
        body,
    }
}

fn action(body: ActionBody) -> ConnectorAction {
    ConnectorAction {
        connector_id: connector_id(),
        session_id: SessionId(1),
        caused_by: EventSequence(2),
        cancellation_generation: CancellationGeneration(3),
        operation_id: OperationId(4),
        deadline_token: TimerToken(5),
        body,
    }
}

#[test]
fn action_variant_has_a_second_byte_frozen_vector() {
    assert_eq!(
        encode_canonical(&ActionBody::StopScan),
        Ok(vec![0x82, 0x01, 0xa0])
    );
}

#[test]
fn lifecycle_indexes_are_closed_and_frozen() {
    let states = [
        ConnectorLifecycle::Installed,
        ConnectorLifecycle::Selected,
        ConnectorLifecycle::Scanning,
        ConnectorLifecycle::Connecting,
        ConnectorLifecycle::Discovering,
        ConnectorLifecycle::Pairing,
        ConnectorLifecycle::Configuring,
        ConnectorLifecycle::Streaming,
        ConnectorLifecycle::Historical,
        ConnectorLifecycle::Suspending,
        ConnectorLifecycle::Disconnected,
        ConnectorLifecycle::Failed,
    ];
    for (index, state) in states.into_iter().enumerate() {
        assert_eq!(encode_canonical(&state), Ok(vec![index as u8]));
    }
}

#[test]
fn every_closed_event_variant_validates_and_round_trips() {
    let bodies = vec![
        EventBody::Init {
            manifest_hash: [1; 32],
        },
        EventBody::Activate,
        EventBody::Deactivate,
        EventBody::Suspend,
        EventBody::Resume,
        EventBody::Cancel {
            reason: CancelReason::User,
        },
        EventBody::RestoreState { bytes: vec![1] },
        EventBody::Advertisement {
            address: "device-1".to_owned(),
            rssi: -60,
            service_uuids: vec!["180d".to_owned()],
            manufacturer_data: vec![1],
            name: Some("Band".to_owned()),
        },
        EventBody::ScanStopped { reason_code: 1 },
        EventBody::ServicesDiscovered {
            service_uuids: vec!["180d".to_owned()],
        },
        EventBody::IdentityRead {
            field_id: "model".to_owned(),
            bytes: vec![1],
        },
        EventBody::Connected { mtu: 247 },
        EventBody::PairingResult {
            success: true,
            error_code: None,
        },
        EventBody::MtuChanged { mtu: 247 },
        EventBody::Subscribed {
            characteristic_id: "data".to_owned(),
        },
        EventBody::Unsubscribed {
            characteristic_id: "data".to_owned(),
        },
        EventBody::ReadResult {
            operation_id: OperationId(1),
            characteristic_id: "data".to_owned(),
            bytes: vec![1],
        },
        EventBody::WriteResult {
            operation_id: OperationId(1),
            characteristic_id: "control".to_owned(),
        },
        EventBody::Notification {
            characteristic_id: "data".to_owned(),
            bytes: vec![1],
        },
        EventBody::Disconnected { reason_code: 1 },
        EventBody::TransportError {
            operation_id: Some(OperationId(1)),
            code: 1,
            message: "failed".to_owned(),
        },
        EventBody::TimerFired {
            token: TimerToken(1),
        },
        EventBody::StateCommitted { revision: 1 },
        EventBody::SamplesCommitted {
            batch_id: BatchId(1),
            count: 1,
        },
        EventBody::SamplesRejected {
            batch_id: BatchId(1),
            code: 1,
        },
        EventBody::PrepareStateMigration {
            from_schema: 1,
            to_schema: 2,
            state: vec![1],
        },
        EventBody::StateMigrationCommitted { schema: 2 },
    ];
    assert_eq!(bodies.len(), 27);
    for body in bodies {
        let value = event(body);
        let encoded = encode_canonical(&value).expect("event encodes");
        assert_eq!(decode_canonical::<ConnectorEvent>(&encoded), Ok(value));
    }
}

#[test]
fn every_closed_action_variant_validates_and_round_trips() {
    let bodies = vec![
        ActionBody::StartScan {
            service_uuids: vec!["180d".to_owned()],
            manufacturer_ids: vec![1],
        },
        ActionBody::StopScan,
        ActionBody::Connect {
            address: "device-1".to_owned(),
        },
        ActionBody::EnsurePaired,
        ActionBody::DiscoverServices,
        ActionBody::Subscribe {
            characteristic_id: "data".to_owned(),
        },
        ActionBody::Unsubscribe {
            characteristic_id: "data".to_owned(),
        },
        ActionBody::Read {
            characteristic_id: "data".to_owned(),
        },
        ActionBody::Write {
            characteristic_id: "control".to_owned(),
            bytes: vec![1],
            confirmed: true,
        },
        ActionBody::Disconnect,
        ActionBody::SetTimer {
            token: TimerToken(1),
            delay_ms: 1,
        },
        ActionBody::CancelTimer {
            token: TimerToken(1),
        },
        ActionBody::StatePut {
            key: "cursor".to_owned(),
            value: vec![1],
        },
        ActionBody::StateDelete {
            key: "cursor".to_owned(),
        },
        ActionBody::StateCommit,
        ActionBody::EmitSamples {
            batch_id: BatchId(1),
            samples: vec![WireSample {
                stream: "heart-rate".to_owned(),
                value_microunits: 60_000_000,
                device_time_ms: Some(1),
                sequence: 1,
                unit: "beats-per-minute".to_owned(),
            }],
        },
        ActionBody::EmitDiagnostic {
            level: DiagnosticLevel::Warning,
            code: "packet-rejected".to_owned(),
            message: "bad packet".to_owned(),
        },
        ActionBody::DeclareCapabilities {
            streams: vec!["heart-rate".to_owned()],
        },
        ActionBody::CompleteOperation {
            operation_id: OperationId(1),
        },
    ];
    assert_eq!(bodies.len(), 19);
    for body in bodies {
        let value = action(body);
        let encoded = encode_canonical(&value).expect("action encodes");
        assert_eq!(decode_canonical::<ConnectorAction>(&encoded), Ok(value));
    }
}

#[test]
fn action_and_state_bounds_fail_at_the_first_excess_byte_or_item() {
    let oversized_write = action(ActionBody::Write {
        characteristic_id: "data".to_owned(),
        bytes: vec![0; MAX_EVENT_BYTES + 1],
        confirmed: false,
    });
    assert_eq!(
        encode_canonical(&oversized_write),
        Err(WireError::Bounds("write bytes"))
    );

    let oversized_state = action(ActionBody::StatePut {
        key: "cursor".to_owned(),
        value: vec![0; MAX_STATE_BYTES + 1],
    });
    assert_eq!(
        encode_canonical(&oversized_state),
        Err(WireError::Bounds("state value"))
    );

    let actions = (0..=MAX_ACTIONS)
        .map(|_| action(ActionBody::StopScan))
        .collect();
    assert_eq!(
        encode_canonical(&ActionBatch { actions }),
        Err(WireError::Bounds("actions per event"))
    );

    assert_eq!(
        encode_canonical(&ActionBody::SetTimer {
            token: TimerToken(1),
            delay_ms: MAX_TIMER_DELAY_MS + 1,
        }),
        Err(WireError::Bounds("timer delay"))
    );
}

#[test]
fn artifact_records_round_trip_under_the_same_canonical_decoder() {
    let manifest = Manifest {
        schema: MANIFEST_SCHEMA.to_owned(),
        connector_id: connector_id(),
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
        artifact_limits_profile: LimitsProfileId::new("mobile-v1").expect("valid profile"),
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
        permissions: vec![Permission::Ble],
        entrypoints: Entrypoints::default(),
        fixture_set_hash: [2; 32],
        update: UpdatePolicy {
            channel: "stable".to_owned(),
            downgrade: DowngradePolicy::Reject,
        },
    };
    let descriptor = AbiDescriptor {
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
        wasm_features: vec![WasmFeature::MutableGlobals],
        sdk_version: "0.1.0".to_owned(),
    };
    let fixtures = FixtureSet {
        schema: FIXTURES_SCHEMA.to_owned(),
        cases: vec![FixtureCase {
            name: "activate".to_owned(),
            initial_state: Vec::new(),
            events: vec![event(EventBody::Activate)],
            expected: vec![ActionBatch {
                actions: vec![action(ActionBody::StopScan)],
            }],
            expected_state_hash: [3; 32],
            max_fuel: 1_000,
            expected_samples: None,
            expected_diagnostics: None,
        }],
    };
    let signature = SignatureRecord {
        schema: SIGNATURE_SCHEMA.to_owned(),
        algorithm: SignatureAlgorithm::Ed25519,
        publisher_key_id: "publisher-1".to_owned(),
        digest: [4; 32],
        signature: [5; 64],
    };

    let manifest_bytes = encode_canonical(&manifest).expect("manifest encodes");
    let descriptor_bytes = encode_canonical(&descriptor).expect("descriptor encodes");
    let fixtures_bytes = encode_canonical(&fixtures).expect("fixtures encode");
    let signature_bytes = encode_canonical(&signature).expect("signature encodes");
    assert_eq!(decode_canonical(&manifest_bytes), Ok(manifest));
    assert_eq!(decode_canonical(&descriptor_bytes), Ok(descriptor));
    assert_eq!(decode_canonical(&fixtures_bytes), Ok(fixtures));
    assert_eq!(decode_canonical(&signature_bytes), Ok(signature));
}

#[test]
fn schema_hash_is_frozen() {
    assert_eq!(
        ABI_V1_SCHEMA_HASH,
        [
            0xb9, 0x01, 0xe5, 0xa7, 0x01, 0xe7, 0xaf, 0x57, 0x94, 0xb7, 0x4f, 0xf5, 0xbe, 0xb0,
            0x55, 0x12, 0xa1, 0xe6, 0xfa, 0x0e, 0x3e, 0x76, 0xcc, 0x7c, 0x97, 0xdc, 0x72, 0xf8,
            0xb6, 0x6d, 0x2e, 0xa8,
        ]
    );
    let cases: &[(&[u8], [u8; 32])] = &[
        (include_bytes!("../schema/abi-v1.cddl"), ABI_V1_SCHEMA_HASH),
        (
            include_bytes!("../schema/manifest-v1.cddl"),
            MANIFEST_V1_SCHEMA_HASH,
        ),
        (
            include_bytes!("../schema/fixtures-v1.cddl"),
            FIXTURES_V1_SCHEMA_HASH,
        ),
        (
            include_bytes!("../schema/signature-v1.cddl"),
            SIGNATURE_V1_SCHEMA_HASH,
        ),
    ];
    for (schema, expected) in cases {
        assert_eq!(Sha256::digest(schema).as_slice(), expected);
    }
}
