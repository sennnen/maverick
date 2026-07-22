//! Host behaviour under a scripted connector program: lifecycle order, transport ids, queue
//! bounds, sample durability, commit accounting, and clock placement.

use super::*;

/// WHOOP 4.0 publishes a thermistor register, not degrees. The two temperature streams must
/// stay distinguishable at the pipeline boundary or a raw count is stored as a temperature.
#[test]
pub(super) fn raw_and_calibrated_skin_temperature_are_separate_streams() {
    assert_eq!(
        stream_contract("skin-temp").unwrap(),
        (StreamKind::SkinTemp, "degrees-celsius")
    );
    assert_eq!(
        stream_contract("skin-temp-raw").unwrap(),
        (StreamKind::SkinTempRaw, "counts")
    );
    // A raw reading offered as degrees is refused by the unit check, not silently accepted.
    let mislabelled = WireSample {
        stream: "skin-temp-raw".to_owned(),
        value_microunits: 861_000_000,
        device_time_ms: Some(1),
        sequence: 0,
        unit: "degrees-celsius".to_owned(),
    };
    assert_eq!(
        validate_sample(&mislabelled).unwrap_err().code,
        codes::CONNECTOR_HOST_SAMPLE_INVALID
    );
}

use mav_connector_abi::{
    AbiRange, CapabilityDecl, CoreRange, DeviceFamily, DowngradePolicy, Entrypoints,
    LimitsProfileId, Permission, ServiceDecl, TimerToken, UpdatePolicy,
};

#[derive(Default)]
struct ScriptedProgram {
    batches: VecDeque<ActionBatch>,
}

impl ScriptedProgram {
    fn new(batches: Vec<ActionBatch>) -> Self {
        Self {
            batches: batches.into(),
        }
    }

    fn next(&mut self) -> Result<ActionBatch> {
        self.batches
            .pop_front()
            .ok_or_else(|| host_state("test program ran out of batches"))
    }
}

impl ConnectorProgram for ScriptedProgram {
    fn init(&mut self, _event: &ConnectorEvent) -> Result<ActionBatch> {
        self.next()
    }

    fn handle(&mut self, _event: &ConnectorEvent) -> Result<ActionBatch> {
        self.next()
    }

    fn snapshot(&mut self) -> Result<Vec<u8>> {
        Ok(vec![1, 2, 3])
    }
}

pub(super) fn manifest() -> Manifest {
    Manifest {
        schema: mav_connector_abi::MANIFEST_SCHEMA.to_owned(),
        connector_id: ConnectorId::new("org.example.host").expect("connector id"),
        version: "1.0.0".to_owned(),
        display_name: "Host test".to_owned(),
        description: "Generic lifecycle test".to_owned(),
        publisher_key_id: "test-key".to_owned(),
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
            id: "test".to_owned(),
            name_prefixes: vec!["Test".to_owned()],
            service_uuids: vec!["service".to_owned()],
            manufacturer_id: None,
            manufacturer_mask: Vec::new(),
            manufacturer_value: Vec::new(),
        }],
        services: vec![ServiceDecl {
            id: "primary".to_owned(),
            uuid: "service".to_owned(),
            characteristics: vec![CharacteristicDecl {
                id: "data".to_owned(),
                uuid: "data-uuid".to_owned(),
                properties: vec![
                    CharacteristicProperty::Notify,
                    CharacteristicProperty::Read,
                    CharacteristicProperty::Write,
                ],
                sensitive: false,
                confirmed_write_required: true,
            }],
        }],
        capabilities: vec![CapabilityDecl {
            stream: "heart-rate".to_owned(),
            transport: vec![
                TransportCapability::Scan,
                TransportCapability::Connect,
                TransportCapability::Pair,
                TransportCapability::Discover,
                TransportCapability::Subscribe,
                TransportCapability::Read,
                TransportCapability::Write,
            ],
        }],
        permissions: vec![Permission::Ble],
        entrypoints: Entrypoints::default(),
        fixture_set_hash: [0; 32],
        update: UpdatePolicy {
            channel: "stable".to_owned(),
            downgrade: DowngradePolicy::Reject,
        },
    }
}

pub(super) fn action(cause: u64, operation: u64, body: ActionBody) -> ConnectorAction {
    ConnectorAction {
        connector_id: ConnectorId::new("org.example.host").expect("connector id"),
        session_id: mav_connector_abi::SessionId(7),
        caused_by: EventSequence(cause),
        cancellation_generation: CancellationGeneration(0),
        operation_id: OperationId(operation),
        deadline_token: TimerToken(operation + 100),
        body,
    }
}

