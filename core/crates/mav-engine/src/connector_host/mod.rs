use mav_connector_abi::{
    ActionBatch, ActionBody, BatchId, CancelReason, CancellationGeneration, CharacteristicDecl,
    CharacteristicProperty, ConnectorAction, ConnectorEvent, ConnectorId, ConnectorLifecycle,
    DiagnosticLevel, EventBody, EventSequence, Manifest, OperationId, TransportCapability,
    Validate, WireSample, MAX_STATE_BYTES,
};
use mav_connector_runtime::{Artifact, ConnectorInstance, LimitProfile};
use mav_model::ecg::{EcgInferenceEvidence, EcgResult};
use mav_model::error::{codes, MavError, Result};
use mav_model::ids::{DeviceId, EcgCaptureId, MetadataId};
use mav_model::raw::{RawSample, RawValue};
use mav_model::stream::StreamKind;
use mav_model::time::{ClockMap, DeviceTime, WallTime};
use mav_model::version::Version;
use mav_obs::{Ids, Stage, Tap, TapEvent};
use mav_store::{InsertOutcome as StoreInsertOutcome, Provenance, Store};
use mav_timeline::{
    anchor_from, place_on_wall_with, InsertOutcome as TimelineInsertOutcome, Timeline,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use crate::ecg_capture::{
    EcgCaptureController, EcgCapturePhase, EcgCaptureSnapshot, EcgInferenceRequest,
};

const MAX_CHAINED_EVENTS: usize = 32;
const MAX_SESSION_OPERATIONS: usize = 4_096;
const MAX_ADVERTISED_ADDRESSES: usize = 256;

trait ConnectorProgram: Send {
    fn init(&mut self, event: &ConnectorEvent) -> Result<ActionBatch>;
    fn handle(&mut self, event: &ConnectorEvent) -> Result<ActionBatch>;
    fn snapshot(&mut self) -> Result<Vec<u8>>;
}

impl ConnectorProgram for ConnectorInstance {
    fn init(&mut self, event: &ConnectorEvent) -> Result<ActionBatch> {
        ConnectorInstance::init(self, event)
    }

    fn handle(&mut self, event: &ConnectorEvent) -> Result<ActionBatch> {
        ConnectorInstance::handle(self, event)
    }

    fn snapshot(&mut self) -> Result<Vec<u8>> {
        ConnectorInstance::snapshot(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectorHostConfig {
    pub session_id: u64,
    pub device_id: u64,
    pub transport_capacity: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    Applied,
    IgnoredLate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectorLifecycleSnapshot {
    pub lifecycle: ConnectorLifecycle,
    pub session_id: u64,
    pub cancellation_generation: u64,
    pub last_event_sequence: u64,
    pub queued_actions: u32,
    pub outstanding_operations: u32,
    pub state_revision: u64,
    pub trace_hash: String,
    /// Samples this session persisted, and samples it recognised as already held. The second
    /// number is not an error — a historical replay is expected to repeat — but it must be visible,
    /// because an emitted sample that neither persists nor counts as a duplicate has been lost.
    pub samples_persisted: u64,
    pub samples_duplicate: u64,
}

/// A signed captured stream that is also active for the currently connected hardware.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectorCaptureCapability {
    pub stream: String,
    pub unit: String,
    pub minimum_sample_rate_hz: u16,
    pub maximum_sample_rate_hz: u16,
}

/// What one `EmitSamples` batch actually did. `emitted` is what the connector handed over and what
/// it is acknowledged for; the rest is what happened to those samples afterwards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CommitAccounting {
    emitted: usize,
    persisted: usize,
    duplicate: usize,
    stop_capture: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectorTransportAction {
    pub connector_id: ConnectorId,
    pub session_id: u64,
    pub cancellation_generation: u64,
    pub operation_id: u64,
    pub deadline_token: u64,
    pub body: ConnectorTransportRequest,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectorTransportRequest {
    StartScan {
        service_uuids: Vec<String>,
        manufacturer_ids: Vec<u16>,
    },
    StopScan,
    Connect {
        address: String,
    },
    EnsurePaired,
    DiscoverServices,
    Subscribe {
        characteristic_id: String,
    },
    Unsubscribe {
        characteristic_id: String,
    },
    Read {
        characteristic_id: String,
    },
    Write {
        characteristic_id: String,
        bytes: Vec<u8>,
        confirmed: bool,
    },
    Disconnect,
    SetTimer {
        token: u64,
        delay_ms: u64,
    },
    CancelTimer {
        token: u64,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ExpectedResult {
    Read {
        characteristic_id: String,
        connector_operation_id: u64,
    },
    Write {
        characteristic_id: String,
        connector_operation_id: u64,
    },
}

impl ExpectedResult {
    pub(super) fn connector_operation_id(&self) -> u64 {
        match self {
            Self::Read {
                connector_operation_id,
                ..
            }
            | Self::Write {
                connector_operation_id,
                ..
            } => *connector_operation_id,
        }
    }
}

pub struct ConnectorHost {
    connector_id: ConnectorId,
    manifest_hash: [u8; 32],
    manifest: Manifest,
    program: Box<dyn ConnectorProgram>,
    store: Store,
    config: ConnectorHostConfig,
    lifecycle: ConnectorLifecycle,
    event_sequence: u64,
    cancellation_generation: u64,
    actions: VecDeque<ConnectorTransportAction>,
    advertised_addresses: BTreeSet<String>,
    seen_operations: BTreeSet<u64>,
    seen_deadlines: BTreeSet<u64>,
    outstanding: BTreeMap<u64, ExpectedResult>,
    next_host_operation_id: u64,
    next_host_deadline_token: u64,
    pending_timers: BTreeSet<u64>,
    staged_state: BTreeMap<String, Option<Vec<u8>>>,
    /// The host's mirror of what the connector has committed, kept so the session can bound the
    /// namespace. What is made durable is the connector's own `mav_snapshot`, not this map — see
    /// [`Self::snapshot_state`].
    committed_state: BTreeMap<String, Vec<u8>>,
    state_revision: u64,
    timeline: Timeline,
    /// Corrections learned this session. ADR-004 wants clock correction to be a stored mapping;
    /// ADR-022 records why the store does not hold it across sessions yet.
    clock_map: ClockMap,
    /// A passive observer of the stage boundaries. Absent by default so nothing pays for it; the
    /// FFI attaches the ring log.
    tap: Option<Arc<dyn Tap>>,
    trace_hash: u64,
    samples_persisted: u64,
    samples_duplicate: u64,
    /// The duplicate total at the last journal entry. A backfill repeats by design and journalling
    /// every commit buries everything else — a real capture wrote 495 duplicate notices in a
    /// 500-row window. One entry per order of magnitude keeps the fact and drops the noise.
    duplicates_journalled_at: u64,
    /// Host power policy, mirrored to the connector as `PowerModeChanged` (ADR-030). Host-side, not
    /// connector state: a reinstalled or resumed connector is told again rather than remembering.
    low_power: bool,
    /// The device-specific subset the connector declared after identifying this hardware session.
    active_capabilities: BTreeSet<String>,
    /// Only one host-owned capture may drive transport commands at a time.
    active_capture: Option<String>,
    /// ECG orchestration remains in the host so every ECG-capable connector gets the same
    /// calibration, exact duration, native inference contract and history semantics.
    ecg_capture: Option<EcgCaptureController>,
}

mod actions;
mod admission;
mod lifecycle;
mod manifest;
mod trace;

#[cfg(test)]
use admission::stream_contract;
use admission::validate_sample;
use lifecycle::{is_transport, transition_for_action};
use trace::{metadata_id, trace_action, trace_event};

impl ConnectorHost {
    pub fn instantiate(
        artifact: &Artifact,
        profile: LimitProfile,
        store: Store,
        config: ConnectorHostConfig,
    ) -> Result<Self> {
        let manifest = artifact.report().manifest.clone();
        let manifest_hash = artifact.report().manifest_digest;
        let instance = ConnectorInstance::instantiate(artifact, profile)?;
        Self::with_program(manifest, manifest_hash, Box::new(instance), store, config)
    }

    fn with_program(
        manifest: Manifest,
        manifest_hash: [u8; 32],
        program: Box<dyn ConnectorProgram>,
        store: Store,
        config: ConnectorHostConfig,
    ) -> Result<Self> {
        manifest.validate().map_err(|source| {
            error(
                codes::CONNECTOR_HOST_ACTION_INVALID,
                format!("connector manifest rejected by host: {source}"),
            )
        })?;
        if config.session_id == 0 || config.device_id == 0 || config.transport_capacity == 0 {
            return Err(error(
                codes::CONNECTOR_HOST_STATE,
                "connector host ids and transport capacity must be positive",
            ));
        }
        Ok(Self {
            connector_id: manifest.connector_id.clone(),
            manifest_hash,
            manifest,
            program,
            store,
            config,
            lifecycle: ConnectorLifecycle::Installed,
            event_sequence: 0,
            cancellation_generation: 0,
            actions: VecDeque::new(),
            advertised_addresses: BTreeSet::new(),
            seen_operations: BTreeSet::new(),
            seen_deadlines: BTreeSet::new(),
            outstanding: BTreeMap::new(),
            next_host_operation_id: 1,
            next_host_deadline_token: 1,
            pending_timers: BTreeSet::new(),
            staged_state: BTreeMap::new(),
            committed_state: BTreeMap::new(),
            state_revision: 0,
            timeline: Timeline::new(),
            clock_map: ClockMap::default(),
            tap: None,
            trace_hash: 0xcbf2_9ce4_8422_2325,
            samples_persisted: 0,
            samples_duplicate: 0,
            duplicates_journalled_at: 0,
            low_power: false,
            active_capabilities: BTreeSet::new(),
            active_capture: None,
            ecg_capture: None,
        })
    }

    /// Start a fresh session. The connector is told nothing about any previous one.
    pub fn start(&mut self) -> Result<()> {
        self.begin(None)
    }

    /// Start from state a previous session committed.
    ///
    /// `RestoreState` replaces `Init` as the first event rather than following it — that is the
    /// order the embedded fixtures run and therefore the order every connector's parity report was
    /// produced under, so a restored production session and a restored fixture take the same path
    /// through the connector.
    ///
    /// A connector that refuses the bytes fails the session rather than silently continuing from a
    /// blank slate: state it cannot read is a fact worth surfacing, and the caller can always drop
    /// the row and start fresh.
    pub fn start_restored(&mut self, state: &[u8]) -> Result<()> {
        self.begin(Some(state))
    }

    fn begin(&mut self, restored: Option<&[u8]>) -> Result<()> {
        if self.lifecycle != ConnectorLifecycle::Installed {
            return Err(host_state("connector session has already started"));
        }
        let first = match restored {
            Some(bytes) => EventBody::RestoreState {
                bytes: bytes.to_vec(),
            },
            None => EventBody::Init {
                manifest_hash: self.manifest_hash,
            },
        };
        self.dispatch(first, None, true, 0)?;
        self.lifecycle = ConnectorLifecycle::Selected;
        self.dispatch(EventBody::Activate, None, false, 0)?;
        // Connectors start at full power, so only a low-power session needs stating (ADR-030).
        // Sending the default would spend a connector's fuel to tell it what it already assumes.
        if self.low_power {
            self.dispatch(
                EventBody::PowerModeChanged { low_power: true },
                None,
                false,
                0,
            )?;
        }
        Ok(())
    }

    /// Set the host's power policy and tell the running connector. Returns whether it changed;
    /// re-stating the current mode is a no-op rather than a redundant event.
    pub fn set_low_power(&mut self, low_power: bool, wall_time_ms: Option<i64>) -> Result<bool> {
        if self.low_power == low_power {
            return Ok(false);
        }
        self.low_power = low_power;
        if self.lifecycle == ConnectorLifecycle::Installed {
            return Ok(true);
        }
        self.dispatch(
            EventBody::PowerModeChanged { low_power },
            wall_time_ms,
            false,
            0,
        )?;
        Ok(true)
    }

    /// The power policy currently in force.
    pub fn low_power(&self) -> bool {
        self.low_power
    }

    /// Signed capture declarations intersected with the active connected-hardware capabilities.
    pub fn available_captures(&self) -> Vec<ConnectorCaptureCapability> {
        self.manifest
            .captures
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|capture| self.active_capabilities.contains(&capture.stream))
            .map(|capture| ConnectorCaptureCapability {
                stream: capture.stream.clone(),
                unit: capture.unit.clone(),
                minimum_sample_rate_hz: capture.minimum_sample_rate_hz,
                maximum_sample_rate_hz: capture.maximum_sample_rate_hz,
            })
            .collect()
    }

    /// Ask the connector to begin a generic captured stream. Device commands remain connector
    /// knowledge; this method only names a signed, session-active stream.
    pub fn start_capture(&mut self, stream: &str, wall_time_ms: Option<i64>) -> Result<()> {
        if self.lifecycle != ConnectorLifecycle::Streaming {
            return Err(host_state(
                "capture can start only while the connector is streaming",
            ));
        }
        if self.active_capture.is_some() {
            return Err(host_state("another connector capture is already active"));
        }
        let capture = self
            .available_captures()
            .into_iter()
            .find(|capture| capture.stream == stream);
        let Some(capture) = capture else {
            return Err(undeclared(
                "capture stream is not signed and active for this session",
            ));
        };
        let ecg_controller = if stream == "ecg" {
            if capture.minimum_sample_rate_hz != capture.maximum_sample_rate_hz {
                return Err(host_state(
                    "ECG capture requires one declared session sample rate",
                ));
            }
            let now_ms =
                wall_time_ms.ok_or_else(|| host_state("ECG capture requires host wall time"))?;
            Some(EcgCaptureController::begin(
                EcgCaptureId::new(metadata_id(
                    self.config.session_id,
                    self.event_sequence.saturating_add(1),
                    u64::MAX,
                )),
                DeviceId::new(self.config.device_id),
                u32::from(capture.minimum_sample_rate_hz),
                capture.unit,
                now_ms,
            )?)
        } else {
            None
        };
        self.dispatch(
            EventBody::CaptureStart {
                stream: stream.to_owned(),
            },
            wall_time_ms,
            false,
            0,
        )?;
        self.active_capture = Some(stream.to_owned());
        if ecg_controller.is_some() {
            self.ecg_capture = ecg_controller;
        }
        Ok(())
    }

    pub fn stop_capture(&mut self, stream: &str, wall_time_ms: Option<i64>) -> Result<()> {
        if self.active_capture.as_deref() != Some(stream) {
            return Err(host_state("capture stop does not match the active stream"));
        }
        self.dispatch(
            EventBody::CaptureStop {
                stream: stream.to_owned(),
            },
            wall_time_ms,
            false,
            0,
        )?;
        self.active_capture = None;
        if stream == "ecg" {
            if let Some(capture) = self.ecg_capture.as_mut() {
                if !matches!(
                    capture.snapshot().phase,
                    EcgCapturePhase::Analysing | EcgCapturePhase::Result
                ) {
                    capture.cancel();
                }
            }
        }
        Ok(())
    }

    pub fn active_capture(&self) -> Option<&str> {
        self.active_capture.as_deref()
    }

    pub fn ecg_capture_snapshot(&self) -> Option<EcgCaptureSnapshot> {
        self.ecg_capture
            .as_ref()
            .map(EcgCaptureController::snapshot)
    }

    /// The capture state, having first expired a calibration whose deadline has passed.
    ///
    /// A capture that stalls before its first sample has no arriving batch to notice the deadline
    /// on, so the expiry has to ride the read the screen is already making. When it fires the raw
    /// stream is stopped as if the wearer had cancelled: leaving it running is what would keep a
    /// dead capture draining the strap.
    pub fn ecg_capture_snapshot_at(
        &mut self,
        wall_time_ms: Option<i64>,
    ) -> Result<Option<EcgCaptureSnapshot>> {
        let expired = wall_time_ms.is_some_and(|now_ms| {
            self.ecg_capture
                .as_mut()
                .is_some_and(|capture| capture.expire_stalled(now_ms))
        });
        if expired && self.active_capture.as_deref() == Some("ecg") {
            self.active_capture = None;
            self.dispatch(
                EventBody::CaptureStop {
                    stream: "ecg".to_owned(),
                },
                wall_time_ms,
                false,
                0,
            )?;
        }
        Ok(self.ecg_capture_snapshot())
    }

    pub fn ecg_inference_request(&self) -> Option<EcgInferenceRequest> {
        self.ecg_capture
            .as_ref()
            .and_then(EcgCaptureController::inference_request)
    }

    pub fn submit_ecg_inference(
        &mut self,
        capture_id: EcgCaptureId,
        predictions: Vec<[f32; 3]>,
        model_sha256: String,
        now_ms: i64,
    ) -> Result<EcgResult> {
        let capture = self
            .ecg_capture
            .as_mut()
            .ok_or_else(|| host_state("there is no ECG analysis awaiting inference"))?;
        let (evidence, result): (EcgInferenceEvidence, EcgResult) =
            capture.submit_inference(capture_id, predictions, model_sha256, now_ms)?;
        self.store.in_transaction(|store| {
            store.insert_ecg_inference(&evidence)?;
            store.upsert_ecg_result(&result)?;
            Ok(())
        })?;
        Ok(result)
    }

    pub fn apply(
        &mut self,
        mut body: EventBody,
        wall_time_ms: Option<i64>,
    ) -> Result<ApplyOutcome> {
        if !self.accept_event(&mut body, wall_time_ms)? {
            return Ok(ApplyOutcome::IgnoredLate);
        }
        let resumed = matches!(body, EventBody::Resume);
        self.dispatch(body, wall_time_ms, false, 0)?;
        // A resumed connector may have been rebuilt from a snapshot that never carried the power
        // policy, so re-state it rather than making every connector persist a host-owned bit. As on
        // activation, the default needs no restating.
        if resumed && self.low_power {
            self.dispatch(
                EventBody::PowerModeChanged {
                    low_power: self.low_power,
                },
                wall_time_ms,
                false,
                0,
            )?;
        }
        Ok(ApplyOutcome::Applied)
    }

    pub fn cancel(&mut self, reason: CancelReason, wall_time_ms: Option<i64>) -> Result<()> {
        self.cancellation_generation = self
            .cancellation_generation
            .checked_add(1)
            .ok_or_else(|| host_state("connector cancellation generation exhausted"))?;
        self.actions.clear();
        self.outstanding.clear();
        self.pending_timers.clear();
        self.staged_state.clear();
        // A capture owns an open device raw stream. Give the connector a semantic stop under the
        // new cancellation generation before cancellation itself, so its stop write cannot be
        // mistaken for stale work or cleared from the queue.
        if let Some(stream) = self.active_capture.take() {
            self.dispatch(EventBody::CaptureStop { stream }, wall_time_ms, false, 0)?;
        }
        if let Some(capture) = self.ecg_capture.as_mut() {
            capture.cancel();
        }
        self.lifecycle = ConnectorLifecycle::Suspending;
        self.dispatch(EventBody::Cancel { reason }, wall_time_ms, false, 0)?;
        if !self
            .actions
            .iter()
            .any(|action| matches!(action.body, ConnectorTransportRequest::Disconnect))
        {
            self.lifecycle = ConnectorLifecycle::Disconnected;
        }
        Ok(())
    }

    pub fn terminate(&mut self, reason: CancelReason, wall_time_ms: Option<i64>) -> Result<()> {
        if let Err(error) = self.cancel(reason, wall_time_ms) {
            self.store.record_error(
                &error,
                wall_time_ms.unwrap_or_default().saturating_mul(1_000_000),
            )?;
            self.lifecycle = ConnectorLifecycle::Failed;
        }
        Ok(())
    }

    pub fn drain_actions(&mut self, limit: u32) -> Vec<ConnectorTransportAction> {
        let take = usize::try_from(limit)
            .unwrap_or(usize::MAX)
            .min(self.actions.len());
        self.actions.drain(..take).collect()
    }

    pub fn lifecycle_snapshot(&self) -> ConnectorLifecycleSnapshot {
        ConnectorLifecycleSnapshot {
            lifecycle: self.lifecycle,
            session_id: self.config.session_id,
            cancellation_generation: self.cancellation_generation,
            last_event_sequence: self.event_sequence,
            queued_actions: self.actions.len() as u32,
            outstanding_operations: self.outstanding.len() as u32,
            state_revision: self.state_revision,
            trace_hash: format!("{:016x}", self.trace_hash),
            samples_persisted: self.samples_persisted,
            samples_duplicate: self.samples_duplicate,
        }
    }

    /// Attach a passive observer of the pipeline boundaries. A tap can watch, never change.
    pub fn set_tap(&mut self, tap: Arc<dyn Tap>) {
        self.tap = Some(tap);
    }

    /// The ids in flight, so nothing a tap sees is orphaned from the data that caused it.
    pub(super) fn tap_ids(&self) -> Ids {
        Ids {
            device: Some(mav_model::ids::DeviceId::new(self.config.device_id)),
            session: Some(mav_model::ids::SessionId::new(self.config.session_id)),
            stream: None,
            frame: None,
        }
    }

    pub(super) fn observe(&self, stage: Stage, event: TapEvent) {
        if let Some(tap) = &self.tap {
            tap.on_stage(stage, event);
        }
    }

    pub(super) fn observe_produced(&self, stage: Stage, count: usize) {
        self.observe(
            stage,
            TapEvent::Produced {
                count,
                ids: self.tap_ids(),
                summary: None,
            },
        );
    }

    pub fn connector_id(&self) -> &str {
        self.connector_id.as_str()
    }

    pub const fn device_id(&self) -> u64 {
        self.config.device_id
    }

    /// The connector's own serialised state, as its `mav_snapshot` export produces it.
    ///
    /// The inverse of [`Self::start_restored`]. `mav-ffi` takes this whenever
    /// [`Self::state_revision`] moves and writes it to the connector store, which is what makes a
    /// reconnect resume rather than restart.
    pub fn snapshot_state(&mut self) -> Result<Vec<u8>> {
        self.program.snapshot()
    }

    /// How many times the connector has committed its state this session.
    ///
    /// Monotonic, and the only thing a caller needs in order to decide whether a snapshot is worth
    /// taking: `mav_snapshot` runs connector code, so taking one per event would spend a
    /// connector's fuel to re-serialise state that has not moved.
    pub const fn state_revision(&self) -> u64 {
        self.state_revision
    }

    /// The publisher and state-schema half of this connector's state namespace, from the signed
    /// manifest. The store refuses a namespace that disagrees with the active artifact's.
    pub fn state_namespace(&self) -> (&str, u32) {
        (
            self.manifest.publisher_key_id.as_str(),
            self.manifest.state_schema,
        )
    }

    pub(super) fn dispatch(
        &mut self,
        body: EventBody,
        wall_time_ms: Option<i64>,
        initialize: bool,
        depth: usize,
    ) -> Result<()> {
        if depth >= MAX_CHAINED_EVENTS {
            return Err(error(
                codes::CONNECTOR_HOST_ACTION_INVALID,
                "connector generated too many chained host events",
            ));
        }
        self.event_sequence = self
            .event_sequence
            .checked_add(1)
            .ok_or_else(|| host_state("connector event sequence exhausted"))?;
        let sequence = EventSequence(self.event_sequence);
        let event = ConnectorEvent {
            connector_id: self.connector_id.clone(),
            session_id: mav_connector_abi::SessionId(self.config.session_id),
            sequence,
            cancellation_generation: CancellationGeneration(self.cancellation_generation),
            wall_time_ms,
            body,
        };
        self.trace_hash = trace_event(self.trace_hash, &event)?;
        let batch_result = if initialize {
            self.program.init(&event)
        } else {
            self.program.handle(&event)
        };
        let batch = match batch_result {
            Ok(batch) => batch,
            Err(error) => {
                self.lifecycle = ConnectorLifecycle::Failed;
                return Err(error);
            }
        };
        if let Err(error) = self.process_batch(batch, sequence, wall_time_ms, depth) {
            self.lifecycle = ConnectorLifecycle::Failed;
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn process_batch(
        &mut self,
        batch: ActionBatch,
        caused_by: EventSequence,
        wall_time_ms: Option<i64>,
        depth: usize,
    ) -> Result<()> {
        batch.validate().map_err(|source| {
            error(
                codes::CONNECTOR_HOST_ACTION_INVALID,
                format!("connector action batch rejected: {source}"),
            )
        })?;
        let transport_count = batch
            .actions
            .iter()
            .filter(|action| is_transport(&action.body))
            .count();
        if self.actions.len().saturating_add(transport_count)
            > self.config.transport_capacity as usize
        {
            return Err(error(
                codes::CONNECTOR_HOST_QUEUE_FULL,
                "connector transport action queue is full",
            ));
        }
        // Only the ids this batch introduces are collected; the session sets stay untouched until
        // the whole batch has validated. Cloning them to get that atomicity meant copying up to
        // MAX_SESSION_OPERATIONS entries twice per event, on the busiest path in the host.
        let mut operations = BTreeSet::new();
        let mut deadlines = BTreeSet::new();
        let mut simulated = self.lifecycle;
        for action in &batch.actions {
            self.validate_action(action, caused_by, &mut operations, &mut deadlines)?;
            self.simulate_action(&action.body, &mut simulated)?;
        }
        self.validate_state_batch(&batch)?;
        if self.seen_operations.len() + operations.len() > MAX_SESSION_OPERATIONS
            || self.seen_deadlines.len() + deadlines.len() > MAX_SESSION_OPERATIONS
        {
            return Err(error(
                codes::CONNECTOR_HOST_OPERATION_DUPLICATE,
                "connector session operation budget exhausted",
            ));
        }
        self.seen_operations.append(&mut operations);
        self.seen_deadlines.append(&mut deadlines);
        for action in &batch.actions {
            self.trace_hash = trace_action(self.trace_hash, action)?;
        }

        let mut followups = Vec::new();
        for action in batch.actions {
            self.execute_action(action, wall_time_ms, &mut followups)?;
        }
        for body in followups {
            self.dispatch(body, wall_time_ms, false, depth + 1)?;
        }
        Ok(())
    }
}

fn ms_to_ns(value: i64) -> Result<i64> {
    value.checked_mul(1_000_000).ok_or_else(|| {
        error(
            codes::CONNECTOR_HOST_SAMPLE_INVALID,
            "connector millisecond timestamp overflows nanoseconds",
        )
    })
}

fn host_state(message: &str) -> MavError {
    error(codes::CONNECTOR_HOST_STATE, message)
}

fn undeclared(message: &str) -> MavError {
    error(codes::CONNECTOR_HOST_ACTION_UNDECLARED, message)
}

fn error(code: u16, message: impl Into<String>) -> MavError {
    MavError::new(code, message)
}

#[cfg(test)]
mod tests;
