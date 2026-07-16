//! The UniFFI facade: the single surface iOS and Android call into (ADR-010). It exposes only what
//! an app needs and nothing of the pipeline's internals, so the types behind it can keep moving
//! while the boundary stays small. Stateless capture replay provides parity fixtures; `MavRuntime`
//! owns persistent product state and the ordered live-data pipeline.
//!
//! Generating the Swift and Kotlin bindings and linking them on each platform is documented in
//! apps/ios/README.md and apps/android/README.md; the Rust side and the bindgen step are verified
//! in CI, and the simulator link is a documented local step until the app milestone.
#![forbid(unsafe_code)]

use mav_model::error::MavError;
use mav_obs::stage::Stage;
use mav_obs::tap::{Tap, TapEvent};
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

/// Canonical session and analytics read models, each paired with its parity hash.
#[derive(Debug, uniffi::Record)]
pub struct RunResult {
    pub snapshot_json: String,
    pub hash: String,
    pub analytics_json: String,
    pub analytics_hash: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct RuntimeConfig {
    pub database_path: String,
    pub timezone_id: String,
    pub transport_capacity: u32,
    pub app_version: String,
    pub app_build: String,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct ConnectorRegistration {
    pub connector_id: String,
    pub connector_version: String,
    pub manifest_json: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, uniffi::Enum)]
pub enum RuntimeConnectionState {
    Disconnected,
    Scanning,
    Connecting,
    Subscribing,
    Streaming,
    Failed,
}

#[derive(Clone, PartialEq, Eq, Debug, uniffi::Enum)]
pub enum TransportAction {
    StartScan {
        service_filters: Vec<String>,
    },
    StopScan,
    Connect {
        native_device_id: String,
    },
    Subscribe {
        characteristic: String,
    },
    Write {
        characteristic: String,
        bytes: Vec<u8>,
        with_response: bool,
        sequence: u8,
    },
    Disconnect {
        native_device_id: String,
        reason: String,
    },
}

#[derive(Clone, Copy, Debug, uniffi::Record)]
pub struct IngestResult {
    pub inserted: u32,
    pub duplicates: u32,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct HostSnapshotResult {
    pub json: String,
    pub hash: String,
    pub revision: u64,
}

#[derive(uniffi::Object)]
pub struct MavRuntime {
    inner: Mutex<mav_engine::HostRuntime>,
}

#[uniffi::export]
impl MavRuntime {
    #[uniffi::constructor]
    pub fn new(config: RuntimeConfig) -> Result<Arc<Self>, FfiError> {
        let runtime = mav_engine::HostRuntime::open(mav_engine::RuntimeConfig {
            database_path: config.database_path,
            timezone_id: config.timezone_id,
            transport_capacity: config.transport_capacity,
            app_version: config.app_version,
            app_build: config.app_build,
        })?;
        Ok(Arc::new(Self {
            inner: Mutex::new(runtime),
        }))
    }

    pub fn install_connector(&self, registration: ConnectorRegistration) -> Result<(), FfiError> {
        self.lock()?
            .install_connector(mav_engine::ConnectorRegistration {
                connector_id: registration.connector_id,
                connector_version: registration.connector_version,
                manifest_json: registration.manifest_json,
            })?;
        Ok(())
    }

    pub fn start_scan(&self, connector_id: String, device_id: u64) -> Result<(), FfiError> {
        self.lock()?.start_scan(&connector_id, device_id)?;
        Ok(())
    }

    pub fn device_discovered(
        &self,
        connector_id: String,
        native_device_id: String,
        display_name: Option<String>,
    ) -> Result<(), FfiError> {
        self.lock()?
            .device_discovered(&connector_id, native_device_id, display_name)?;
        Ok(())
    }

    pub fn connected(&self, native_device_id: String) -> Result<(), FfiError> {
        self.lock()?.connected(&native_device_id)?;
        Ok(())
    }

    pub fn subscribed(&self, characteristic: String) -> Result<(), FfiError> {
        self.lock()?.subscribed(&characteristic)?;
        Ok(())
    }

