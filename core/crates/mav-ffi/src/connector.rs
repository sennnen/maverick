use crate::FfiError;
use mav_connector_abi::{CancelReason, ConnectorLifecycle, EventBody, OperationId, TimerToken};
use mav_connector_runtime::{
    KeyScope, KeyStatus, PublisherKey, Revocation, RevocationSet, TrustPolicy,
};
use mav_connector_store::{ConnectorSource, InstalledConnector, SourceKind};
use mav_engine::{
    ApplyOutcome, ConnectorLifecycleSnapshot as EngineLifecycleSnapshot,
    ConnectorTransportAction as EngineTransportAction,
    ConnectorTransportRequest as EngineTransportRequest,
};
use mav_model::error::{codes, MavError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectorSourceKind {
    Bundled,
    Imported,
    Remote,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ConnectorSourceMetadata {
    pub kind: ConnectorSourceKind,
    pub display_name: String,
    pub locator_digest: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectorKeyScope {
    Official,
    ThirdParty,
    Development,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectorKeyStatus {
    Active,
    Revoked,
    Rotated,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ConnectorPublisherKey {
    pub id: String,
    pub public_key: Vec<u8>,
    pub scope: ConnectorKeyScope,
    pub valid_from_ms: i64,
    pub valid_until_ms: Option<i64>,
    pub status: ConnectorKeyStatus,
    pub status_at_ms: Option<i64>,
    pub status_detail: Option<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ConnectorTrustPolicy {
    pub revision: u64,
    pub allow_third_party: bool,
    pub allow_development: bool,
    pub keys: Vec<ConnectorPublisherKey>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ConnectorRevocationRecord {
    pub publisher_key_id: String,
    pub revoked_at_ms: i64,
    pub reason: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ConnectorTrustRevocations {
    pub revision: u64,
    pub generated_at_ms: i64,
    pub valid_until_ms: i64,
    pub entries: Vec<ConnectorRevocationRecord>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ConnectorInspection {
    pub artifact_digest: Vec<u8>,
    pub manifest_digest: Vec<u8>,
    pub connector_id: String,
    pub version: String,
    pub display_name: String,
    pub description: String,
    pub publisher_key_id: String,
    pub capabilities: Vec<String>,
    pub permissions: Vec<String>,
    pub state_schema: u32,
    pub fixture_count: u32,
    pub source: ConnectorSourceMetadata,
    pub approval_token: Vec<u8>,
    pub approval_expires_at_ms: i64,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ConnectorInstallRequest {
    pub bytes: Vec<u8>,
    pub source: ConnectorSourceMetadata,
    pub approval_token: Vec<u8>,
    pub activate: bool,
    pub now_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct InstalledConnectorRecord {
    pub connector_id: String,
    pub version: String,
    pub publisher_key_id: String,
    pub state_schema: u32,
    pub artifact_digest: Vec<u8>,
    pub source: ConnectorSourceMetadata,
    pub installed_at_ms: i64,
    pub policy_revision: u64,
    pub revocation_revision: u64,
    pub fixture_count: u32,
    pub active: bool,
    pub disabled_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectorRemovalMode {
    DeleteState,
    QuarantineState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectorCancelReason {
    User,
    Platform,
    Disconnect,
    Suspend,
    Update,
    Removal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectorApplyOutcome {
    Applied,
    IgnoredLate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectorLifecycleState {
    Installed,
    Selected,
    Scanning,
    Connecting,
    Discovering,
    Pairing,
    Configuring,
    Streaming,
    Historical,
    Suspending,
    Disconnected,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ConnectorLifecycleReport {
    pub lifecycle: ConnectorLifecycleState,
    pub session_id: u64,
    pub cancellation_generation: u64,
    pub last_event_sequence: u64,
    pub queued_actions: u32,
    pub outstanding_operations: u32,
    pub state_revision: u64,
    pub trace_hash: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ConnectorSessionConfig {
    pub connector_id: String,
    pub session_id: u64,
    pub device_id: u64,
    pub transport_capacity: u32,
    pub now_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ConnectorTransportEvent {
    Advertisement {
        address: String,
        rssi: i16,
        service_uuids: Vec<String>,
        manufacturer_data: Vec<u8>,
        name: Option<String>,
    },
    ScanStopped {
        reason_code: u16,
    },
    Connected {
        mtu: u16,
    },
    PairingResult {
        success: bool,
        error_code: Option<u16>,
    },
    MtuChanged {
        mtu: u16,
    },
    ServicesDiscovered {
        service_uuids: Vec<String>,
    },
    IdentityRead {
        field_id: String,
        bytes: Vec<u8>,
    },
    Subscribed {
        characteristic_id: String,
    },
    Unsubscribed {
        characteristic_id: String,
    },
    ReadResult {
        operation_id: u64,
        characteristic_id: String,
        bytes: Vec<u8>,
    },
    WriteResult {
        operation_id: u64,
        characteristic_id: String,
    },
    Notification {
        characteristic_id: String,
        bytes: Vec<u8>,
    },
    TransportError {
        operation_id: Option<u64>,
        code: u16,
        safe_message: String,
    },
    TimerFired {
        token: u64,
    },
    Disconnected {
        reason_code: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
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
        service_uuid: String,
        characteristic_uuid: String,
    },
    Unsubscribe {
        characteristic_id: String,
        service_uuid: String,
        characteristic_uuid: String,
    },
    Read {
        characteristic_id: String,
        service_uuid: String,
        characteristic_uuid: String,
    },
    Write {
        characteristic_id: String,
        service_uuid: String,
        characteristic_uuid: String,
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

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ConnectorTransportAction {
    pub connector_id: String,
    pub session_id: u64,
    pub cancellation_generation: u64,
    pub operation_id: u64,
    pub deadline_token: u64,
    pub request: ConnectorTransportRequest,
}

pub(crate) fn source_from_ffi(
    source: ConnectorSourceMetadata,
) -> Result<ConnectorSource, FfiError> {
    Ok(ConnectorSource {
        kind: match source.kind {
            ConnectorSourceKind::Bundled => SourceKind::Bundled,
            ConnectorSourceKind::Imported => SourceKind::Imported,
            ConnectorSourceKind::Remote => SourceKind::Remote,
        },
        display_name: source.display_name,
        locator_digest: digest(&source.locator_digest, "connector source locator digest")?,
    })
}

pub(crate) fn source_to_ffi(source: ConnectorSource) -> ConnectorSourceMetadata {
    ConnectorSourceMetadata {
        kind: match source.kind {
            SourceKind::Bundled => ConnectorSourceKind::Bundled,
            SourceKind::Imported => ConnectorSourceKind::Imported,
            SourceKind::Remote => ConnectorSourceKind::Remote,
        },
        display_name: source.display_name,
        locator_digest: source.locator_digest.to_vec(),
    }
}

pub(crate) fn policy_from_ffi(policy: ConnectorTrustPolicy) -> Result<TrustPolicy, FfiError> {
    let keys = policy
        .keys
        .into_iter()
        .map(|key| {
            let status = match key.status {
                ConnectorKeyStatus::Active
                    if key.status_at_ms.is_none() && key.status_detail.is_none() =>
                {
                    KeyStatus::Active
                }
                ConnectorKeyStatus::Revoked => KeyStatus::Revoked {
                    at_ms: key.status_at_ms.ok_or_else(invalid_trust)?,
                    reason: key.status_detail.ok_or_else(invalid_trust)?,
                },
                ConnectorKeyStatus::Rotated => KeyStatus::Rotated {
                    at_ms: key.status_at_ms.ok_or_else(invalid_trust)?,
                    replacement_id: key.status_detail.ok_or_else(invalid_trust)?,
                },
                ConnectorKeyStatus::Active => return Err(invalid_trust()),
            };
            Ok(PublisherKey {
                id: key.id,
                public_key: digest(&key.public_key, "connector publisher public key")?,
                scope: match key.scope {
                    ConnectorKeyScope::Official => KeyScope::Official,
                    ConnectorKeyScope::ThirdParty => KeyScope::ThirdParty,
                    ConnectorKeyScope::Development => KeyScope::Development,
                },
                valid_from_ms: key.valid_from_ms,
                valid_until_ms: key.valid_until_ms,
                status,
            })
        })
        .collect::<Result<Vec<_>, FfiError>>()?;
    Ok(TrustPolicy {
        revision: policy.revision,
        allow_third_party: policy.allow_third_party,
        allow_development: policy.allow_development,
        keys,
    })
}

pub(crate) fn revocations_from_ffi(value: ConnectorTrustRevocations) -> RevocationSet {
    RevocationSet {
        revision: value.revision,
        generated_at_ms: value.generated_at_ms,
        valid_until_ms: value.valid_until_ms,
        entries: value
            .entries
            .into_iter()
            .map(|entry| Revocation {
                publisher_key_id: entry.publisher_key_id,
                revoked_at_ms: entry.revoked_at_ms,
                reason: entry.reason,
            })
            .collect(),
    }
}

pub(crate) fn installed_to_ffi(value: InstalledConnector) -> InstalledConnectorRecord {
    InstalledConnectorRecord {
        connector_id: value.connector_id,
        version: value.version,
        publisher_key_id: value.publisher_key_id,
        state_schema: value.state_schema,
        artifact_digest: value.artifact_digest.to_vec(),
        source: source_to_ffi(value.source),
        installed_at_ms: value.installed_at_ms,
        policy_revision: value.policy_revision,
        revocation_revision: value.revocation_revision,
        fixture_count: value.fixture_count,
        active: value.active,
        disabled_reason: value.disabled_reason,
    }
}

pub(crate) fn event_from_ffi(event: ConnectorTransportEvent) -> EventBody {
    match event {
        ConnectorTransportEvent::Advertisement {
            address,
            rssi,
            service_uuids,
            manufacturer_data,
            name,
        } => EventBody::Advertisement {
            address,
            rssi,
            service_uuids,
            manufacturer_data,
            name,
        },
        ConnectorTransportEvent::ScanStopped { reason_code } => {
            EventBody::ScanStopped { reason_code }
        }
        ConnectorTransportEvent::Connected { mtu } => EventBody::Connected { mtu },
        ConnectorTransportEvent::PairingResult {
            success,
            error_code,
        } => EventBody::PairingResult {
            success,
            error_code,
        },
        ConnectorTransportEvent::MtuChanged { mtu } => EventBody::MtuChanged { mtu },
        ConnectorTransportEvent::ServicesDiscovered { service_uuids } => {
            EventBody::ServicesDiscovered { service_uuids }
        }
        ConnectorTransportEvent::IdentityRead { field_id, bytes } => {
            EventBody::IdentityRead { field_id, bytes }
        }
        ConnectorTransportEvent::Subscribed { characteristic_id } => {
            EventBody::Subscribed { characteristic_id }
        }
        ConnectorTransportEvent::Unsubscribed { characteristic_id } => {
            EventBody::Unsubscribed { characteristic_id }
        }
        ConnectorTransportEvent::ReadResult {
            operation_id,
            characteristic_id,
            bytes,
        } => EventBody::ReadResult {
            operation_id: OperationId(operation_id),
            characteristic_id,
            bytes,
        },
        ConnectorTransportEvent::WriteResult {
            operation_id,
            characteristic_id,
        } => EventBody::WriteResult {
            operation_id: OperationId(operation_id),
            characteristic_id,
        },
        ConnectorTransportEvent::Notification {
            characteristic_id,
            bytes,
        } => EventBody::Notification {
            characteristic_id,
            bytes,
        },
        ConnectorTransportEvent::TransportError {
            operation_id,
            code,
            safe_message,
        } => EventBody::TransportError {
            operation_id: operation_id.map(OperationId),
            code,
            message: safe_message,
        },
        ConnectorTransportEvent::TimerFired { token } => EventBody::TimerFired {
            token: TimerToken(token),
        },
        ConnectorTransportEvent::Disconnected { reason_code } => {
            EventBody::Disconnected { reason_code }
        }
    }
}

pub(crate) fn cancel_from_ffi(reason: ConnectorCancelReason) -> CancelReason {
    match reason {
        ConnectorCancelReason::User => CancelReason::User,
        ConnectorCancelReason::Platform => CancelReason::Platform,
        ConnectorCancelReason::Disconnect => CancelReason::Disconnect,
        ConnectorCancelReason::Suspend => CancelReason::Suspend,
        ConnectorCancelReason::Update => CancelReason::Update,
        ConnectorCancelReason::Removal => CancelReason::Removal,
    }
}

impl From<ApplyOutcome> for ConnectorApplyOutcome {
    fn from(value: ApplyOutcome) -> Self {
        match value {
            ApplyOutcome::Applied => Self::Applied,
            ApplyOutcome::IgnoredLate => Self::IgnoredLate,
        }
    }
}

impl From<EngineLifecycleSnapshot> for ConnectorLifecycleReport {
    fn from(value: EngineLifecycleSnapshot) -> Self {
        Self {
            lifecycle: match value.lifecycle {
                ConnectorLifecycle::Installed => ConnectorLifecycleState::Installed,
                ConnectorLifecycle::Selected => ConnectorLifecycleState::Selected,
                ConnectorLifecycle::Scanning => ConnectorLifecycleState::Scanning,
                ConnectorLifecycle::Connecting => ConnectorLifecycleState::Connecting,
                ConnectorLifecycle::Discovering => ConnectorLifecycleState::Discovering,
                ConnectorLifecycle::Pairing => ConnectorLifecycleState::Pairing,
                ConnectorLifecycle::Configuring => ConnectorLifecycleState::Configuring,
                ConnectorLifecycle::Streaming => ConnectorLifecycleState::Streaming,
                ConnectorLifecycle::Historical => ConnectorLifecycleState::Historical,
                ConnectorLifecycle::Suspending => ConnectorLifecycleState::Suspending,
                ConnectorLifecycle::Disconnected => ConnectorLifecycleState::Disconnected,
                ConnectorLifecycle::Failed => ConnectorLifecycleState::Failed,
            },
            session_id: value.session_id,
            cancellation_generation: value.cancellation_generation,
            last_event_sequence: value.last_event_sequence,
            queued_actions: value.queued_actions,
            outstanding_operations: value.outstanding_operations,
            state_revision: value.state_revision,
            trace_hash: value.trace_hash,
        }
    }
}

pub(crate) fn transport_action_to_ffi(
    value: EngineTransportAction,
    characteristic_address: Option<(String, String)>,
) -> Result<ConnectorTransportAction, FfiError> {
    let request = match value.body {
        EngineTransportRequest::StartScan {
            service_uuids,
            manufacturer_ids,
        } => ConnectorTransportRequest::StartScan {
            service_uuids,
            manufacturer_ids,
        },
        EngineTransportRequest::StopScan => ConnectorTransportRequest::StopScan,
        EngineTransportRequest::Connect { address } => {
            ConnectorTransportRequest::Connect { address }
        }
        EngineTransportRequest::EnsurePaired => ConnectorTransportRequest::EnsurePaired,
        EngineTransportRequest::DiscoverServices => ConnectorTransportRequest::DiscoverServices,
        EngineTransportRequest::Subscribe { characteristic_id } => {
            let (service_uuid, characteristic_uuid) = characteristic_address
                .ok_or_else(|| invalid_transport_mapping(&characteristic_id))?;
            ConnectorTransportRequest::Subscribe {
                characteristic_id,
                service_uuid,
                characteristic_uuid,
            }
        }
        EngineTransportRequest::Unsubscribe { characteristic_id } => {
            let (service_uuid, characteristic_uuid) = characteristic_address
                .ok_or_else(|| invalid_transport_mapping(&characteristic_id))?;
            ConnectorTransportRequest::Unsubscribe {
                characteristic_id,
                service_uuid,
                characteristic_uuid,
            }
        }
        EngineTransportRequest::Read { characteristic_id } => {
            let (service_uuid, characteristic_uuid) = characteristic_address
                .ok_or_else(|| invalid_transport_mapping(&characteristic_id))?;
            ConnectorTransportRequest::Read {
                characteristic_id,
                service_uuid,
                characteristic_uuid,
            }
        }
        EngineTransportRequest::Write {
            characteristic_id,
            bytes,
            confirmed,
        } => {
            let (service_uuid, characteristic_uuid) = characteristic_address
                .ok_or_else(|| invalid_transport_mapping(&characteristic_id))?;
            ConnectorTransportRequest::Write {
                characteristic_id,
                service_uuid,
                characteristic_uuid,
                bytes,
                confirmed,
            }
        }
        EngineTransportRequest::Disconnect => ConnectorTransportRequest::Disconnect,
        EngineTransportRequest::SetTimer { token, delay_ms } => {
            ConnectorTransportRequest::SetTimer { token, delay_ms }
        }
        EngineTransportRequest::CancelTimer { token } => {
            ConnectorTransportRequest::CancelTimer { token }
        }
    };
    Ok(ConnectorTransportAction {
        connector_id: value.connector_id.as_str().to_owned(),
        session_id: value.session_id,
        cancellation_generation: value.cancellation_generation,
        operation_id: value.operation_id,
        deadline_token: value.deadline_token,
        request,
    })
}

fn invalid_transport_mapping(characteristic_id: &str) -> FfiError {
    MavError::new(
        codes::CONNECTOR_HOST_ACTION_INVALID,
        format!("connector characteristic {characteristic_id} has no native address"),
    )
    .into()
}

fn digest(bytes: &[u8], field: &str) -> Result<[u8; 32], FfiError> {
    bytes.try_into().map_err(|_| {
        FfiError::from(MavError::new(
            codes::CONNECTOR_TRUST_POLICY_INVALID,
            format!("{field} must be exactly 32 bytes"),
        ))
    })
}

fn invalid_trust() -> FfiError {
    MavError::new(
        codes::CONNECTOR_TRUST_POLICY_INVALID,
        "connector publisher key status fields are inconsistent",
    )
    .into()
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::{event_from_ffi, transport_action_to_ffi, ConnectorTransportEvent};
    use mav_connector_abi::{ConnectorId, EventBody};
    use mav_engine::{
        ConnectorTransportAction as EngineAction, ConnectorTransportRequest as EngineRequest,
    };

    #[test]
    fn event_conversion_preserves_raw_notification_bytes() {
        let body = event_from_ffi(ConnectorTransportEvent::Notification {
            characteristic_id: "data".to_owned(),
            bytes: vec![0, 127, 128, 255],
        });
        assert_eq!(
            body,
            EventBody::Notification {
                characteristic_id: "data".to_owned(),
                bytes: vec![0, 127, 128, 255],
            }
        );
    }

    #[test]
    fn action_conversion_preserves_ids_flags_and_raw_bytes() {
        let action = transport_action_to_ffi(
            EngineAction {
                connector_id: ConnectorId::new("org.example.ffi").expect("connector id"),
                session_id: 4,
                cancellation_generation: 5,
                operation_id: 6,
                deadline_token: 7,
                body: EngineRequest::Write {
                    characteristic_id: "control".to_owned(),
                    bytes: vec![0, 1, 254, 255],
                    confirmed: true,
                },
            },
            Some(("180d".to_owned(), "2a39".to_owned())),
        )
        .expect("mapped action");
        assert_eq!(action.connector_id, "org.example.ffi");
        assert_eq!(action.session_id, 4);
        assert_eq!(action.cancellation_generation, 5);
        assert_eq!(action.operation_id, 6);
        assert_eq!(action.deadline_token, 7);
        assert_eq!(
            action.request,
            super::ConnectorTransportRequest::Write {
                characteristic_id: "control".to_owned(),
                service_uuid: "180d".to_owned(),
                characteristic_uuid: "2a39".to_owned(),
                bytes: vec![0, 1, 254, 255],
                confirmed: true,
            }
        );
    }
}
