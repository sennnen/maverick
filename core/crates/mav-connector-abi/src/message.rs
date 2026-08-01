use crate::{bounds, *};
use minicbor::{Decode, Encode};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(index_only)]
pub enum CancelReason {
    #[n(0)]
    User,
    #[n(1)]
    Platform,
    #[n(2)]
    Disconnect,
    #[n(3)]
    Suspend,
    #[n(4)]
    Update,
    #[n(5)]
    Removal,
}

impl Validate for CancelReason {
    fn validate(&self) -> Result<(), WireError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(index_only)]
pub enum DiagnosticLevel {
    #[n(0)]
    Debug,
    #[n(1)]
    Info,
    #[n(2)]
    Warning,
    #[n(3)]
    Error,
}

impl Validate for DiagnosticLevel {
    fn validate(&self) -> Result<(), WireError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(index_only)]
pub enum ConnectorLifecycle {
    #[n(0)]
    Installed,
    #[n(1)]
    Selected,
    #[n(2)]
    Scanning,
    #[n(3)]
    Connecting,
    #[n(4)]
    Discovering,
    #[n(5)]
    Pairing,
    #[n(6)]
    Configuring,
    #[n(7)]
    Streaming,
    #[n(8)]
    Historical,
    #[n(9)]
    Suspending,
    #[n(10)]
    Disconnected,
    #[n(11)]
    Failed,
}