pub(super) fn batch(actions: Vec<ConnectorAction>) -> ActionBatch {
    ActionBatch { actions }
}

pub(super) fn empty() -> ActionBatch {
    batch(Vec::new())
}

pub(super) fn host(batches: Vec<ActionBatch>, capacity: u32) -> ConnectorHost {
    ConnectorHost::with_program(
        manifest(),
        [7; 32],
        Box::new(ScriptedProgram::new(batches)),
        Store::open_in_memory().expect("store"),
        ConnectorHostConfig {
            session_id: 7,
            device_id: 9,
            transport_capacity: capacity,
        },
    )
    .expect("host")
}

pub(super) fn advertisement() -> EventBody {
    EventBody::Advertisement {
        address: "native-device".to_owned(),
        rssi: -42,
        service_uuids: vec!["service".to_owned()],
        manufacturer_data: Vec::new(),
        name: Some("Test".to_owned()),
    }
}

#[test]
pub(super) fn forced_termination_journals_hostile_cancel_failure() {
    let mut host = host(vec![empty(), empty()], 4);
    host.start().expect("start");
    host.terminate(CancelReason::Update, Some(10))
        .expect("forced termination");
    assert_eq!(host.lifecycle, ConnectorLifecycle::Failed);
    let errors = host.store.recent_errors(1).expect("errors");
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].code, codes::CONNECTOR_HOST_STATE);
}

#[test]
pub(super) fn lifecycle_script_is_ordered_and_device_neutral() {
    let mut host = host(
        vec![
            empty(),
            batch(vec![action(
                2,
                1,
                ActionBody::StartScan {
                    service_uuids: vec!["service".to_owned()],
                    manufacturer_ids: Vec::new(),
                },
            )]),
            batch(vec![
                action(3, 2, ActionBody::StopScan),
                action(
                    3,
                    3,
                    ActionBody::Connect {
                        address: "native-device".to_owned(),
                    },
                ),
            ]),
            batch(vec![action(4, 4, ActionBody::EnsurePaired)]),
            batch(vec![action(5, 5, ActionBody::DiscoverServices)]),
            batch(vec![action(
                6,
                6,
                ActionBody::Subscribe {
                    characteristic_id: "data".to_owned(),
                },
            )]),
            batch(vec![action(
                7,
                7,
                ActionBody::DeclareCapabilities {
                    streams: vec!["heart-rate".to_owned()],
                },
            )]),
            empty(),
        ],
        16,
    );
    host.start().expect("start");
    assert_eq!(
        host.drain_actions(8)[0].body,
        ConnectorTransportRequest::StartScan {
            service_uuids: vec!["service".to_owned()],
            manufacturer_ids: Vec::new(),
        }
    );
    assert_eq!(host.apply(advertisement(), None), Ok(ApplyOutcome::Applied));
    assert_eq!(host.drain_actions(8).len(), 2);
    host.apply(EventBody::Connected { mtu: 247 }, None)
        .expect("connected");
    assert!(matches!(
        host.drain_actions(8)[0].body,
        ConnectorTransportRequest::EnsurePaired
    ));
    host.apply(
        EventBody::PairingResult {
            success: true,
            error_code: None,
        },
        None,
    )
    .expect("paired");
    host.drain_actions(8);
    host.apply(
        EventBody::ServicesDiscovered {
            service_uuids: vec!["service".to_owned()],
        },
        None,
    )
    .expect("discovered");
    host.drain_actions(8);
    host.apply(
        EventBody::Subscribed {
            characteristic_id: "data".to_owned(),
        },
        None,
    )
    .expect("subscribed");
    assert_eq!(
        host.lifecycle_snapshot().lifecycle,
        ConnectorLifecycle::Streaming
    );
    host.cancel(CancelReason::Disconnect, None)
        .expect("disconnect cancellation");
    assert_eq!(
        host.lifecycle_snapshot().lifecycle,
        ConnectorLifecycle::Disconnected
    );
    assert_eq!(host.lifecycle_snapshot().trace_hash, "803108d42f3ddcf5");
}

#[test]
pub(super) fn transport_ids_are_host_assigned() {
    let mut host = host(
        vec![
            empty(),
            batch(vec![ConnectorAction {
                connector_id: ConnectorId::new("org.example.host").expect("connector id"),
                session_id: mav_connector_abi::SessionId(7),
                caused_by: EventSequence(2),
                cancellation_generation: CancellationGeneration(0),
                operation_id: OperationId(900),
                deadline_token: TimerToken(901),
                body: ActionBody::StartScan {
                    service_uuids: vec!["service".to_owned()],
                    manufacturer_ids: Vec::new(),
                },
            }]),
        ],
        8,
    );
    host.start().expect("start");
    let action = host.drain_actions(1).pop().expect("transport action");
    assert_eq!(action.operation_id, 1);
    assert_eq!(action.deadline_token, 1);
}

