use mav_connector_abi::{
    ActionBatch, ActionBody, BatchId, CancelReason, CancellationGeneration, CharacteristicDecl,
    CharacteristicProperty, ConnectorAction, ConnectorEvent, ConnectorId, ConnectorLifecycle,
    EventBody, EventSequence, Manifest, OperationId, TransportCapability, Validate, WireSample,
    MAX_STATE_BYTES,
};
use mav_connector_runtime::{Artifact, ConnectorInstance, LimitProfile};
use mav_model::error::{codes, MavError, Result};
use mav_model::ids::{DeviceId, MetadataId};
use mav_model::raw::{RawSample, RawSampleBatch, RawValue};
use mav_model::stream::StreamKind;
use mav_model::time::{DeviceTime, WallTime};
use mav_model::version::Version;
use mav_store::{Provenance, Store};
use mav_timeline::{place_on_wall, InsertOutcome as TimelineInsertOutcome, Timeline};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

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
    fn connector_operation_id(&self) -> u64 {
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
    trace_hash: u64,
}

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
            trace_hash: 0xcbf2_9ce4_8422_2325,
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
        self.lifecycle = match reason {
            CancelReason::Suspend => ConnectorLifecycle::Suspending,
            _ => ConnectorLifecycle::Disconnected,
        };
        self.dispatch(EventBody::Cancel { reason }, wall_time_ms, false, 0)
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
        }
    }

    pub fn snapshot_state(&mut self) -> Result<Vec<u8>> {
        self.program.snapshot()
    }

    fn dispatch(
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

    fn process_batch(
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

    fn validate_action(
        &self,
        action: &ConnectorAction,
        caused_by: EventSequence,
        operations: &mut BTreeSet<u64>,
        deadlines: &mut BTreeSet<u64>,
    ) -> Result<()> {
        if action.connector_id != self.connector_id
            || action.session_id.0 != self.config.session_id
            || action.caused_by != caused_by
            || action.cancellation_generation.0 != self.cancellation_generation
        {
            return Err(error(
                codes::CONNECTOR_HOST_ACTION_INVALID,
                "connector action context differs from the active event",
            ));
        }
        if action.operation_id.0 == 0
            || !operations.insert(action.operation_id.0)
            || action.deadline_token.0 == 0
            || !deadlines.insert(action.deadline_token.0)
        {
            return Err(error(
                codes::CONNECTOR_HOST_OPERATION_DUPLICATE,
                "connector operation and deadline ids must be positive and session-unique",
            ));
        }
        self.validate_declared(&action.body)
    }

    fn validate_declared(&self, body: &ActionBody) -> Result<()> {
        match body {
            ActionBody::StartScan {
                service_uuids,
                manufacturer_ids,
            } => {
                self.require_capability(TransportCapability::Scan)?;
                let declared_services = self.declared_service_uuids();
                if service_uuids
                    .iter()
                    .any(|uuid| !declared_services.contains(uuid))
                {
                    return Err(undeclared("scan names an undeclared service UUID"));
                }
                let declared_manufacturers: BTreeSet<u16> = self
                    .manifest
                    .device_families
                    .iter()
                    .filter_map(|family| family.manufacturer_id)
                    .collect();
                if manufacturer_ids
                    .iter()
                    .any(|id| !declared_manufacturers.contains(id))
                {
                    return Err(undeclared("scan names an undeclared manufacturer id"));
                }
            }
            ActionBody::Connect { address } => {
                self.require_capability(TransportCapability::Connect)?;
                if !self.advertised_addresses.contains(address) {
                    return Err(undeclared(
                        "connect address was not advertised in this session",
                    ));
                }
            }
            ActionBody::EnsurePaired => self.require_capability(TransportCapability::Pair)?,
            ActionBody::DiscoverServices => {
                self.require_capability(TransportCapability::Discover)?
            }
            ActionBody::Subscribe { characteristic_id }
            | ActionBody::Unsubscribe { characteristic_id } => {
                self.require_capability(TransportCapability::Subscribe)?;
                let characteristic = self.characteristic(characteristic_id)?;
                if !characteristic.properties.iter().any(|property| {
                    matches!(
                        property,
                        CharacteristicProperty::Notify | CharacteristicProperty::Indicate
                    )
                }) {
                    return Err(undeclared("characteristic is not subscribable"));
                }
            }
            ActionBody::Read { characteristic_id } => {
                self.require_capability(TransportCapability::Read)?;
                self.require_property(characteristic_id, CharacteristicProperty::Read)?;
            }
            ActionBody::Write {
                characteristic_id,
                confirmed,
                ..
            } => {
                self.require_capability(TransportCapability::Write)?;
                let characteristic = self.characteristic(characteristic_id)?;
                if characteristic.confirmed_write_required && !confirmed {
                    return Err(undeclared("characteristic requires confirmed writes"));
                }
                let required = if *confirmed {
                    CharacteristicProperty::Write
                } else {
                    CharacteristicProperty::WriteWithoutResponse
                };
                if !characteristic.properties.contains(&required) {
                    return Err(undeclared(
                        "characteristic does not allow the requested write",
                    ));
                }
            }
            ActionBody::DeclareCapabilities { streams } => {
                let declared: BTreeSet<&str> = self
                    .manifest
                    .capabilities
                    .iter()
                    .map(|capability| capability.stream.as_str())
                    .collect();
                if streams
                    .iter()
                    .any(|stream| !declared.contains(stream.as_str()))
                {
                    return Err(undeclared(
                        "connector declared an unsigned stream capability",
                    ));
                }
            }
            ActionBody::EmitSamples { samples, .. } => {
                for sample in samples {
                    let declared = self
                        .manifest
                        .capabilities
                        .iter()
                        .any(|capability| capability.stream == sample.stream);
                    if !declared {
                        return Err(undeclared("sample uses an undeclared stream"));
                    }
                    validate_sample(sample)?;
                }
            }
            ActionBody::StopScan
            | ActionBody::Disconnect
            | ActionBody::SetTimer { .. }
            | ActionBody::CancelTimer { .. }
            | ActionBody::StatePut { .. }
            | ActionBody::StateDelete { .. }
            | ActionBody::StateCommit
            | ActionBody::EmitDiagnostic { .. }
            | ActionBody::CompleteOperation { .. } => {}
        }
        Ok(())
    }

    fn simulate_action(&self, body: &ActionBody, lifecycle: &mut ConnectorLifecycle) -> Result<()> {
        match body {
            ActionBody::StartScan { .. }
                if matches!(
                    lifecycle,
                    ConnectorLifecycle::Selected | ConnectorLifecycle::Disconnected
                ) =>
            {
                *lifecycle = ConnectorLifecycle::Scanning;
            }
            ActionBody::StopScan if *lifecycle == ConnectorLifecycle::Scanning => {}
            ActionBody::Connect { .. } if *lifecycle == ConnectorLifecycle::Scanning => {
                *lifecycle = ConnectorLifecycle::Connecting;
            }
            ActionBody::EnsurePaired
                if matches!(
                    lifecycle,
                    ConnectorLifecycle::Connecting | ConnectorLifecycle::Configuring
                ) =>
            {
                *lifecycle = ConnectorLifecycle::Pairing;
            }
            ActionBody::DiscoverServices
                if matches!(
                    lifecycle,
                    ConnectorLifecycle::Connecting
                        | ConnectorLifecycle::Pairing
                        | ConnectorLifecycle::Configuring
                ) =>
            {
                *lifecycle = ConnectorLifecycle::Discovering;
            }
            ActionBody::Subscribe { .. }
                if matches!(
                    lifecycle,
                    ConnectorLifecycle::Discovering | ConnectorLifecycle::Configuring
                ) =>
            {
                *lifecycle = ConnectorLifecycle::Configuring;
            }
            ActionBody::Unsubscribe { .. } | ActionBody::Read { .. } | ActionBody::Write { .. }
                if matches!(
                    lifecycle,
                    ConnectorLifecycle::Configuring
                        | ConnectorLifecycle::Streaming
                        | ConnectorLifecycle::Historical
                ) => {}
            ActionBody::Disconnect
                if !matches!(
                    lifecycle,
                    ConnectorLifecycle::Installed | ConnectorLifecycle::Disconnected
                ) =>
            {
                *lifecycle = ConnectorLifecycle::Suspending;
            }
            body if !is_transport(body) => {}
            _ => {
                return Err(host_state(
                    "connector action is invalid in the current lifecycle state",
                ));
            }
        }
        Ok(())
    }

    fn execute_action(
        &mut self,
        action: ConnectorAction,
        wall_time_ms: Option<i64>,
        followups: &mut Vec<EventBody>,
    ) -> Result<()> {
        let connector_operation_id = action.operation_id.0;
        match action.body {
            ActionBody::StatePut { key, value } => {
                self.staged_state.insert(key, Some(value));
            }
            ActionBody::StateDelete { key } => {
                self.staged_state.insert(key, None);
            }
            ActionBody::StateCommit => {
                for (key, value) in std::mem::take(&mut self.staged_state) {
                    match value {
                        Some(value) => {
                            self.committed_state.insert(key, value);
                        }
                        None => {
                            self.committed_state.remove(&key);
                        }
                    }
                }
                self.state_revision = self
                    .state_revision
                    .checked_add(1)
                    .ok_or_else(|| host_state("connector state revision exhausted"))?;
                followups.push(EventBody::StateCommitted {
                    revision: self.state_revision,
                });
            }
            ActionBody::EmitSamples { batch_id, samples } => {
                let count = self.commit_samples(batch_id, &samples, wall_time_ms)?;
                followups.push(EventBody::SamplesCommitted { batch_id, count });
            }
            ActionBody::EmitDiagnostic { code, .. } => {
                let diagnostic = error(
                    codes::CONNECTOR_HOST_ACTION_INVALID,
                    "connector emitted a diagnostic",
                )
                .context(code);
                self.store.record_error(
                    &diagnostic,
                    wall_time_ms.unwrap_or_default().saturating_mul(1_000_000),
                )?;
            }
            ActionBody::DeclareCapabilities { .. } => {}
            ActionBody::CompleteOperation {
                operation_id: completed,
            } => {
                self.outstanding
                    .retain(|_, expected| expected.connector_operation_id() != completed.0);
            }
            body => {
                let operation_id = self.allocate_host_operation_id()?;
                let deadline_token = self.allocate_host_deadline_token()?;
                if let ActionBody::Read { characteristic_id } = &body {
                    self.outstanding.insert(
                        operation_id,
                        ExpectedResult::Read {
                            characteristic_id: characteristic_id.clone(),
                            connector_operation_id,
                        },
                    );
                }
                if let ActionBody::Write {
                    characteristic_id, ..
                } = &body
                {
                    self.outstanding.insert(
                        operation_id,
                        ExpectedResult::Write {
                            characteristic_id: characteristic_id.clone(),
                            connector_operation_id,
                        },
                    );
                }
                if let ActionBody::SetTimer { token, .. } = &body {
                    self.pending_timers.insert(token.0);
                }
                if let ActionBody::CancelTimer { token } = &body {
                    self.pending_timers.remove(&token.0);
                }
                self.lifecycle = transition_for_action(self.lifecycle, &body);
                self.actions.push_back(ConnectorTransportAction {
                    connector_id: self.connector_id.clone(),
                    session_id: self.config.session_id,
                    cancellation_generation: self.cancellation_generation,
                    operation_id,
                    deadline_token,
                    body: transport_request(body)?,
                });
            }
        }
        Ok(())
    }

    fn allocate_host_operation_id(&mut self) -> Result<u64> {
        let value = self.next_host_operation_id;
        self.next_host_operation_id = value
            .checked_add(1)
            .ok_or_else(|| host_state("host operation id exhausted"))?;
        Ok(value)
    }

    fn validate_state_batch(&self, batch: &ActionBatch) -> Result<()> {
        let mut staged = self.staged_state.clone();
        let mut committed = self.committed_state.clone();
        for action in &batch.actions {
            match &action.body {
                ActionBody::StatePut { key, value } => {
                    staged.insert(key.clone(), Some(value.clone()));
                }
                ActionBody::StateDelete { key } => {
                    staged.insert(key.clone(), None);
                }
                ActionBody::StateCommit => {
                    for (key, value) in std::mem::take(&mut staged) {
                        match value {
                            Some(value) => {
                                committed.insert(key, value);
                            }
                            None => {
                                committed.remove(&key);
                            }
                        }
                    }
                    if state_bytes(&committed) > MAX_STATE_BYTES {
                        return Err(error(
                            codes::CONNECTOR_HOST_ACTION_INVALID,
                            "connector committed state exceeds the session bound",
                        ));
                    }
                }
                _ => {}
            }
            if staged_state_bytes(&staged) > MAX_STATE_BYTES {
                return Err(error(
                    codes::CONNECTOR_HOST_ACTION_INVALID,
                    "connector staged state exceeds the session bound",
                ));
            }
        }
        Ok(())
    }

    fn allocate_host_deadline_token(&mut self) -> Result<u64> {
        let value = self.next_host_deadline_token;
        self.next_host_deadline_token = value
            .checked_add(1)
            .ok_or_else(|| host_state("host deadline token exhausted"))?;
        Ok(value)
    }

    fn accept_event(&mut self, body: &mut EventBody, wall_time_ms: Option<i64>) -> Result<bool> {
        let allowed = match body {
            EventBody::Advertisement { address, .. }
                if self.lifecycle == ConnectorLifecycle::Scanning =>
            {
                self.advertised_addresses.insert(address.clone());
                if self.advertised_addresses.len() > MAX_ADVERTISED_ADDRESSES {
                    return Err(host_state(
                        "connector session advertisement budget exhausted",
                    ));
                }
                true
            }
            EventBody::Connected { .. } if self.lifecycle == ConnectorLifecycle::Connecting => true,
            EventBody::PairingResult { .. } if self.lifecycle == ConnectorLifecycle::Pairing => {
                self.lifecycle = ConnectorLifecycle::Configuring;
                true
            }
            EventBody::ServicesDiscovered { .. }
                if self.lifecycle == ConnectorLifecycle::Discovering =>
            {
                self.lifecycle = ConnectorLifecycle::Configuring;
                true
            }
            EventBody::Subscribed { characteristic_id }
                if self.lifecycle == ConnectorLifecycle::Configuring =>
            {
                self.characteristic(characteristic_id)?;
                self.lifecycle = ConnectorLifecycle::Streaming;
                true
            }
            EventBody::Unsubscribed { characteristic_id }
                if matches!(
                    self.lifecycle,
                    ConnectorLifecycle::Configuring | ConnectorLifecycle::Streaming
                ) =>
            {
                self.characteristic(characteristic_id)?;
                true
            }
            EventBody::Notification {
                characteristic_id, ..
            } if matches!(
                self.lifecycle,
                ConnectorLifecycle::Streaming | ConnectorLifecycle::Historical
            ) =>
            {
                self.characteristic(characteristic_id)?;
                true
            }
            EventBody::ReadResult {
                operation_id,
                characteristic_id,
                ..
            } => self.take_expected(operation_id, characteristic_id, true, wall_time_ms)?,
            EventBody::WriteResult {
                operation_id,
                characteristic_id,
            } => self.take_expected(operation_id, characteristic_id, false, wall_time_ms)?,
            EventBody::TimerFired { token } => {
                if self.pending_timers.remove(&token.0) {
                    true
                } else {
                    self.record_late("late or cancelled timer result", wall_time_ms)?;
                    false
                }
            }
            EventBody::TransportError {
                operation_id: Some(operation_id),
                ..
            } => {
                if let Some(expected) = self.outstanding.remove(&operation_id.0) {
                    operation_id.0 = expected.connector_operation_id();
                    true
                } else {
                    self.record_late("late transport error", wall_time_ms)?;
                    false
                }
            }
            EventBody::TransportError {
                operation_id: None, ..
            } => true,
            EventBody::Disconnected { .. } => {
                self.cancellation_generation = self
                    .cancellation_generation
                    .checked_add(1)
                    .ok_or_else(|| host_state("connector cancellation generation exhausted"))?;
                self.actions.clear();
                self.outstanding.clear();
                self.pending_timers.clear();
                self.lifecycle = ConnectorLifecycle::Disconnected;
                true
            }
            EventBody::ScanStopped { .. } if self.lifecycle == ConnectorLifecycle::Scanning => true,
            EventBody::MtuChanged { .. }
                if matches!(
                    self.lifecycle,
                    ConnectorLifecycle::Connecting
                        | ConnectorLifecycle::Configuring
                        | ConnectorLifecycle::Streaming
                ) =>
            {
                true
            }
            _ => {
                return Err(host_state(
                    "transport event is invalid in the current lifecycle state",
                ));
            }
        };
        Ok(allowed)
    }

    fn take_expected(
        &mut self,
        operation_id: &mut OperationId,
        characteristic_id: &str,
        read: bool,
        wall_time_ms: Option<i64>,
    ) -> Result<bool> {
        let Some(expected) = self.outstanding.get(&operation_id.0) else {
            self.record_late("late or cancelled transport result", wall_time_ms)?;
            return Ok(false);
        };
        let matches = match expected {
            ExpectedResult::Read {
                characteristic_id: expected,
                ..
            } => read && expected == characteristic_id,
            ExpectedResult::Write {
                characteristic_id: expected,
                ..
            } => !read && expected == characteristic_id,
        };
        if !matches {
            return Err(error(
                codes::CONNECTOR_HOST_RESULT_MISMATCH,
                "transport result differs from its pending operation",
            ));
        }
        let expected = self
            .outstanding
            .remove(&operation_id.0)
            .ok_or_else(|| host_state("pending operation disappeared during result mapping"))?;
        operation_id.0 = expected.connector_operation_id();
        Ok(true)
    }

    fn commit_samples(
        &mut self,
        batch_id: BatchId,
        samples: &[WireSample],
        wall_time_ms: Option<i64>,
    ) -> Result<u32> {
        let wall_ms = wall_time_ms.ok_or_else(|| {
            error(
                codes::CONNECTOR_HOST_SAMPLE_INVALID,
                "sample emission requires an explicit host wall time",
            )
        })?;
        let wall = WallTime::from_nanos(ms_to_ns(wall_ms)?);
        let mut provenance = Vec::with_capacity(samples.len());
        for (index, sample) in samples.iter().enumerate() {
            let (kind, unit) = stream_contract(&sample.stream)?;
            if sample.unit != unit {
                return Err(error(
                    codes::CONNECTOR_HOST_SAMPLE_INVALID,
                    "connector sample unit differs from the pipeline stream contract",
                ));
            }
            let device_ms = sample.device_time_ms.ok_or_else(|| {
                error(
                    codes::CONNECTOR_HOST_SAMPLE_INVALID,
                    "connector sample has no device timestamp",
                )
            })?;
            let sequence = u16::try_from(sample.sequence).map_err(|_| {
                error(
                    codes::CONNECTOR_HOST_SAMPLE_INVALID,
                    "connector sample sequence exceeds pipeline width",
                )
            })?;
            let metadata = MetadataId::new(metadata_id(
                self.config.session_id,
                batch_id.0,
                index as u64,
            ));
            let batch = RawSampleBatch {
                device: DeviceId::new(self.config.device_id),
                samples: vec![RawSample {
                    kind,
                    device_time: DeviceTime::from_nanos(ms_to_ns(device_ms)?),
                    seq: sequence,
                    value: RawValue::Converted(sample.value_microunits as f64 / 1_000_000.0),
                }],
            };
            let mut scored = mav_sqi::score_batch(&batch, metadata);
            let mut scored = scored.pop().ok_or_else(|| {
                error(
                    codes::CONNECTOR_HOST_SAMPLE_INVALID,
                    "signal-quality stage returned no connector sample",
                )
            })?;
            provenance.push(Provenance {
                metadata,
                source_stream: kind,
                quality: scored.quality.score,
                algorithm_id: "connector-abi-v1".to_owned(),
                algorithm_version: Version::new(1, 0, 0),
                sample_count: 1,
            });
            place_on_wall(&mut scored, wall);
            let _ = self.timeline.insert(scored) == TimelineInsertOutcome::Duplicate;
        }
        let device = DeviceId::new(self.config.device_id);
        let ordered = self.timeline.drain_ordered();
        self.store.in_transaction(|store| {
            for record in &provenance {
                store.upsert_provenance(record)?;
            }
            for sample in &ordered {
                let _ = store.insert_sample(device, sample)?;
            }
            Ok(())
        })?;
        u32::try_from(samples.len()).map_err(|_| {
            error(
                codes::CONNECTOR_HOST_SAMPLE_INVALID,
                "connector sample acknowledgment count exceeds ABI width",
            )
        })
    }

    fn record_late(&self, message: &str, wall_time_ms: Option<i64>) -> Result<()> {
        self.store.record_error(
            &error(codes::CONNECTOR_HOST_LATE_RESULT, message),
            wall_time_ms.unwrap_or_default().saturating_mul(1_000_000),
        )
    }

    fn require_capability(&self, required: TransportCapability) -> Result<()> {
        if self
            .manifest
            .capabilities
            .iter()
            .any(|capability| capability.transport.contains(&required))
        {
            Ok(())
        } else {
            Err(undeclared(
                "transport capability is not signed in the manifest",
            ))
        }
    }

    fn declared_service_uuids(&self) -> BTreeSet<String> {
        self.manifest
            .services
            .iter()
            .map(|service| service.uuid.clone())
            .chain(
                self.manifest
                    .device_families
                    .iter()
                    .flat_map(|family| family.service_uuids.clone()),
            )
            .collect()
    }

    fn characteristic(&self, id: &str) -> Result<&CharacteristicDecl> {
        self.manifest
            .services
            .iter()
            .flat_map(|service| &service.characteristics)
            .find(|characteristic| characteristic.id == id)
            .ok_or_else(|| undeclared("action names an undeclared characteristic"))
    }

    pub fn characteristic_address(&self, id: &str) -> Option<(String, String)> {
        self.manifest.services.iter().find_map(|service| {
            service
                .characteristics
                .iter()
                .find(|characteristic| characteristic.id == id)
                .map(|characteristic| (service.uuid.clone(), characteristic.uuid.clone()))
        })
    }

    fn require_property(&self, id: &str, property: CharacteristicProperty) -> Result<()> {
        if self.characteristic(id)?.properties.contains(&property) {
            Ok(())
        } else {
            Err(undeclared(
                "characteristic property is not signed in the manifest",
            ))
        }
    }
}

fn transition_for_action(current: ConnectorLifecycle, body: &ActionBody) -> ConnectorLifecycle {
    match body {
        ActionBody::StartScan { .. } => ConnectorLifecycle::Scanning,
        ActionBody::Connect { .. } => ConnectorLifecycle::Connecting,
        ActionBody::EnsurePaired => ConnectorLifecycle::Pairing,
        ActionBody::DiscoverServices => ConnectorLifecycle::Discovering,
        ActionBody::Subscribe { .. } => ConnectorLifecycle::Configuring,
        ActionBody::Disconnect => ConnectorLifecycle::Suspending,
        _ => current,
    }
}

fn is_transport(body: &ActionBody) -> bool {
    matches!(
        body,
        ActionBody::StartScan { .. }
            | ActionBody::StopScan
            | ActionBody::Connect { .. }
            | ActionBody::EnsurePaired
            | ActionBody::DiscoverServices
            | ActionBody::Subscribe { .. }
            | ActionBody::Unsubscribe { .. }
            | ActionBody::Read { .. }
            | ActionBody::Write { .. }
            | ActionBody::Disconnect
            | ActionBody::SetTimer { .. }
            | ActionBody::CancelTimer { .. }
    )
}

fn transport_request(body: ActionBody) -> Result<ConnectorTransportRequest> {
    let request = match body {
        ActionBody::StartScan {
            service_uuids,
            manufacturer_ids,
        } => ConnectorTransportRequest::StartScan {
            service_uuids,
            manufacturer_ids,
        },
        ActionBody::StopScan => ConnectorTransportRequest::StopScan,
        ActionBody::Connect { address } => ConnectorTransportRequest::Connect { address },
        ActionBody::EnsurePaired => ConnectorTransportRequest::EnsurePaired,
        ActionBody::DiscoverServices => ConnectorTransportRequest::DiscoverServices,
        ActionBody::Subscribe { characteristic_id } => {
            ConnectorTransportRequest::Subscribe { characteristic_id }
        }
        ActionBody::Unsubscribe { characteristic_id } => {
            ConnectorTransportRequest::Unsubscribe { characteristic_id }
        }
        ActionBody::Read { characteristic_id } => {
            ConnectorTransportRequest::Read { characteristic_id }
        }
        ActionBody::Write {
            characteristic_id,
            bytes,
            confirmed,
        } => ConnectorTransportRequest::Write {
            characteristic_id,
            bytes,
            confirmed,
        },
        ActionBody::Disconnect => ConnectorTransportRequest::Disconnect,
        ActionBody::SetTimer { token, delay_ms } => ConnectorTransportRequest::SetTimer {
            token: token.0,
            delay_ms,
        },
        ActionBody::CancelTimer { token } => {
            ConnectorTransportRequest::CancelTimer { token: token.0 }
        }
        _ => {
            return Err(error(
                codes::CONNECTOR_HOST_ACTION_INVALID,
                "non-transport action reached the transport queue",
            ));
        }
    };
    Ok(request)
}

fn state_bytes(values: &BTreeMap<String, Vec<u8>>) -> usize {
    values.iter().fold(0_usize, |total, (key, value)| {
        total.saturating_add(key.len()).saturating_add(value.len())
    })
}

fn staged_state_bytes(values: &BTreeMap<String, Option<Vec<u8>>>) -> usize {
    values.iter().fold(0_usize, |total, (key, value)| {
        total
            .saturating_add(key.len())
            .saturating_add(value.as_deref().map_or(0, <[u8]>::len))
    })
}

fn stream_contract(value: &str) -> Result<(StreamKind, &'static str)> {
    match value {
        "heart-rate" => Ok((StreamKind::HeartRate, "beats-per-minute")),
        "rr-interval" => Ok((StreamKind::RrInterval, "milliseconds")),
        "ppg" => Ok((StreamKind::Ppg, "counts")),
        "optical-raw" => Ok((StreamKind::OpticalRaw, "counts")),
        "imu" => Ok((StreamKind::Imu, "milli-g")),
        "gyro" => Ok((StreamKind::Gyro, "milli-degrees-per-second")),
        "gravity" => Ok((StreamKind::Gravity, "milli-g")),
        "skin-temp" => Ok((StreamKind::SkinTemp, "degrees-celsius")),
        "spo2-raw" => Ok((StreamKind::Spo2Raw, "counts")),
        "spo2-percent" => Ok((StreamKind::Spo2Percent, "percent")),
        "resp-raw" => Ok((StreamKind::RespRaw, "counts")),
        "battery-soc" => Ok((StreamKind::BatterySoc, "percent")),
        "step-count" => Ok((StreamKind::StepCount, "count")),
        "activity-class" => Ok((StreamKind::ActivityClass, "code")),
        "skin-contact" => Ok((StreamKind::SkinContact, "boolean")),
        "signal-quality" => Ok((StreamKind::SignalQuality, "percent")),
        "wrist-state" => Ok((StreamKind::WristState, "boolean")),
        "sleep-state-raw" => Ok((StreamKind::SleepStateRaw, "code")),
        _ => Err(error(
            codes::CONNECTOR_HOST_SAMPLE_INVALID,
            "connector sample stream is not admitted by the pipeline",
        )),
    }
}

fn validate_sample(sample: &WireSample) -> Result<()> {
    let (_, unit) = stream_contract(&sample.stream)?;
    if sample.unit != unit {
        return Err(error(
            codes::CONNECTOR_HOST_SAMPLE_INVALID,
            "connector sample unit differs from the pipeline stream contract",
        ));
    }
    if sample.device_time_ms.is_none() || sample.sequence > u32::from(u16::MAX) {
        return Err(error(
            codes::CONNECTOR_HOST_SAMPLE_INVALID,
            "connector sample timestamp or sequence is outside pipeline bounds",
        ));
    }
    Ok(())
}

fn metadata_id(session_id: u64, batch_id: u64, index: u64) -> u64 {
    let mut value = 0xcbf2_9ce4_8422_2325_u64;
    for part in [session_id, batch_id, index] {
        for byte in part.to_le_bytes() {
            value ^= u64::from(byte);
            value = value.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    value | (1_u64 << 63)
}

fn trace_event(hash: u64, value: &ConnectorEvent) -> Result<u64> {
    let bytes = mav_connector_abi::encode_canonical(value).map_err(|source| {
        error(
            codes::CONNECTOR_HOST_ACTION_INVALID,
            format!("connector trace value is not canonical: {source}"),
        )
    })?;
    Ok(trace_bytes(hash, bytes))
}

fn trace_action(hash: u64, value: &ConnectorAction) -> Result<u64> {
    let bytes = mav_connector_abi::encode_canonical(value).map_err(|source| {
        error(
            codes::CONNECTOR_HOST_ACTION_INVALID,
            format!("connector trace value is not canonical: {source}"),
        )
    })?;
    Ok(trace_bytes(hash, bytes))
}

fn trace_bytes(hash: u64, bytes: Vec<u8>) -> u64 {
    bytes.into_iter().fold(hash, |current, byte| {
        (current ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
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
mod tests {
    use super::*;
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

    fn manifest() -> Manifest {
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

    fn action(cause: u64, operation: u64, body: ActionBody) -> ConnectorAction {
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

    fn batch(actions: Vec<ConnectorAction>) -> ActionBatch {
        ActionBatch { actions }
    }

    fn empty() -> ActionBatch {
        batch(Vec::new())
    }

    fn host(batches: Vec<ActionBatch>, capacity: u32) -> ConnectorHost {
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

    fn advertisement() -> EventBody {
        EventBody::Advertisement {
            address: "native-device".to_owned(),
            rssi: -42,
            service_uuids: vec!["service".to_owned()],
            manufacturer_data: Vec::new(),
            name: Some("Test".to_owned()),
        }
    }

    #[test]
    fn forced_termination_journals_hostile_cancel_failure() {
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
    fn lifecycle_script_is_ordered_and_device_neutral() {
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
                empty(),
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
        assert_eq!(host.lifecycle_snapshot().trace_hash, "09b6ce81d8da683f");
    }

    #[test]
    fn transport_ids_are_host_assigned() {
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
    fn wrong_order_and_undeclared_actions_reject_exactly() {
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
    fn queue_bound_is_atomic_and_cancelled_results_are_logged_and_ignored() {
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
    fn samples_are_durable_before_a_later_write_is_visible() {
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
    fn duplicate_samples_acknowledge_as_durable_without_duplicate_rows() {
        let sample = WireSample {
            stream: "heart-rate".to_owned(),
            value_microunits: 63_000_000,
            device_time_ms: Some(1_000),
            sequence: 0,
            unit: "beats-per-minute".to_owned(),
        };
        let mut host = host(Vec::new(), 8);
        assert_eq!(
            host.commit_samples(BatchId(1), std::slice::from_ref(&sample), Some(2_000)),
            Ok(1)
        );
        assert_eq!(
            host.commit_samples(BatchId(2), std::slice::from_ref(&sample), Some(2_000)),
            Ok(1)
        );
        assert_eq!(
            host.store
                .samples(DeviceId::new(9), StreamKind::HeartRate)
                .expect("stored")
                .len(),
            1
        );
    }
}