impl Validate for ConnectorLifecycle {
    fn validate(&self) -> Result<(), WireError> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct WireSample {
    #[n(0)]
    pub stream: String,
    #[n(1)]
    pub value_microunits: i64,
    #[n(2)]
    pub device_time_ms: Option<i64>,
    #[n(3)]
    pub sequence: u32,
    #[n(4)]
    pub unit: String,
}

impl Validate for WireSample {
    fn validate(&self) -> Result<(), WireError> {
        bounds::identifier(&self.stream, bounds::MAX_LOGICAL_ID_BYTES, "sample stream")?;
        bounds::identifier(&self.unit, bounds::MAX_LOGICAL_ID_BYTES, "sample unit")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct ConnectorEvent {
    #[n(0)]
    pub connector_id: ConnectorId,
    #[n(1)]
    pub session_id: SessionId,
    #[n(2)]
    pub sequence: EventSequence,
    #[n(3)]
    pub cancellation_generation: CancellationGeneration,
    #[n(4)]
    pub wall_time_ms: Option<i64>,
    #[n(5)]
    pub body: EventBody,
}

impl Validate for ConnectorEvent {
    fn validate(&self) -> Result<(), WireError> {
        self.connector_id.validate()?;
        self.body.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub enum EventBody {
    #[n(0)]
    Init {
        #[n(0)]
        #[cbor(with = "minicbor::bytes")]
        manifest_hash: [u8; 32],
    },
    #[n(1)]
    Activate,
    #[n(2)]
    Deactivate,
    #[n(3)]
    Suspend,
    #[n(4)]
    Resume,
    #[n(5)]
    Cancel {
        #[n(0)]
        reason: CancelReason,
    },
    #[n(6)]
    RestoreState {
        #[n(0)]
        #[cbor(with = "minicbor::bytes")]
        bytes: Vec<u8>,
    },
    #[n(7)]
    Advertisement {
        #[n(0)]
        address: String,
        #[n(1)]
        rssi: i16,
        #[n(2)]
        service_uuids: Vec<String>,
        #[n(3)]
        #[cbor(with = "minicbor::bytes")]
        manufacturer_data: Vec<u8>,
        #[n(4)]
        name: Option<String>,
    },
    #[n(8)]
    ScanStopped {
        #[n(0)]
        reason_code: u16,
    },
    #[n(9)]
    ServicesDiscovered {
        #[n(0)]
        service_uuids: Vec<String>,
    },
    #[n(10)]
    IdentityRead {
        #[n(0)]
        field_id: String,
        #[n(1)]
        #[cbor(with = "minicbor::bytes")]
        bytes: Vec<u8>,
    },
    #[n(11)]
    Connected {
        #[n(0)]
        mtu: u16,
    },
    #[n(12)]
    PairingResult {
        #[n(0)]
        success: bool,
        #[n(1)]
        error_code: Option<u16>,
    },
    #[n(13)]
    MtuChanged {
        #[n(0)]
        mtu: u16,
    },
    #[n(14)]
    Subscribed {
        #[n(0)]
        characteristic_id: String,
    },
    #[n(15)]
    Unsubscribed {
        #[n(0)]
        characteristic_id: String,
    },
    #[n(16)]
    ReadResult {
        #[n(0)]
        operation_id: OperationId,
        #[n(1)]
        characteristic_id: String,
        #[n(2)]
        #[cbor(with = "minicbor::bytes")]
        bytes: Vec<u8>,
    },
    #[n(17)]
    WriteResult {
        #[n(0)]
        operation_id: OperationId,
        #[n(1)]
        characteristic_id: String,
    },
    #[n(18)]
    Notification {
        #[n(0)]
        characteristic_id: String,
        #[n(1)]
        #[cbor(with = "minicbor::bytes")]
        bytes: Vec<u8>,
    },
    #[n(19)]
    Disconnected {
        #[n(0)]
        reason_code: u16,
    },
    #[n(20)]
    TransportError {
        #[n(0)]
        operation_id: Option<OperationId>,
        #[n(1)]
        code: u16,
        #[n(2)]
        message: String,
    },
    #[n(21)]
    TimerFired {
        #[n(0)]
        token: TimerToken,
    },
    #[n(22)]
    StateCommitted {
        #[n(0)]
        revision: u64,
    },
    #[n(23)]
    SamplesCommitted {
        #[n(0)]
        batch_id: BatchId,
        #[n(1)]
        count: u32,
    },
    #[n(24)]
    SamplesRejected {
        #[n(0)]
        batch_id: BatchId,
        #[n(1)]
        code: u16,
    },
    #[n(25)]
    PrepareStateMigration {
        #[n(0)]
        from_schema: u32,
        #[n(1)]
        to_schema: u32,
        #[n(2)]
        #[cbor(with = "minicbor::bytes")]
        state: Vec<u8>,
    },
    #[n(26)]
    StateMigrationCommitted {
        #[n(0)]
        schema: u32,
    },
    /// The host's power policy changed. A connector is expected to trade data density for battery
    /// when `low_power` is set — longer offload cadence, no optional diagnostic subscriptions — and
    /// to keep its primary vitals stream working either way. Delivered on activation and whenever
    /// the user changes the setting, so a connector never has to ask. See ADR-030.
    #[n(27)]
    PowerModeChanged {
        #[n(0)]
        low_power: bool,
    },
    /// Begin a host-owned captured-waveform flow for a signed and session-active stream.
    #[n(28)]
    CaptureStart {
        #[n(0)]
        stream: String,
    },
    /// Stop a previously started captured-waveform flow.
    #[n(29)]
    CaptureStop {
        #[n(0)]
        stream: String,
    },
}

fn logical_id(value: &str, field: &'static str) -> Result<(), WireError> {
    bounds::identifier(value, bounds::MAX_LOGICAL_ID_BYTES, field)
}

fn uuids(values: &[String], field: &'static str) -> Result<(), WireError> {
    bounds::count(values.len(), bounds::MAX_SCAN_FILTERS, field)?;
    for value in values {
        bounds::text(value, bounds::MAX_UUID_BYTES, field)?;
    }
    Ok(())
}

impl Validate for EventBody {
    fn validate(&self) -> Result<(), WireError> {
        match self {
            Self::Init { .. }
            | Self::Activate
            | Self::Deactivate
            | Self::Suspend
            | Self::Resume
            | Self::ScanStopped { .. }
            | Self::Connected { .. }
            | Self::PairingResult { .. }
            | Self::MtuChanged { .. }
            | Self::Disconnected { .. }
            | Self::TimerFired { .. }
            | Self::StateCommitted { .. }
            | Self::SamplesCommitted { .. }
            | Self::SamplesRejected { .. }
            | Self::StateMigrationCommitted { .. }
            | Self::PowerModeChanged { .. } => Ok(()),
            Self::CaptureStart { stream } | Self::CaptureStop { stream } => {
                logical_id(stream, "capture event stream")
            }
            Self::Cancel { reason } => reason.validate(),
            Self::RestoreState { bytes: value } => {
                bounds::bytes(value, bounds::MAX_STATE_BYTES, "restore state bytes")
            }
            Self::Advertisement {
                address,
                service_uuids,
                manufacturer_data,
                name,
                ..
            } => {
                bounds::text(address, bounds::MAX_LABEL_BYTES, "advertisement address")?;
                uuids(service_uuids, "advertisement service UUIDs")?;
                bounds::bytes(manufacturer_data, 512, "advertisement manufacturer bytes")?;
                if let Some(name) = name {
                    bounds::text(name, bounds::MAX_LABEL_BYTES, "advertisement name")?;
                }
                Ok(())
            }
            Self::ServicesDiscovered { service_uuids } => {
                uuids(service_uuids, "discovered service UUIDs")
            }
            Self::IdentityRead { field_id, bytes } => {
                logical_id(field_id, "identity field id")?;
                bounds::bytes(bytes, bounds::MAX_EVENT_BYTES, "identity bytes")
            }
            Self::Subscribed { characteristic_id }
            | Self::Unsubscribed { characteristic_id }
            | Self::WriteResult {
                characteristic_id, ..
            } => logical_id(characteristic_id, "event characteristic id"),
            Self::ReadResult {
                characteristic_id,
                bytes,
                ..
            }
            | Self::Notification {
                characteristic_id,
                bytes,
            } => {
                logical_id(characteristic_id, "event characteristic id")?;
                bounds::bytes(bytes, bounds::MAX_EVENT_BYTES, "event notification bytes")
            }
            Self::TransportError { message, .. } => bounds::text(
                message,
                bounds::MAX_DIAGNOSTIC_BYTES,
                "transport error message",
            ),
            Self::PrepareStateMigration { state, .. } => {
                bounds::bytes(state, bounds::MAX_STATE_BYTES, "migration state bytes")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct ConnectorAction {
    #[n(0)]
    pub connector_id: ConnectorId,
    #[n(1)]
    pub session_id: SessionId,
    #[n(2)]
    pub caused_by: EventSequence,
    #[n(3)]
    pub cancellation_generation: CancellationGeneration,
    #[n(4)]
    pub operation_id: OperationId,
    #[n(5)]
    pub deadline_token: TimerToken,
    #[n(6)]
    pub body: ActionBody,
}

impl Validate for ConnectorAction {
    fn validate(&self) -> Result<(), WireError> {
        self.connector_id.validate()?;
        self.body.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub enum ActionBody {
    #[n(0)]
    StartScan {
        #[n(0)]
        service_uuids: Vec<String>,
        #[n(1)]
        manufacturer_ids: Vec<u16>,
    },
    #[n(1)]
    StopScan,
    #[n(2)]
    Connect {
        #[n(0)]
        address: String,
    },
    #[n(3)]
    EnsurePaired,
    #[n(4)]
    DiscoverServices,
    #[n(5)]
    Subscribe {
        #[n(0)]
        characteristic_id: String,
    },
    #[n(6)]
    Unsubscribe {
        #[n(0)]
        characteristic_id: String,
    },
    #[n(7)]
    Read {
        #[n(0)]
        characteristic_id: String,
    },
    #[n(8)]
    Write {
        #[n(0)]
        characteristic_id: String,
        #[n(1)]
        #[cbor(with = "minicbor::bytes")]
        bytes: Vec<u8>,
        #[n(2)]
        confirmed: bool,
    },
    #[n(9)]
    Disconnect,
    #[n(10)]
    SetTimer {
        #[n(0)]
        token: TimerToken,
        #[n(1)]
        delay_ms: u64,
    },
    #[n(11)]
    CancelTimer {
        #[n(0)]
        token: TimerToken,
    },
    #[n(12)]
    StatePut {
        #[n(0)]
        key: String,
        #[n(1)]
        #[cbor(with = "minicbor::bytes")]
        value: Vec<u8>,
    },
    #[n(13)]
    StateDelete {
        #[n(0)]
        key: String,
    },
    #[n(14)]
    StateCommit,
    #[n(15)]
    EmitSamples {
        #[n(0)]
        batch_id: BatchId,
        #[n(1)]
        samples: Vec<WireSample>,
    },
    #[n(16)]
    EmitDiagnostic {
        #[n(0)]
        level: DiagnosticLevel,
        #[n(1)]
        code: String,
        #[n(2)]
        message: String,
    },
    #[n(17)]
    DeclareCapabilities {
        #[n(0)]
        streams: Vec<String>,
    },
    #[n(18)]
    CompleteOperation {
        #[n(0)]
        operation_id: OperationId,
    },
}

impl Validate for ActionBody {
    fn validate(&self) -> Result<(), WireError> {
        match self {
            Self::StartScan {
                service_uuids,
                manufacturer_ids,
            } => {
                uuids(service_uuids, "scan service UUIDs")?;
                bounds::count(
                    manufacturer_ids.len(),
                    bounds::MAX_SCAN_FILTERS,
                    "scan manufacturer ids",
                )
            }
            Self::StopScan
            | Self::EnsurePaired
            | Self::DiscoverServices
            | Self::Disconnect
            | Self::CancelTimer { .. }
            | Self::StateCommit
            | Self::CompleteOperation { .. } => Ok(()),
            Self::Connect { address } => {
                bounds::text(address, bounds::MAX_LABEL_BYTES, "connect address")
            }
            Self::Subscribe { characteristic_id }
            | Self::Unsubscribe { characteristic_id }
            | Self::Read { characteristic_id } => {
                logical_id(characteristic_id, "action characteristic id")
            }
            Self::Write {
                characteristic_id,
                bytes,
                ..
            } => {
                logical_id(characteristic_id, "action characteristic id")?;
                bounds::bytes(bytes, bounds::MAX_EVENT_BYTES, "write bytes")
            }
            Self::SetTimer { delay_ms, .. } => {
                if *delay_ms == 0 || *delay_ms > bounds::MAX_TIMER_DELAY_MS {
                    return Err(WireError::Bounds("timer delay"));
                }
                Ok(())
            }
            Self::StatePut { key, value } => {
                bounds::text(key, bounds::MAX_STATE_KEY_BYTES, "state key")?;
                bounds::bytes(value, bounds::MAX_STATE_BYTES, "state value")
            }
            Self::StateDelete { key } => {
                bounds::text(key, bounds::MAX_STATE_KEY_BYTES, "state key")
            }
            Self::EmitSamples { samples, .. } => {
                bounds::count(
                    samples.len(),
                    bounds::MAX_SAMPLES_PER_ACTION,
                    "samples per action",
                )?;
                bounds::all(samples)
            }
            Self::EmitDiagnostic {
                level,
                code,
                message,
            } => {
                level.validate()?;
                logical_id(code, "diagnostic code")?;
                bounds::text(message, bounds::MAX_DIAGNOSTIC_BYTES, "diagnostic message")
            }
            Self::DeclareCapabilities { streams } => {
                bounds::count(
                    streams.len(),
                    bounds::MAX_CAPABILITIES,
                    "declared capabilities",
                )?;
                for stream in streams {
                    logical_id(stream, "declared capability")?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct ActionBatch {
    #[n(0)]
    pub actions: Vec<ConnectorAction>,
}

impl Validate for ActionBatch {
    fn validate(&self) -> Result<(), WireError> {
        bounds::count(self.actions.len(), bounds::MAX_ACTIONS, "actions per event")?;
        bounds::all(&self.actions)
    }
}