#[test]
pub(super) fn wrong_order_and_undeclared_actions_reject_exactly() {
    let mut wrong_order = host(vec![empty(), empty()], 8);
    wrong_order.start().expect("start");
    assert_eq!(
        wrong_order
            .apply(EventBody::Connected { mtu: 247 }, None)
            .expect_err("wrong order")
            .code,
        codes::CONNECTOR_HOST_STATE
    );

    let mut undeclared = host(
        vec![
            empty(),
            batch(vec![action(
                2,
                1,
                ActionBody::Read {
                    characteristic_id: "secret".to_owned(),
                },
            )]),
        ],
        8,
    );
    assert_eq!(
        undeclared.start().expect_err("undeclared").code,
        codes::CONNECTOR_HOST_ACTION_UNDECLARED
    );
    assert!(undeclared.drain_actions(8).is_empty());
}

#[test]
pub(super) fn queue_bound_is_atomic_and_cancelled_results_are_logged_and_ignored() {
    let mut bounded = host(
        vec![
            empty(),
            batch(vec![
                action(
                    2,
                    1,
                    ActionBody::StartScan {
                        service_uuids: vec!["service".to_owned()],
                        manufacturer_ids: Vec::new(),
                    },
                ),
                action(2, 2, ActionBody::StopScan),
            ]),
        ],
        1,
    );
    assert_eq!(
        bounded.start().expect_err("queue full").code,
        codes::CONNECTOR_HOST_QUEUE_FULL
    );
    assert!(bounded.drain_actions(8).is_empty());

    let mut late = host(
        vec![
            empty(),
            batch(vec![action(
                2,
                1,
                ActionBody::StartScan {
                    service_uuids: vec!["service".to_owned()],
                    manufacturer_ids: Vec::new(),
                },
            )]),
            batch(vec![action(
                3,
                2,
                ActionBody::Connect {
                    address: "native-device".to_owned(),
                },
            )]),
            batch(vec![action(4, 3, ActionBody::DiscoverServices)]),
            batch(vec![action(
                5,
                4,
                ActionBody::Subscribe {
                    characteristic_id: "data".to_owned(),
                },
            )]),
            batch(vec![action(
                6,
                5,
                ActionBody::Write {
                    characteristic_id: "data".to_owned(),
                    bytes: vec![1],
                    confirmed: true,
                },
            )]),
            empty(),
            empty(),
        ],
        16,
    );
    late.start().expect("start");
    late.drain_actions(8);
    late.apply(advertisement(), None).expect("advertisement");
    late.drain_actions(8);
    late.apply(EventBody::Connected { mtu: 247 }, None)
        .expect("connected");
    late.drain_actions(8);
    late.apply(
        EventBody::ServicesDiscovered {
            service_uuids: vec!["service".to_owned()],
        },
        None,
    )
    .expect("services");
    late.drain_actions(8);
    late.apply(
        EventBody::Subscribed {
            characteristic_id: "data".to_owned(),
        },
        None,
    )
    .expect("subscribed");
    late.drain_actions(8);
    late.cancel(CancelReason::Disconnect, Some(10))
        .expect("cancel");
    assert_eq!(
        late.apply(
            EventBody::WriteResult {
                operation_id: OperationId(5),
                characteristic_id: "data".to_owned(),
            },
            Some(11),
        ),
        Ok(ApplyOutcome::IgnoredLate)
    );
    assert_eq!(
        late.store.recent_errors(1).expect("errors")[0].code,
        codes::CONNECTOR_HOST_LATE_RESULT
    );
}

