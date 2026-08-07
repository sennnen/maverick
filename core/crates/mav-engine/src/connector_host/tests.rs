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
    AbiRange, CapabilityDecl, CaptureDecl, CoreRange, DeviceFamily, DowngradePolicy, Entrypoints,
    LimitsProfileId, Permission, ServiceDecl, TimerToken, UpdatePolicy, MANIFEST_SCHEMA_V2,
};

/// What a scripted connector saw and what it will hand back, shared with the test that built it.
#[derive(Default)]
struct ProgramLog {
    /// Every event body the program was given, in order.
    seen: Vec<EventBody>,
    /// What `mav_snapshot` returns. A test that wants to prove a snapshot was taken at a
    /// particular moment sets this to something recognisable first.
    snapshot: Vec<u8>,
    /// When set, the program refuses this many leading events. For the connector that cannot read
    /// its own stored state.
    refuse_first: usize,
}

#[derive(Default)]
struct ScriptedProgram {
    batches: VecDeque<ActionBatch>,
    log: Arc<std::sync::Mutex<ProgramLog>>,
}

impl ScriptedProgram {
    fn new(batches: Vec<ActionBatch>) -> Self {
        Self {
            batches: batches.into(),
            log: Arc::new(std::sync::Mutex::new(ProgramLog {
                snapshot: vec![1, 2, 3],
                ..ProgramLog::default()
            })),
        }
    }

    fn next(&mut self, event: &ConnectorEvent) -> Result<ActionBatch> {
        {
            let mut log = self.log.lock().map_err(|_| host_state("poisoned log"))?;
            log.seen.push(event.body.clone());
            if log.refuse_first > 0 {
                log.refuse_first -= 1;
                return Err(host_state("scripted connector refuses this event"));
            }
        }
        self.batches
            .pop_front()
            .ok_or_else(|| host_state("test program ran out of batches"))
    }
}

impl ConnectorProgram for ScriptedProgram {
    fn init(&mut self, event: &ConnectorEvent) -> Result<ActionBatch> {
        self.next(event)
    }

    fn handle(&mut self, event: &ConnectorEvent) -> Result<ActionBatch> {
        self.next(event)
    }

