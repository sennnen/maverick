//! The UniFFI facade: the single surface iOS and Android call into (ADR-010). It exposes only what
//! an app needs and nothing of the pipeline's internals, so the types behind it can keep moving
//! while the boundary stays small. `MavRuntime` owns installed artifact state and one active,
//! platform-neutral connector session.
//!
//! Generating the Swift and Kotlin bindings and linking them on each platform is documented in
//! apps/ios/README.md and apps/android/README.md; the Rust side and the bindgen step are verified
//! in CI, and the simulator link is a documented local step until the app milestone.
#![forbid(unsafe_code)]

mod connector;

pub use connector::*;

use mav_model::error::MavError;
use std::sync::{Arc, Mutex, MutexGuard};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    #[error("MAV-{code} [{category}/{severity}] {safe_message}")]
    Core {
        code: u16,
        category: String,
        severity: String,
        safe_message: String,
        context: Vec<String>,
    },
}

impl From<MavError> for FfiError {
    fn from(error: MavError) -> Self {
        FfiError::Core {
            code: error.code,
            category: format!("{:?}", error.category).to_lowercase(),
            severity: format!("{:?}", error.severity).to_lowercase(),
            safe_message: error.message,
            context: error.context,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeConfig {
    pub database_path: String,
    pub timezone_id: String,
    pub transport_capacity: u32,
    pub app_version: String,
    pub app_build: String,
}

#[derive(uniffi::Object)]
pub struct MavRuntime {
    connectors: Mutex<mav_connector_store::ConnectorRepository>,
    connector_session: Mutex<Option<mav_engine::ConnectorHost>>,
    database_path: String,
}

#[uniffi::export]
impl MavRuntime {
    #[uniffi::constructor]
    pub fn new(config: RuntimeConfig) -> Result<Arc<Self>, FfiError> {
        let database_path = config.database_path;
        let connectors = mav_connector_store::ConnectorRepository::open(&database_path)?;
        Ok(Arc::new(Self {
            connectors: Mutex::new(connectors),
            connector_session: Mutex::new(None),
            database_path,
        }))
    }

    pub fn inspect_connector_bytes(
        &self,
        bytes: Vec<u8>,
        source: ConnectorSourceMetadata,
        policy: ConnectorTrustPolicy,
        revocations: ConnectorTrustRevocations,
        now_ms: i64,
        approval_ttl_ms: i64,
    ) -> Result<ConnectorInspection, FfiError> {
        let source = connector::source_from_ffi(source)?;
        let policy = connector::policy_from_ffi(policy)?;
        let revocations = connector::revocations_from_ffi(revocations);
        let approval = self.connectors_lock()?.inspect_connector(
            bytes,
            source,
            &policy,
            &revocations,
            now_ms,
            approval_ttl_ms,
        )?;
        let capabilities = approval
            .report
            .manifest
            .capabilities
            .iter()
            .map(|capability| capability.stream.clone())
            .collect();
        let permissions = approval
            .report
            .manifest
            .permissions
            .iter()
            .map(|permission| match permission {
                mav_connector_abi::Permission::Ble => "Bluetooth device access".to_owned(),
            })
            .collect();
        Ok(ConnectorInspection {
            artifact_digest: approval.report.artifact_digest.to_vec(),
            manifest_digest: approval.report.manifest_digest.to_vec(),
            connector_id: approval.report.manifest.connector_id.as_str().to_owned(),
            version: approval.report.manifest.version,
            display_name: approval.report.manifest.display_name,
            description: approval.report.manifest.description,
            publisher_key_id: approval.report.manifest.publisher_key_id,
            capabilities,
            permissions,
            state_schema: approval.report.manifest.state_schema,
            fixture_count: approval.fixture_count,
            source: connector::source_to_ffi(approval.source),
            approval_token: approval.approval.to_bytes().to_vec(),
            approval_expires_at_ms: approval.approval.expires_at_ms(),
        })
    }

    pub fn install_connector_bytes(
        &self,
        request: ConnectorInstallRequest,
        policy: ConnectorTrustPolicy,
        revocations: ConnectorTrustRevocations,
    ) -> Result<InstalledConnectorRecord, FfiError> {
        let now_ms = request.now_ms;
        let request = mav_connector_store::InstallRequest {
            bytes: request.bytes,
            source: connector::source_from_ffi(request.source)?,
            approval: mav_connector_store::ApprovalToken::from_bytes(&request.approval_token)?,
            activate: request.activate,
        };
        let policy = connector::policy_from_ffi(policy)?;
        let revocations = connector::revocations_from_ffi(revocations);
        let installed =
            self.connectors_lock()?
                .install_connector(request, &policy, &revocations, now_ms)?;
        if installed.active {
            self.retire_connector_session(mav_connector_abi::CancelReason::Update, now_ms)?;
        }
        Ok(connector::installed_to_ffi(installed))
    }

    pub fn list_installed_connectors(&self) -> Result<Vec<InstalledConnectorRecord>, FfiError> {
        Ok(self
            .connectors_lock()?
            .list_connectors()?
            .into_iter()
            .map(connector::installed_to_ffi)
            .collect())
    }

    pub fn activate_installed_connector(
        &self,
        connector_id: String,
        version: String,
        policy: ConnectorTrustPolicy,
        revocations: ConnectorTrustRevocations,
        now_ms: i64,
    ) -> Result<(), FfiError> {
        let policy = connector::policy_from_ffi(policy)?;
        let revocations = connector::revocations_from_ffi(revocations);
        self.connectors_lock()?.activate_connector(
            &connector_id,
            &version,
            &policy,
            &revocations,
            now_ms,
        )?;
        self.retire_connector_session(mav_connector_abi::CancelReason::Update, now_ms)?;
        Ok(())
    }

    pub fn rollback_installed_connector(
        &self,
        connector_id: String,
        policy: ConnectorTrustPolicy,
        revocations: ConnectorTrustRevocations,
        now_ms: i64,
    ) -> Result<(), FfiError> {
        let policy = connector::policy_from_ffi(policy)?;
        let revocations = connector::revocations_from_ffi(revocations);
        self.connectors_lock()?
            .rollback_connector(&connector_id, &policy, &revocations, now_ms)?;
        self.retire_connector_session(mav_connector_abi::CancelReason::Update, now_ms)?;
        Ok(())
    }

    pub fn remove_installed_connector(
        &self,
        connector_id: String,
        version: String,
        mode: ConnectorRemovalMode,
        policy: ConnectorTrustPolicy,
        revocations: ConnectorTrustRevocations,
        now_ms: i64,
    ) -> Result<(), FfiError> {
        let policy = connector::policy_from_ffi(policy)?;
        let revocations = connector::revocations_from_ffi(revocations);
        let mode = match mode {
            ConnectorRemovalMode::DeleteState => mav_connector_store::RemovalMode::DeleteState,
            ConnectorRemovalMode::QuarantineState => {
                mav_connector_store::RemovalMode::QuarantineState
            }
        };
        self.connectors_lock()?.remove_connector(
            &connector_id,
            &version,
            mode,
            &policy,
            &revocations,
            now_ms,
        )?;
        self.retire_connector_session(mav_connector_abi::CancelReason::Removal, now_ms)?;
        Ok(())
    }

    pub fn enforce_connector_trust(
        &self,
        policy: ConnectorTrustPolicy,
        revocations: ConnectorTrustRevocations,
        now_ms: i64,
    ) -> Result<Vec<String>, FfiError> {
        let policy = connector::policy_from_ffi(policy)?;
        let revocations = connector::revocations_from_ffi(revocations);
        let disabled = self
            .connectors_lock()?
            .enforce_policy(&policy, &revocations, now_ms)?;
        if !disabled.is_empty() {
            self.retire_connector_session(mav_connector_abi::CancelReason::Update, now_ms)?;
        }
        Ok(disabled)
    }

    pub fn open_connector_session(
        &self,
        config: ConnectorSessionConfig,
        policy: ConnectorTrustPolicy,
        revocations: ConnectorTrustRevocations,
    ) -> Result<ConnectorLifecycleReport, FfiError> {
        let policy = connector::policy_from_ffi(policy)?;
        let revocations = connector::revocations_from_ffi(revocations);
        let artifact = self.connectors_lock()?.active_artifact(
            &config.connector_id,
            &policy,
            &revocations,
            config.now_ms,
        )?;
        let store = mav_engine::Store::open(std::path::Path::new(&self.database_path))?;
        let mut host = mav_engine::ConnectorHost::instantiate(
            &artifact,
            mav_connector_runtime::LimitProfile::mobile_v1(),
            store,
            mav_engine::ConnectorHostConfig {
                session_id: config.session_id,
                device_id: config.device_id,
                transport_capacity: config.transport_capacity,
            },
        )?;
        host.start()?;
        let report = host.lifecycle_snapshot().into();
        let mut session = self.connector_session_lock()?;
        if let Some(previous) = session.as_mut() {
            previous.terminate(mav_connector_abi::CancelReason::Update, Some(config.now_ms))?;
        }
        *session = Some(host);
        Ok(report)
    }

    pub fn apply_connector_event(
        &self,
        event: ConnectorTransportEvent,
        wall_time_ms: Option<i64>,
    ) -> Result<ConnectorApplyOutcome, FfiError> {
        let mut session = self.connector_session_lock()?;
        let host = session.as_mut().ok_or_else(no_connector_session)?;
        Ok(host
            .apply(connector::event_from_ffi(event), wall_time_ms)?
            .into())
    }

    pub fn cancel_connector_session(
        &self,
        reason: ConnectorCancelReason,
        wall_time_ms: Option<i64>,
    ) -> Result<ConnectorLifecycleReport, FfiError> {
        let mut session = self.connector_session_lock()?;
        let host = session.as_mut().ok_or_else(no_connector_session)?;
        host.cancel(connector::cancel_from_ffi(reason), wall_time_ms)?;
        Ok(host.lifecycle_snapshot().into())
    }

    pub fn drain_connector_actions(
        &self,
        limit: u32,
    ) -> Result<Vec<ConnectorTransportAction>, FfiError> {
        let mut session = self.connector_session_lock()?;
        let host = session.as_mut().ok_or_else(no_connector_session)?;
        let actions = host.drain_actions(limit);
        actions
            .into_iter()
            .map(|action| {
                let address = match &action.body {
                    mav_engine::ConnectorTransportRequest::Subscribe { characteristic_id }
                    | mav_engine::ConnectorTransportRequest::Unsubscribe { characteristic_id }
                    | mav_engine::ConnectorTransportRequest::Read { characteristic_id }
                    | mav_engine::ConnectorTransportRequest::Write {
                        characteristic_id, ..
                    } => host.characteristic_address(characteristic_id),
                    _ => None,
                };
                connector::transport_action_to_ffi(action, address)
            })
            .collect()
    }

    pub fn connector_lifecycle(&self) -> Result<ConnectorLifecycleReport, FfiError> {
        let session = self.connector_session_lock()?;
        let host = session.as_ref().ok_or_else(no_connector_session)?;
        Ok(host.lifecycle_snapshot().into())
    }
}

impl MavRuntime {
    fn connectors_lock(
        &self,
    ) -> Result<MutexGuard<'_, mav_connector_store::ConnectorRepository>, FfiError> {
        self.connectors
            .lock()
            .map_err(|_| poisoned("connector repository"))
    }

    fn connector_session_lock(
        &self,
    ) -> Result<MutexGuard<'_, Option<mav_engine::ConnectorHost>>, FfiError> {
        self.connector_session
            .lock()
            .map_err(|_| poisoned("connector session"))
    }

    fn retire_connector_session(
        &self,
        reason: mav_connector_abi::CancelReason,
        now_ms: i64,
    ) -> Result<(), FfiError> {
        let mut session = self.connector_session_lock()?;
        if let Some(host) = session.as_mut() {
            host.terminate(reason, Some(now_ms))?;
        }
        *session = None;
        Ok(())
    }
}

fn poisoned(owner: &str) -> FfiError {
    MavError::fatal(
        mav_model::error::codes::INTERNAL_INVARIANT,
        format!("{owner} lock is poisoned"),
    )
    .into()
}

fn no_connector_session() -> FfiError {
    MavError::new(
        mav_model::error::codes::CONNECTOR_HOST_STATE,
        "no connector session is open",
    )
    .into()
}

/// The core version, so a host and a bug report can pin exactly which build produced a result.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_version_matches_the_crate() {
        assert_eq!(core_version(), env!("CARGO_PKG_VERSION"));
    }
}