#[test]
pub(super) fn samples_are_durable_before_a_later_write_is_visible() {
    let sample = WireSample {
        stream: "heart-rate".to_owned(),
        value_microunits: 63_000_000,
        device_time_ms: Some(1_000),
        sequence: 0,
        unit: "beats-per-minute".to_owned(),
    };
    let mut host = host(
        vec![
            empty(),
            batch(vec![action(
                2,
                1,
                ActionBody::StartScan {
                    service_uuids: vec!["service".to_owned()],
                    manufacturer_ids: Vec::new(),
                },
            )]),
            batch(vec![action(
                3,
                2,
                ActionBody::Connect {
                    address: "native-device".to_owned(),
                },
            )]),
            batch(vec![action(4, 3, ActionBody::DiscoverServices)]),
            batch(vec![action(
                5,
                4,
                ActionBody::Subscribe {
                    characteristic_id: "data".to_owned(),
                },
            )]),
            empty(),
            batch(vec![
                action(
                    7,
                    5,
                    ActionBody::EmitSamples {
                        batch_id: BatchId(44),
                        samples: vec![sample],
                    },
                ),
                action(
                    7,
                    6,
                    ActionBody::Write {
                        characteristic_id: "data".to_owned(),
                        bytes: vec![0xaa],
                        confirmed: true,
                    },
                ),
            ]),
            empty(),
        ],
        16,
    );
    host.start().expect("start");
    host.drain_actions(8);
    host.apply(advertisement(), None).expect("advertisement");
    host.drain_actions(8);
    host.apply(EventBody::Connected { mtu: 247 }, None)
        .expect("connected");
    host.drain_actions(8);
    host.apply(
        EventBody::ServicesDiscovered {
            service_uuids: vec!["service".to_owned()],
        },
        None,
    )
    .expect("services");
    host.drain_actions(8);
    host.apply(
        EventBody::Subscribed {
            characteristic_id: "data".to_owned(),
        },
        None,
    )
    .expect("subscribed");
    host.apply(
        EventBody::Notification {
            characteristic_id: "data".to_owned(),
            bytes: vec![1, 2],
        },
        Some(2_000),
    )
    .expect("notification");
    assert_eq!(
        host.store
            .samples(DeviceId::new(9), StreamKind::HeartRate)
            .expect("stored")
            .len(),
        1
    );
    assert!(matches!(
        host.drain_actions(8)[0].body,
        ConnectorTransportRequest::Write { .. }
    ));
}

#[test]
pub(super) fn duplicate_samples_acknowledge_as_durable_without_duplicate_rows() {
    let sample = WireSample {
        stream: "heart-rate".to_owned(),
        value_microunits: 63_000_000,
        device_time_ms: Some(1_000),
        sequence: 0,
        unit: "beats-per-minute".to_owned(),
    };
    let mut host = host(Vec::new(), 8);
    // First pass: one sample emitted, one persisted, none already held.
    assert_eq!(
        host.commit_samples(BatchId(1), std::slice::from_ref(&sample), Some(2_000)),
        Ok(CommitAccounting {
            emitted: 1,
            persisted: 1,
            duplicate: 0,
        })
    );
    // Second pass: the same sample is recognised, not silently dropped and not stored twice.
    assert_eq!(
        host.commit_samples(BatchId(2), std::slice::from_ref(&sample), Some(2_000)),
        Ok(CommitAccounting {
            emitted: 1,
            persisted: 0,
            duplicate: 1,
        })
    );
    assert_eq!(
        host.store
            .samples(DeviceId::new(9), StreamKind::HeartRate)
            .expect("stored")
            .len(),
        1
    );
    // Provenance is written for the sample that persisted and withheld for the duplicate; a
    // second row would point at nothing.
    let session = host.config.session_id;
    let first = MetadataId::new(metadata_id(session, 1, 0));
    let second = MetadataId::new(metadata_id(session, 2, 0));
    assert!(host.store.provenance(first).expect("first").is_some());
    assert!(host.store.provenance(second).expect("second").is_none());
    let snapshot = host.lifecycle_snapshot();
    assert_eq!(snapshot.samples_persisted, 1);
    assert_eq!(snapshot.samples_duplicate, 1);
}

/// A strap whose RTC never latched real time still reports monotonic device time. Placing the
/// whole burst on the capture instant destroys every interval in it; one learned anchor shifts
/// the burst and keeps them.
#[test]
pub(super) fn a_stale_device_clock_keeps_the_intervals_inside_a_burst() {
    let batch = [0i64, 10_000, 25_000]
        .into_iter()
        .enumerate()
        .map(|(sequence, device_ms)| WireSample {
            stream: "heart-rate".to_owned(),
            value_microunits: 60_000_000 + sequence as i64 * 1_000_000,
            device_time_ms: Some(device_ms),
            sequence: sequence as u32,
            unit: "beats-per-minute".to_owned(),
        })
        .collect::<Vec<_>>();
    let mut host = host(Vec::new(), 8);
    let capture_ms = 1_752_600_123_000;
    host.commit_samples(BatchId(1), &batch, Some(capture_ms))
        .expect("commit");

    let stored = host
        .store
        .samples(DeviceId::new(9), StreamKind::HeartRate)
        .expect("stored");
    assert_eq!(stored.len(), 3);
    let walls = stored
        .iter()
        .map(|sample| sample.wall_time.expect("placed").as_nanos())
        .collect::<Vec<_>>();
    assert_eq!(walls[1] - walls[0], 10_000 * 1_000_000);
    assert_eq!(walls[2] - walls[1], 15_000 * 1_000_000);
    // Raw device timestamps are never rewritten, only mapped.
    assert_eq!(stored[0].device_time, DeviceTime::from_nanos(0));
    assert_eq!(
        stored[2].device_time,
        DeviceTime::from_nanos(25_000_000_000)
    );
}

