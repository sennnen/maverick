use mav_connector_abi::{
    ActionBatch, ActionBody, BatchId, CancelReason, CancellationGeneration, CharacteristicDecl,
    CharacteristicProperty, ConnectorAction, ConnectorEvent, ConnectorId, ConnectorLifecycle,
    DiagnosticLevel, EventBody, EventSequence, Manifest, OperationId, TransportCapability,
    Validate, WireSample, MAX_STATE_BYTES,
};
use mav_connector_runtime::{Artifact, ConnectorInstance, LimitProfile};
use mav_model::error::{codes, MavError, Result};
use mav_model::ids::{DeviceId, MetadataId};
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

/// What one `EmitSamples` batch actually did. `emitted` is what the connector handed over and what
/// it is acknowledged for; the rest is what happened to those samples afterwards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CommitAccounting {
    emitted: usize,
    persisted: usize,
    duplicate: usize,
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
        })
    }

    pub fn start(&mut self) -> Result<()> {
        if self.lifecycle != ConnectorLifecycle::Installed {
            return Err(host_state("connector session has already started"));
        }
        self.dispatch(
            EventBody::Init {
                manifest_hash: self.manifest_hash,
            },
            None,
            true,
            0,
        )?;
        self.lifecycle = ConnectorLifecycle::Selected;
        self.dispatch(EventBody::Activate, None, false, 0)?;
        Ok(())
    }

    pub fn apply(
        &mut self,
        mut body: EventBody,
        wall_time_ms: Option<i64>,
    ) -> Result<ApplyOutcome> {
        if !self.accept_event(&mut body, wall_time_ms)? {
            return Ok(ApplyOutcome::IgnoredLate);
        }
        self.dispatch(body, wall_time_ms, false, 0)?;
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

    pub fn snapshot_state(&mut self) -> Result<Vec<u8>> {
        self.program.snapshot()
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
        let mut operations = self.seen_operations.clone();
        let mut deadlines = self.seen_deadlines.clone();
        let mut simulated = self.lifecycle;
        for action in &batch.actions {
            self.validate_action(action, caused_by, &mut operations, &mut deadlines)?;
            self.simulate_action(&action.body, &mut simulated)?;
        }
        self.validate_state_batch(&batch)?;
        if operations.len() > MAX_SESSION_OPERATIONS || deadlines.len() > MAX_SESSION_OPERATIONS {
            return Err(error(
                codes::CONNECTOR_HOST_OPERATION_DUPLICATE,
                "connector session operation budget exhausted",
            ));
        }
        self.seen_operations = operations;
        self.seen_deadlines = deadlines;
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