    fn snapshot(&mut self) -> Result<Vec<u8>> {
        Ok(self
            .log
            .lock()
            .map_err(|_| host_state("poisoned log"))?
            .snapshot
            .clone())
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
        captures: None,
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
    host_with_manifest(manifest(), batches, capacity)
}

/// A host plus a handle on what its connector saw and returns.
fn host_logged(
    batches: Vec<ActionBatch>,
    capacity: u32,
) -> (ConnectorHost, Arc<std::sync::Mutex<ProgramLog>>) {
    let program = ScriptedProgram::new(batches);
    let log = Arc::clone(&program.log);
    let host = ConnectorHost::with_program(
        manifest(),
        [7; 32],
        Box::new(program),
        Store::open_in_memory().expect("store"),
        ConnectorHostConfig {
            session_id: 7,
            device_id: 9,
            transport_capacity: capacity,
        },
    )
    .expect("host");
    (host, log)
}

fn host_with_manifest(
    manifest: Manifest,
    batches: Vec<ActionBatch>,
    capacity: u32,
) -> ConnectorHost {
    ConnectorHost::with_program(
        manifest,
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

fn capture_manifest() -> Manifest {
    let mut value = manifest();
    value.schema = MANIFEST_SCHEMA_V2.to_owned();
    value.capabilities.push(CapabilityDecl {
        stream: "ecg".to_owned(),
        transport: vec![TransportCapability::Subscribe, TransportCapability::Write],
    });
    value.captures = Some(vec![CaptureDecl {
        stream: "ecg".to_owned(),
        unit: "millivolts".to_owned(),
        minimum_sample_rate_hz: 100,
        maximum_sample_rate_hz: 100,
    }]);
    value
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

/// Operation and deadline ids stay unique for the life of a session, not just within one batch.
///
/// The session's sets used to be cloned per batch so a failed batch could be dropped without
/// having touched them; they are now updated only after a batch validates, which is the same
/// guarantee without the copy. Worth pinning because losing it is silent — a connector reusing
/// an id would keep working right up until two operations answered to the same number.
#[test]
pub(super) fn operation_ids_stay_unique_across_batches_and_not_only_within_one() {
    let scan = |operation: u64| {
        action(
            2,
            operation,
            ActionBody::StartScan {
                service_uuids: vec!["service".to_owned()],
                manufacturer_ids: Vec::new(),
            },
        )
    };
    let session = |first: u64, second: u64| {
        host(
            vec![
                empty(),
                batch(vec![scan(first)]),
                batch(vec![action(3, second, ActionBody::StopScan)]),
            ],
            8,
        )
    };

    let mut repeated = session(1, 1);
    repeated.start().expect("start");
    assert_eq!(
        repeated
            .apply(advertisement(), None)
            .expect_err("an id an earlier batch already used")
            .code,
        codes::CONNECTOR_HOST_OPERATION_DUPLICATE
    );

    let mut distinct = session(1, 2);
    distinct.start().expect("start");
    distinct
        .apply(advertisement(), None)
        .expect("a fresh id in a later batch");
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
            stop_capture: false,
        })
    );
    // Second pass: the same sample is recognised, not silently dropped and not stored twice.
    assert_eq!(
        host.commit_samples(BatchId(2), std::slice::from_ref(&sample), Some(2_000)),
        Ok(CommitAccounting {
            emitted: 1,
            persisted: 0,
            duplicate: 1,
            stop_capture: false,
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
        .map(|sample| sample.wall_time().expect("placed").as_nanos())
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

/// A stream kind nothing can name is a stream kind no connector can emit. This caught the ECG
/// case, where the decoder, the model and the hardware documentation all existed while the wire
/// contract had no arm for it, so the whole path was unreachable by construction. The table is
/// spelled out here on purpose: it is the second opinion that makes the check worth running.
#[test]
fn every_stream_kind_has_a_wire_name() {
    const WIRE: [(StreamKind, &str, &str); 24] = [
        (StreamKind::HeartRate, "heart-rate", "beats-per-minute"),
        (StreamKind::RrInterval, "rr-interval", "milliseconds"),
        (StreamKind::PulseInterval, "pulse-interval", "milliseconds"),
        (StreamKind::Ecg, "ecg", "millivolts"),
        (StreamKind::RedPpg, "red-ppg", "counts"),
        (StreamKind::InfraredPpg, "infrared-ppg", "counts"),
        (StreamKind::AmbientLight, "ambient-light", "counts"),
        (StreamKind::Ppg, "ppg", "counts"),
        (StreamKind::OpticalRaw, "optical-raw", "counts"),
        (StreamKind::Imu, "imu", "milli-g"),
        (StreamKind::Gyro, "gyro", "milli-degrees-per-second"),
        (StreamKind::Gravity, "gravity", "milli-g"),
        (StreamKind::SkinTemp, "skin-temp", "degrees-celsius"),
        (StreamKind::SkinTempRaw, "skin-temp-raw", "counts"),
        (StreamKind::Spo2Raw, "spo2-raw", "counts"),
        (StreamKind::Spo2Percent, "spo2-percent", "percent"),
        (StreamKind::RespRaw, "resp-raw", "counts"),
        (StreamKind::BatterySoc, "battery-soc", "percent"),
        (StreamKind::StepCount, "step-count", "count"),
        (StreamKind::ActivityClass, "activity-class", "code"),
        (StreamKind::SkinContact, "skin-contact", "boolean"),
        (StreamKind::SignalQuality, "signal-quality", "percent"),
        (StreamKind::WristState, "wrist-state", "boolean"),
        (StreamKind::SleepStateRaw, "sleep-state-raw", "code"),
    ];
    for kind in mav_model::stream::STREAM_KINDS {
        let (_, name, unit) = WIRE
            .iter()
            .find(|(named, _, _)| *named == kind)
            .unwrap_or_else(|| panic!("{kind:?} has no wire name"));
        assert_eq!(stream_contract(name).expect("parses"), (kind, *unit));
    }
}

#[test]
pub(super) fn low_power_is_stated_on_activation_and_on_change_but_the_default_is_not() {
    // Full power is the connector's own default, so a normal session must cost no extra event.
    let mut normal = host(vec![empty(), empty(), empty()], 4);
    normal.start().expect("start");
    assert!(!normal.low_power());
    assert!(
        !normal.set_low_power(false, Some(1)).expect("no-op"),
        "restating the current mode must not count as a change"
    );

    // Engaging it mid-session delivers exactly one event, which costs one scripted batch.
    assert!(
        normal.set_low_power(true, Some(2)).expect("engage"),
        "engaging low power must report a change"
    );
    assert!(normal.low_power());

    // A session that starts already in low power is told before it does anything else, so it never
    // has to ask: Init, Activate, then the power statement.
    let mut saver = host(vec![empty(), empty(), empty()], 4);
    saver.set_low_power(true, None).expect("pre-set");
    saver.start().expect("start in low power");
    assert!(saver.low_power());
}

#[test]
pub(super) fn capture_is_exposed_only_by_the_signed_and_session_active_intersection() {
    let mut inactive = host_with_manifest(
        capture_manifest(),
        vec![
            empty(),
            batch(vec![action(
                2,
                1,
                ActionBody::DeclareCapabilities {
                    streams: vec!["heart-rate".to_owned()],
                },
            )]),
        ],
        4,
    );
    inactive.start().expect("start");
    assert!(inactive.available_captures().is_empty());
    assert_eq!(
        inactive
            .start_capture("ecg", Some(10))
            .expect_err("inactive capture")
            .code,
        codes::CONNECTOR_HOST_ACTION_UNDECLARED
    );

    let mut active = host_with_manifest(
        capture_manifest(),
        vec![
            empty(),
            batch(vec![action(
                2,
                1,
                ActionBody::DeclareCapabilities {
                    streams: vec!["heart-rate".to_owned(), "ecg".to_owned()],
                },
            )]),
        ],
        4,
    );
    active.start().expect("start");
    assert_eq!(
        active.available_captures(),
        vec![ConnectorCaptureCapability {
            stream: "ecg".to_owned(),
            unit: "millivolts".to_owned(),
            minimum_sample_rate_hz: 100,
            maximum_sample_rate_hz: 100,
        }]
    );
}

#[test]
pub(super) fn capture_start_and_stop_are_semantic_events_with_one_active_capture() {
    let mut active = host_with_manifest(
        capture_manifest(),
        vec![
            empty(),
            batch(vec![action(
                2,
                1,
                ActionBody::DeclareCapabilities {
                    streams: vec!["heart-rate".to_owned(), "ecg".to_owned()],
                },
            )]),
            batch(vec![action(
                3,
                2,
                ActionBody::Write {
                    characteristic_id: "data".to_owned(),
                    bytes: vec![63, 1],
                    confirmed: true,
                },
            )]),
            batch(vec![action(
                4,
                3,
                ActionBody::Write {
                    characteristic_id: "data".to_owned(),
                    bytes: vec![82, 1],
                    confirmed: true,
                },
            )]),
        ],
        4,
    );
    active.start().expect("start");
    active
        .start_capture("ecg", Some(10))
        .expect("capture start");
    assert_eq!(active.active_capture(), Some("ecg"));
    assert_eq!(
        active
            .start_capture("ecg", Some(11))
            .expect_err("second capture")
            .code,
        codes::CONNECTOR_HOST_STATE
    );
    assert!(matches!(
        active.drain_actions(1)[0].body,
        ConnectorTransportRequest::Write { ref bytes, .. } if bytes == &[63, 1]
    ));
    active.stop_capture("ecg", Some(40)).expect("capture stop");
    assert_eq!(active.active_capture(), None);
    assert!(matches!(
        active.drain_actions(1)[0].body,
        ConnectorTransportRequest::Write { ref bytes, .. } if bytes == &[82, 1]
    ));
}

#[test]
pub(super) fn ecg_connector_samples_flow_through_exact_capture_inference_history_and_report_data() {
    const RATE_HZ: usize = 100;
    const CALIBRATION_AND_RECORDING_SECONDS: usize = 37;
    const BASE_MS: i64 = 1_752_600_000_000;

    let source = include_str!("../../../../../fixtures/ecg/n_regular_72_v1.csv")
        .lines()
        .skip(1)
        .map(|line| {
            line.split(',')
                .nth(1)
                .expect("fixture value")
                .parse::<f64>()
                .expect("finite fixture value")
        })
        .collect::<Vec<_>>();
    let quality_probe = (0..RATE_HZ * 5)
        .map(|index| source[index * 256 / RATE_HZ])
        .collect::<Vec<_>>();
    let quality = mav_analytic::ecg_quality::assess_ecg_quality(&quality_probe, RATE_HZ as f64);
    assert!(
        quality.good,
        "100 Hz fixture must calibrate: {:?}",
        quality.reason
    );
    let mut batches = vec![
        empty(),
        batch(vec![action(
            2,
            1,
            ActionBody::DeclareCapabilities {
                streams: vec!["heart-rate".to_owned(), "ecg".to_owned()],
            },
        )]),
        batch(vec![action(
            3,
            2,
            ActionBody::Write {
                characteristic_id: "data".to_owned(),
                bytes: vec![63, 1],
                confirmed: true,
            },
        )]),
    ];
    for second in 0..CALIBRATION_AND_RECORDING_SECONDS {
        let samples = (0..RATE_HZ)
            .map(|within_second| {
                let sample_index = second * RATE_HZ + within_second;
                let source_index = (sample_index * 256 / RATE_HZ) % source.len();
                WireSample {
                    stream: "ecg".to_owned(),
                    value_microunits: (source[source_index] * 1_000_000.0).round() as i64,
                    device_time_ms: Some(BASE_MS + sample_index as i64 * 10),
                    sequence: sample_index as u32,
                    unit: "millivolts".to_owned(),
                }
            })
            .collect();
        let notification_sequence = 4 + second as u64 * 2;
        batches.push(batch(vec![action(
            notification_sequence,
            3 + second as u64,
            ActionBody::EmitSamples {
                batch_id: BatchId(second as u64 + 1),
                samples,
            },
        )]));
        // Every sample batch is followed by the host's SamplesCommitted acknowledgement.
        batches.push(empty());
    }
    let stop_sequence = 4 + (CALIBRATION_AND_RECORDING_SECONDS as u64 - 1) * 2 + 2;
    batches.push(batch(vec![action(
        stop_sequence,
        3 + CALIBRATION_AND_RECORDING_SECONDS as u64,
        ActionBody::Write {
            characteristic_id: "data".to_owned(),
            bytes: vec![82, 1],
            confirmed: true,
        },
    )]));

    let mut active = host_with_manifest(capture_manifest(), batches, 8);
    active.start().expect("active capture session");
    active
        .start_capture("ecg", Some(BASE_MS))
        .expect("ECG start");
    assert!(matches!(
        active.drain_actions(1)[0].body,
        ConnectorTransportRequest::Write { ref bytes, .. } if bytes == &[63, 1]
    ));

    for second in 0..CALIBRATION_AND_RECORDING_SECONDS {
        active
            .apply(
                EventBody::Notification {
                    characteristic_id: "data".to_owned(),
                    bytes: vec![second as u8],
                },
                Some(BASE_MS + (second as i64 + 1) * 1_000),
            )
            .expect("ECG sample batch");
    }

    assert_eq!(
        active.active_capture(),
        None,
        "host auto-stops at 30 seconds"
    );
    assert!(matches!(
        active.drain_actions(1)[0].body,
        ConnectorTransportRequest::Write { ref bytes, .. } if bytes == &[82, 1]
    ));
    let snapshot = active.ecg_capture_snapshot().expect("capture snapshot");
    assert_eq!(
        snapshot.phase,
        EcgCapturePhase::Analysing,
        "capture ended with {:?}",
        snapshot.quality_reason
    );
    assert_eq!(snapshot.recorded_samples, 3_000);
    assert_eq!(snapshot.target_samples, 3_000);

    let request = active
        .ecg_inference_request()
        .expect("native inference work");
    assert_eq!(request.tensors.len(), 7);
    assert!(request.tensors.iter().all(|tensor| tensor.len() == 7_680));
    let mut predictions = vec![[0.74, 0.01, 0.25]; 7];
    predictions[1] = [0.55, 0.02, 0.43];
    let result = active
        .submit_ecg_inference(
            request.capture_id,
            predictions,
            crate::ecg_capture::ECG_COREML_SHA256.to_owned(),
            BASE_MS + 35_000,
        )
        .expect("admitted native result");
    assert_eq!(result.rhythm, mav_model::ecg::EcgRhythmClass::SinusRhythm);
    assert!(result.provisional);
    assert_eq!(result.explanation.len(), 6);

    let history = active
        .store
        .ecg_results(DeviceId::new(9), 10)
        .expect("ECG history");
    assert_eq!(history.as_slice(), std::slice::from_ref(&result));
    let evidence = active
        .store
        .ecg_inference(result.capture_id)
        .expect("inference lookup")
        .expect("durable inference evidence");
    let report_waveform = active
        .store
        .samples_between(
            DeviceId::new(9),
            StreamKind::Ecg,
            WallTime::from_nanos(evidence.started_ns),
            WallTime::from_nanos(evidence.ended_ns),
        )
        .expect("report waveform");
    assert_eq!(report_waveform.len(), 3_000);
    assert_eq!(evidence.sample_count, 3_000);
    assert_eq!(evidence.raw_sha256, result.raw_sha256);
    assert_eq!(evidence.tensor_sha256, result.tensor_sha256);
}

#[test]
pub(super) fn cancelling_an_active_capture_queues_stop_before_disconnect() {
    let cancellation_action = |cause, operation, body| ConnectorAction {
        connector_id: ConnectorId::new("org.example.host").expect("connector id"),
        session_id: mav_connector_abi::SessionId(7),
        caused_by: EventSequence(cause),
        cancellation_generation: CancellationGeneration(1),
        operation_id: OperationId(operation),
        deadline_token: TimerToken(operation + 100),
        body,
    };
    let mut active = host_with_manifest(
        capture_manifest(),
        vec![
            empty(),
            batch(vec![action(
                2,
                1,
                ActionBody::DeclareCapabilities {
                    streams: vec!["heart-rate".to_owned(), "ecg".to_owned()],
                },
            )]),
            batch(vec![action(
                3,
                2,
                ActionBody::Write {
                    characteristic_id: "data".to_owned(),
                    bytes: vec![63, 1],
                    confirmed: true,
                },
            )]),
            batch(vec![cancellation_action(
                4,
                3,
                ActionBody::Write {
                    characteristic_id: "data".to_owned(),
                    bytes: vec![82, 1],
                    confirmed: true,
                },
            )]),
            batch(vec![cancellation_action(5, 4, ActionBody::Disconnect)]),
        ],
        4,
    );
    active.start().expect("start");
    active
        .start_capture("ecg", Some(10))
        .expect("capture start");
    active.drain_actions(4);
    active
        .cancel(CancelReason::User, Some(20))
        .expect("capture cancellation");

    let actions = active.drain_actions(4);
    assert_eq!(actions.len(), 2);
    assert!(matches!(
        actions[0].body,
        ConnectorTransportRequest::Write { ref bytes, .. } if bytes == &[82, 1]
    ));
    assert_eq!(actions[1].body, ConnectorTransportRequest::Disconnect);
    assert_eq!(active.active_capture(), None);
}

// ------------------------------------------------------------------- durable connector state

/// A restored session's first event is `RestoreState`, and it replaces `Init` rather than
/// following it.
///
/// That order is not a preference: it is the order the embedded fixtures run under, so it is the
/// order every connector's parity report was produced against. A production restore that took a
/// different path would be exercising something no fixture covers.
#[test]
pub(super) fn a_restored_session_replaces_init_with_restore_state() {
    let (mut fresh, fresh_log) = host_logged(vec![empty(), empty()], 8);
    fresh.start().expect("fresh start");
    assert_eq!(
        fresh_log.lock().expect("log").seen,
        vec![
            EventBody::Init {
                manifest_hash: [7; 32]
            },
            EventBody::Activate,
        ],
    );

    let (mut restored, restored_log) = host_logged(vec![empty(), empty()], 8);
    restored.start_restored(&[9, 8, 7]).expect("restored start");
    assert_eq!(
        restored_log.lock().expect("log").seen,
        vec![
            EventBody::RestoreState {
                bytes: vec![9, 8, 7]
            },
            EventBody::Activate,
        ],
        "a restored session must not also be told Init",
    );
    // Either way the session is live and in the same place.
    assert_eq!(
        restored.lifecycle_snapshot().lifecycle,
        fresh.lifecycle_snapshot().lifecycle
    );
}

/// The revision only moves on a commit, and the snapshot is the connector's own bytes.
///
/// `mav-ffi` writes state through exactly when the revision moves, so a revision that advanced
/// without a commit would re-serialise a connector's state on every packet, and one that failed to
/// advance on a commit would lose it.
#[test]
pub(super) fn the_state_revision_moves_only_on_a_commit_and_carries_the_connectors_own_snapshot() {
    let (mut host, log) = host_logged(
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
            // Staging alone is not durability.
            batch(vec![action(
                3,
                2,
                ActionBody::StatePut {
                    key: "session".to_owned(),
                    value: vec![4, 5, 6],
                },
            )]),
            batch(vec![action(4, 3, ActionBody::StateCommit)]),
            // One for the `StateCommitted` the commit chains back into the connector, one for the
            // quiet event below.
            empty(),
            empty(),
        ],
        8,
    );
    log.lock().expect("log").snapshot = vec![42, 43];

    host.start().expect("start");
    host.apply(advertisement(), None).expect("staging event");
    assert_eq!(host.state_revision(), 0, "a put is not a commit");

    host.apply(advertisement(), None).expect("commit event");
    assert_eq!(host.state_revision(), 1);
    assert_eq!(host.snapshot_state().expect("snapshot"), vec![42, 43]);

    // An event that commits nothing leaves the revision where it was, which is what keeps the
    // write-through off the packet path.
    host.apply(advertisement(), None).expect("quiet event");
    assert_eq!(host.state_revision(), 1);
}

/// A connector that refuses its own stored state fails the session rather than silently
/// continuing from a blank slate.
///
/// `mav-ffi` catches this, drops the row and starts fresh — but it can only do that if the host
/// tells it, so "restore failed" must be an error and not a shrug.
#[test]
pub(super) fn a_connector_that_refuses_its_stored_state_fails_the_session() {
    let (mut host, log) = host_logged(vec![empty(), empty()], 8);
    log.lock().expect("log").refuse_first = 1;

    let error = host
        .start_restored(&[1, 2, 3])
        .expect_err("a refused restore is not success");
    assert_eq!(error.code, codes::CONNECTOR_HOST_STATE);
    assert_eq!(
        host.lifecycle_snapshot().lifecycle,
        ConnectorLifecycle::Failed
    );
}

/// The namespace a session writes to comes from the signed manifest, not from anything a caller
/// passes in. The store refuses a namespace that disagrees with the active artifact, so this is
/// what keeps state from one publisher out of another's.
#[test]
pub(super) fn the_state_namespace_comes_from_the_signed_manifest() {
    let host = host(vec![empty(), empty()], 8);
    let (publisher, schema) = host.state_namespace();
    assert_eq!(publisher, manifest().publisher_key_id);
    assert_eq!(schema, manifest().state_schema);
    assert_eq!(host.connector_id(), manifest().connector_id.as_str());
}