/// A batch larger than the timeline's dedup window replays past it, so the fast path can no
/// longer answer. The store's natural key is the layer that still must, and each sample has to
/// persist exactly once across both passes.
#[test]
pub(super) fn a_replay_larger_than_the_dedup_window_still_persists_each_sample_once() {
    let batch = (0..64u32)
        .map(|sequence| WireSample {
            stream: "heart-rate".to_owned(),
            value_microunits: 63_000_000,
            device_time_ms: Some(1_000 + i64::from(sequence)),
            sequence,
            unit: "beats-per-minute".to_owned(),
        })
        .collect::<Vec<_>>();
    let mut host = host(Vec::new(), 8);
    host.timeline = Timeline::with_window(8);

    let first = host
        .commit_samples(BatchId(1), &batch, Some(2_000))
        .expect("first commit");
    assert_eq!(first.emitted, 64);
    assert_eq!(first.persisted, 64);
    assert_eq!(first.duplicate, 0);

    let second = host
        .commit_samples(BatchId(2), &batch, Some(2_000))
        .expect("second commit");
    assert_eq!(second.emitted, 64);
    assert_eq!(
        second.persisted, 0,
        "the store rejects what the window forgot"
    );
    assert_eq!(second.duplicate, 64, "and every one of them is counted");

    assert_eq!(
        host.store
            .samples(DeviceId::new(9), StreamKind::HeartRate)
            .expect("stored")
            .len(),
        64
    );
}

/// The Tap sees the four commit boundaries in pipeline order, with counts that agree with the
/// commit accounting. A tap that reports something different from what was stored is worse than
/// no tap: it is a plausible lie in the report bundle.
#[test]
fn a_tap_sees_the_commit_boundaries_in_pipeline_order() {
    #[derive(Default)]
    struct Recorder(std::sync::Mutex<Vec<(mav_obs::Stage, usize)>>);

    impl mav_obs::Tap for Recorder {
        fn on_stage(&self, stage: mav_obs::Stage, event: mav_obs::TapEvent) {
            if let mav_obs::TapEvent::Produced { count, .. } = event {
                if let Ok(mut seen) = self.0.lock() {
                    seen.push((stage, count));
                }
            }
        }
    }

    let recorder = std::sync::Arc::new(Recorder::default());
    let mut host = host(Vec::new(), 8);
    host.set_tap(recorder.clone());

    let batch = (0..3u32)
        .map(|sequence| WireSample {
            stream: "heart-rate".to_owned(),
            value_microunits: 63_000_000,
            device_time_ms: Some(1_752_600_000_000 + i64::from(sequence)),
            sequence,
            unit: "beats-per-minute".to_owned(),
        })
        .collect::<Vec<_>>();
    let accounting = host
        .commit_samples(BatchId(1), &batch, Some(1_752_600_001_000))
        .expect("commit");

    let seen = recorder.0.lock().expect("recorded").clone();
    assert_eq!(
        seen,
        vec![
            (mav_obs::Stage::Decode, 1),
            (mav_obs::Stage::Sqi, 1),
            (mav_obs::Stage::Decode, 1),
            (mav_obs::Stage::Sqi, 1),
            (mav_obs::Stage::Decode, 1),
            (mav_obs::Stage::Sqi, 1),
            (mav_obs::Stage::Timeline, 3),
            (mav_obs::Stage::Store, 3),
        ]
    );
    assert_eq!(accounting.persisted, 3);

    // A replay produces no timeline output and stores nothing, and the tap says exactly that.
    recorder.0.lock().expect("recorded").clear();
    let replay = host
        .commit_samples(BatchId(2), &batch, Some(1_752_600_002_000))
        .expect("replay");
    let seen = recorder.0.lock().expect("recorded").clone();
    assert_eq!(seen.last(), Some(&(mav_obs::Stage::Store, 0)));
    assert_eq!(replay.duplicate, 3);
}