    pub fn notification(
        &self,
        characteristic: String,
        bytes: Vec<u8>,
        at_unix_ms: i64,
    ) -> Result<IngestResult, FfiError> {
        let stats = self
            .lock()?
            .notification(&characteristic, &bytes, at_unix_ms)?;
        Ok(IngestResult {
            inserted: stats.inserted,
            duplicates: stats.duplicates,
        })
    }

    pub fn transport_failed(
        &self,
        operation: String,
        native_code: String,
        safe_message: String,
        at_unix_ms: i64,
    ) -> Result<(), FfiError> {
        self.lock()?
            .transport_failed(&operation, &native_code, &safe_message, at_unix_ms)?;
        Ok(())
    }

    pub fn disconnected(&self, native_device_id: String) -> Result<(), FfiError> {
        self.lock()?.disconnected(&native_device_id)?;
        Ok(())
    }

    pub fn drain_actions(&self, limit: u32) -> Result<Vec<TransportAction>, FfiError> {
        Ok(self
            .lock()?
            .drain_actions(limit)
            .into_iter()
            .map(TransportAction::from)
            .collect())
    }

    pub fn connection_state(&self) -> Result<RuntimeConnectionState, FfiError> {
        Ok(RuntimeConnectionState::from(
            self.lock()?.connection_state(),
        ))
    }

    pub fn host_snapshot(&self, at_unix_ms: i64) -> Result<HostSnapshotResult, FfiError> {
        let result = self.lock()?.host_snapshot(at_unix_ms)?;
        Ok(HostSnapshotResult {
            json: result.json,
            hash: result.hash,
            revision: result.revision,
        })
    }
}

impl MavRuntime {
    fn lock(&self) -> Result<MutexGuard<'_, mav_engine::HostRuntime>, FfiError> {
        self.inner.lock().map_err(|_| {
            MavError::fatal(
                mav_model::error::codes::INTERNAL_INVARIANT,
                "host runtime lock is poisoned",
            )
            .into()
        })
    }
}

impl From<mav_engine::ConnectionState> for RuntimeConnectionState {
    fn from(state: mav_engine::ConnectionState) -> Self {
        match state {
            mav_engine::ConnectionState::Disconnected => Self::Disconnected,
            mav_engine::ConnectionState::Scanning => Self::Scanning,
            mav_engine::ConnectionState::Connecting => Self::Connecting,
            mav_engine::ConnectionState::Subscribing => Self::Subscribing,
            mav_engine::ConnectionState::Streaming => Self::Streaming,
            mav_engine::ConnectionState::Failed => Self::Failed,
        }
    }
}

impl From<mav_engine::TransportAction> for TransportAction {
    fn from(action: mav_engine::TransportAction) -> Self {
        match action {
            mav_engine::TransportAction::StartScan { service_filters } => {
                Self::StartScan { service_filters }
            }
            mav_engine::TransportAction::StopScan => Self::StopScan,
            mav_engine::TransportAction::Connect { native_device_id } => {
                Self::Connect { native_device_id }
            }
            mav_engine::TransportAction::Subscribe { characteristic } => {
                Self::Subscribe { characteristic }
            }
            mav_engine::TransportAction::Write {
                characteristic,
                bytes,
                with_response,
                sequence,
            } => Self::Write {
                characteristic,
                bytes,
                with_response,
                sequence,
            },
            mav_engine::TransportAction::Disconnect {
                native_device_id,
                reason,
            } => Self::Disconnect {
                native_device_id,
                reason,
            },
        }
    }
}

/// A tap that keeps nothing. A host that wants the boundary dump uses `mav-replay` or a future
/// streaming surface.
struct DiscardTap;
impl Tap for DiscardTap {
    fn on_stage(&self, _stage: Stage, _event: TapEvent) {}
}

/// The core version, so a host and a bug report can pin exactly which build produced a result.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Run one `capture/v1` capture against a device manifest and return canonical session and analytics
/// JSON with their hashes. Both inputs are JSON strings the host already holds, so the boundary
/// carries no pipeline types. The parity harness drives this on each platform: the same inputs must
/// return the same hashes, and any difference is a binding bug.
#[uniffi::export]
pub fn run_capture(manifest_json: String, capture_json: String) -> Result<RunResult, FfiError> {
    let output = mav_engine::run_realtime_output_json(&manifest_json, &capture_json, &DiscardTap)?;
    Ok(RunResult {
        snapshot_json: output.snapshot.canonical_json()?,
        hash: output.snapshot.canonical_hash()?,
        analytics_json: output.analytics.canonical_json()?,
        analytics_hash: output.analytics.canonical_hash()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DB: AtomicU64 = AtomicU64::new(1);
    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/replay")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
    }

    fn db_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "mav-ffi-runtime-{}-{}.sqlite",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn core_version_matches_the_crate() {
        assert_eq!(core_version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn run_capture_reproduces_the_golden_hash() {
        // The FFI must return exactly what mav-replay froze for the same fixture.
        let result = run_capture(
            fixture("realtime_hr_v1.manifest.json"),
            fixture("realtime_hr_v1.capture.json"),
        )
        .unwrap();
        assert_eq!(result.hash, "33143ef069a85a38");
        assert!(result.snapshot_json.contains("\"current_bpm\":63"));
        assert!(result.analytics_json.contains("\"availability\""));
    }

    #[test]
    fn run_capture_exposes_the_frozen_prv_analytics() {
        let result = run_capture(
            fixture("realtime_rr_prv_v1.manifest.json"),
            fixture("realtime_rr_prv_v1.capture.json"),
        )
        .unwrap();
        assert_eq!(result.analytics_hash, "e77c7b04c7fceb2c");
        assert!(result
            .analytics_json
            .contains("\"variability_label\":\"pulse_rate_variability\""));
        assert!(result
            .analytics_json
            .contains("\"kind\":\"algorithm_not_admitted\""));
    }

    #[test]
    fn a_broken_capture_is_a_readable_error() {
        let err = run_capture("{}".to_owned(), "{}".to_owned()).unwrap_err();
        let FfiError::Core {
            code,
            category,
            safe_message,
            ..
        } = err;
        assert_eq!(code, mav_model::error::codes::DECODE_LAYOUT_INVALID);
        assert_eq!(category, "decode");
        assert_eq!(safe_message, "manifest does not parse");
    }

    #[test]
    fn stateful_runtime_reproduces_the_frozen_snapshot() {
        let path = db_path();
        let _ = std::fs::remove_file(&path);
        let runtime = MavRuntime::new(RuntimeConfig {
            database_path: path.to_string_lossy().into_owned(),
            timezone_id: "Europe/London".to_owned(),
            transport_capacity: 16,
            app_version: "0.1.0".to_owned(),
            app_build: "test".to_owned(),
        })
        .unwrap();
        runtime
            .install_connector(ConnectorRegistration {
                connector_id: "fixture".to_owned(),
                connector_version: "1.0.0".to_owned(),
                manifest_json: fixture("realtime_hr_v1.manifest.json"),
            })
            .unwrap();
        runtime.start_scan("fixture".to_owned(), 1).unwrap();
        assert!(matches!(
            runtime.drain_actions(8).unwrap().as_slice(),
            [TransportAction::StartScan { .. }]
        ));
        runtime
            .device_discovered(
                "fixture".to_owned(),
                "native-1".to_owned(),
                Some("MG".to_owned()),
            )
            .unwrap();
        assert_eq!(runtime.drain_actions(8).unwrap().len(), 2);
        runtime.connected("native-1".to_owned()).unwrap();
        let actions = runtime.drain_actions(8).unwrap();
        assert_eq!(
            actions,
            vec![TransportAction::Subscribe {
                characteristic: "n".to_owned()
            }]
        );
        runtime.subscribed("n".to_owned()).unwrap();

        let capture =
            mav_engine::Capture::from_json(&fixture("realtime_hr_v1.capture.json")).unwrap();
        for chunk in capture.chunks {
            runtime
                .notification("n".to_owned(), chunk, 1_752_600_500_000)
                .unwrap();
        }
        let result = runtime.host_snapshot(1_752_600_500_000).unwrap();
        let value: serde_json::Value = serde_json::from_str(&result.json).unwrap();
        assert_eq!(value["session"]["current_bpm"], 63);
        assert_eq!(value["connection"]["display_name"], "MG");
        assert_eq!(
            runtime.connection_state().unwrap(),
            RuntimeConnectionState::Streaming
        );
        let _ = std::fs::remove_file(path);
    }
}
