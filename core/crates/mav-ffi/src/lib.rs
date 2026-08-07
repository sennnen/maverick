//! The UniFFI facade: the single surface iOS and Android call into (ADR-010). It exposes only what
//! an app needs and nothing of the pipeline's internals, so the types behind it can keep moving
//! while the boundary stays small. `MavRuntime` owns installed artifact state and one active,
//! platform-neutral connector session.
//!
//! Generating the Swift and Kotlin bindings and linking them on each platform is documented in
//! apps/ios/README.md and apps/android/README.md; the Rust side and the bindgen step are verified
//! in CI, and the simulator link is a documented local step until the app milestone.
#![forbid(unsafe_code)]

mod analytics;
mod connector;
mod models;

pub use connector::*;
pub use models::*;

use mav_model::error::MavError;
use mav_model::ids::{DeviceId, EcgCaptureId};
use mav_model::stream::{Sample, StreamKind};
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
    /// The IANA id the offset spans came from. A label for display and provenance; the core never
    /// derives an offset from it — see `set_timezone_spans` (ADR-024).
    pub timezone_id: String,
    /// Stamped onto the report bundle so a diagnostic can be tied to a build.
    pub app_version: String,
}

/// Lock order, where two of the mutexes below are ever held at once: `scheduler` before `models`,
/// and never the reverse. `admit_prepared` is the only place that nests them, because deciding
/// "already answered or already running" and queueing the work have to be one atomic step or two
/// planning passes race and the platform runs the same tensors twice. Everything else — including
/// `submit_model_inference`, which touches both — releases one before taking the other.
#[derive(uniffi::Object)]
pub struct MavRuntime {
    connectors: Mutex<mav_connector_store::ConnectorRepository>,
    connector_session: Mutex<Option<mav_engine::ConnectorHost>>,
    /// Survives session teardown so a reconnect re-states the user's choice (ADR-030).
    low_power: core::sync::atomic::AtomicBool,
    database_path: String,
    /// One long-lived connection for every read path. A session's host owns its own writer, so
    /// this is a second connection by design; what it replaces is opening a third on every poll.
    reader: Mutex<mav_engine::Store>,
    /// The analytic spine and the zone the platform supplied for it. Defaults to UTC so a caller
    /// that never sets spans still gets coherent days rather than an error.
    spine: Mutex<mav_engine::Spine>,
    /// The always-on ring log every connector session reports into, and the app version that
    /// stamps the report bundle drawn from it.
    ring: Arc<mav_obs::RingLog>,
    app_version: String,
    /// The pull-based queue between the core and the platform inference runtimes (see
    /// `models.rs`). Outlives any one connector session: an embedding queued from stored
    /// samples does not need a strap connected.
    models: Mutex<mav_engine::ModelHost>,
    /// Which of the forty-one models are worth running on this device, in what order, and what
    /// has already been answered. Outlives a connector session for the same reason `models`
    /// does: yesterday's night is still worth analysing with no strap in range.
    scheduler: Mutex<mav_engine::AnalyticsScheduler>,
    /// The wearer's own figures, as the profile heads take them. Held here rather than passed
    /// per call so the core can complete a chained head — including picking the probe branch a
    /// sex selects — without either platform reimplementing the substitution.
    profile: Mutex<Option<mav_engine::WearerProfile>>,
    /// The connector state revision already written through to the install store, so a session
    /// that commits nothing new does not re-serialise a connector's state on every packet.
    /// Reset when a session opens, because revisions count from zero within one session.
    persisted_state_revision: Mutex<u64>,
}

/// How many stage events the ring log holds. Bounded on purpose: observability that grows without
/// limit is a leak, and the durable record is the error journal, not this.
const RING_CAPACITY: usize = 512;

/// One explicit UTC-offset span, supplied by the platform. Rust takes no tzdata dependency: the
/// phone already carries a correct and updated zone database, and it is the only place the user's
/// zone is genuinely known (ADR-024).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimezoneSpan {
    pub start_unix_seconds: i64,
    pub offset_seconds: i32,
}

/// Whether one analytic is served, and when it is not, why. The reason comes from the core; a
/// platform never substitutes its own number for an unavailable analytic.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AnalyticAvailabilityReport {
    pub analytic: String,
    pub available: bool,
    /// `missing_streams` or `algorithm_not_admitted`, absent when the analytic is available.
    pub reason: Option<String>,
    /// The streams the analytic needed and the day did not hold.
    pub missing_streams: Vec<String>,
}

/// Time-domain interval variability. `label` is `heart_rate_variability` only when the intervals
/// came from ECG; optical intervals are labelled `pulse_rate_variability` and must be displayed
/// as such.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HrvReport {
    pub label: String,
    pub mean_interval_ms: f64,
    pub rmssd_ms: f64,
    pub sdnn_ms: f64,
    pub pnn50_percent: f64,
    /// Poincaré short-term scatter — the beat-to-beat axis.
    pub sd1_ms: f64,
    /// Poincaré long-term scatter — the axis along the identity line.
    pub sd2_ms: f64,
    /// Short-term detrended fluctuation exponent, when a long enough uninterrupted run existed.
    pub alpha1: Option<f64>,
    pub interval_count: u32,
    pub excluded_count: u32,
}

/// Task Force band powers over the longest uninterrupted run of beats. `lf_normalized` and
/// `hf_normalized` are the convention-free form and are what a display should prefer.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HrvSpectrumReport {
    pub vlf_power_ms2: f64,
    pub lf_power_ms2: f64,
    pub hf_power_ms2: f64,
    pub total_power_ms2: f64,
    pub lf_normalized: f64,
    pub hf_normalized: f64,
    pub lf_hf_ratio: f64,
    pub span_seconds: f64,
}

/// The longitudinal readiness readout. Absent while the baseline is still calibrating.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReadinessReport {
    /// `primed`, `normal`, or `suppressed`.
    pub tier: String,
    pub baseline7_ms: f64,
    pub normal_low_ms: f64,
    pub normal_high_ms: f64,
    pub overreaching_watch: bool,
}

/// One local day's analytics. Every absent value is explained by `availability`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DailySnapshotReport {
    pub day: String,
    pub day_index: i64,
    pub current_bpm: Option<u16>,
    pub mean_bpm: Option<f64>,
    pub hr_sample_count: u32,
    pub hr_excluded_count: u32,
    pub hrv: Option<HrvReport>,
    pub hrv_spectrum: Option<HrvSpectrumReport>,
    pub readiness: Option<ReadinessReport>,
    pub availability: Vec<AnalyticAvailabilityReport>,
    /// `id@version` for every algorithm that contributed.
    pub algorithms: Vec<String>,
    /// A stable digest of the whole record. Both platforms must read the same string from the same
    /// fixture day; that equality is the parity contract.
    pub snapshot_hash: String,
}

/// The five heart-rate zones for one person, resolved in the core.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HeartRateZones {
    pub max_hr: f64,
    /// `tanaka` or `override`, so a screen can say where the ceiling came from.
    pub source: String,
    /// Lower bpm bound of zones 1..=5, ascending.
    pub lower_bpm: Vec<f64>,
}

/// One observed pipeline boundary, flattened for the bindings.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ObservedStage {
    pub seq: u64,
    pub stage: String,
    pub kind: String,
    pub count: u32,
    pub detail: String,
}

/// What a bug report carries: the app build, the live session if there is one, and the recent
/// stage boundaries. No sample values — the ring log holds counts, and payload summaries exist
/// only in debug builds.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ReportBundle {
    pub app_version: String,
    pub connector_id: Option<String>,
    pub session_id: Option<u64>,
    pub trace_hash: Option<String>,
    pub samples_persisted: u64,
    pub samples_duplicate: u64,
    pub recent_stages: Vec<ObservedStage>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct EcgCaptureReport {
    pub capture_id: u64,
    /// `calibrating`, `recording`, `analysing`, `result`, `failed`, or `cancelled`.
    pub phase: String,
    pub progress_milli: u16,
    pub quality_milli: u16,
    pub quality_reason: Option<String>,
    pub recorded_samples: u32,
    pub target_samples: u32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EcgTensor {
    pub values: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EcgInferenceRequest {
    pub capture_id: u64,
    /// Baseline first, then six ordered five-second occlusions.
    pub tensors: Vec<EcgTensor>,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct EcgPrediction {
    pub sinus_rhythm: f32,
    pub atrial_fibrillation: f32,
    pub other_abnormal_rhythm: f32,
}

/// One at-a-glance finding. `passed` is true when the reassuring reading is the correct one, so a
/// screen can render a tick without re-deriving what "good" means for each check.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct EcgCheck {
    /// `afib`, `high_heart_rate`, `low_heart_rate`, or `sinus_rhythm`.
    pub id: String,
    pub passed: bool,
    /// False when the reading cannot support the check — no rate means no rate verdict.
    pub known: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct EcgExplanation {
    pub start_second: u8,
    pub end_second: u8,
    pub importance_milli: u16,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EcgResultReport {
    pub capture_id: u64,
    pub device_id: u64,
    pub started_ns: i64,
    pub ended_ns: i64,
    pub source_rate_hz: u32,
    pub sample_count: u32,
    /// `sinus_rhythm`, `atrial_fibrillation`, or `other_abnormal_rhythm`.
    pub rhythm: String,
    pub sinus_probability: f32,
    pub atrial_fibrillation_probability: f32,
    pub other_abnormal_probability: f32,
    pub confidence_milli: u16,
    pub quality_milli: u16,
    /// Mean rate over the recording, absent when too few beats were found to average.
    pub mean_heart_rate_bpm: Option<u16>,
    /// The at-a-glance checks both apps render, in a fixed order. Derived in the core so the two
    /// platforms cannot disagree about what the same result means.
    pub checks: Vec<EcgCheck>,
    pub explanation: Vec<EcgExplanation>,
    pub raw_sha256: String,
    pub tensor_sha256: String,
    pub preprocessing_sha256: String,
    pub model_sha256: String,
    pub algorithm_id: String,
    pub algorithm_version: String,
    pub provisional: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EcgReportPayload {
    pub result: EcgResultReport,
    pub source_unit: String,
    pub waveform: Vec<f32>,
}

#[uniffi::export]
impl MavRuntime {
    #[uniffi::constructor]
    pub fn new(config: RuntimeConfig) -> Result<Arc<Self>, FfiError> {
        let database_path = config.database_path;
        let connectors = mav_connector_store::ConnectorRepository::open(&database_path)?;
        let reader = mav_engine::Store::open(std::path::Path::new(&database_path))?;
        Ok(Arc::new(Self {
            connectors: Mutex::new(connectors),
            connector_session: Mutex::new(None),
            low_power: core::sync::atomic::AtomicBool::new(false),
            reader: Mutex::new(reader),
            spine: Mutex::new(mav_engine::Spine::new(mav_engine::Timezone::fixed(
                &config.timezone_id,
                0,
            ))),
            database_path,
            ring: Arc::new(mav_obs::RingLog::new(RING_CAPACITY)),
            app_version: config.app_version,
            models: Mutex::new(mav_engine::ModelHost::new()),
            scheduler: Mutex::new(mav_engine::AnalyticsScheduler::new()),
            profile: Mutex::new(None),
            persisted_state_revision: Mutex::new(0),
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
        let captures = approval
            .report
            .manifest
            .captures
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(|capture| ConnectorCaptureCapability {
                stream: capture.stream.clone(),
                unit: capture.unit.clone(),
                minimum_sample_rate_hz: capture.minimum_sample_rate_hz,
                maximum_sample_rate_hz: capture.maximum_sample_rate_hz,
            })
            .collect();
        let device_families = approval
            .report
            .manifest
            .device_families
            .iter()
            .map(|family| connector::ConnectorDeviceFamily {
                id: family.id.clone(),
                name_prefixes: family.name_prefixes.clone(),
                service_uuids: family.service_uuids.clone(),
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
            captures,
            permissions,
            device_families,
            state_schema: approval.report.manifest.state_schema,
            fixture_count: approval.fixture_count,
            source: connector::source_to_ffi(approval.source),
            approval_token: approval.approval.to_bytes().to_vec(),
            approval_expires_at_ms: approval.approval.expires_at_ms(),
        })
    }

    pub fn ingest_connector_registry(
        &self,
        bytes: Vec<u8>,
        root: ConnectorRegistryRoot,
        previous: Option<ConnectorRegistryCheckpoint>,
        policy: ConnectorTrustPolicy,
        now_ms: i64,
    ) -> Result<ConnectorRegistrySnapshot, FfiError> {
        let root = connector::registry_root_from_ffi(root)?;
        let previous = previous
            .map(connector::registry_checkpoint_from_ffi)
            .transpose()?;
        let policy = connector::policy_from_ffi(policy)?;
        let snapshot = mav_connector_runtime::ingest_registry(
            &bytes,
            &root,
            previous.as_ref(),
            &policy,
            now_ms,
        )?;
        Ok(connector::registry_snapshot_to_ffi(snapshot))
    }

    pub fn restore_connector_registry(
        &self,
        bytes: Vec<u8>,
        root: ConnectorRegistryRoot,
        checkpoint: ConnectorRegistryCheckpoint,
        policy: ConnectorTrustPolicy,
        now_ms: i64,
    ) -> Result<ConnectorRegistrySnapshot, FfiError> {
        let root = connector::registry_root_from_ffi(root)?;
        let checkpoint = connector::registry_checkpoint_from_ffi(checkpoint)?;
        let policy = connector::policy_from_ffi(policy)?;
        let snapshot =
            mav_connector_runtime::restore_registry(&bytes, &root, &checkpoint, &policy, now_ms)?;
        Ok(connector::registry_snapshot_to_ffi(snapshot))
    }

    pub fn verify_connector_registry_artifact(
        &self,
        entry: ConnectorRegistryEntry,
        bytes: Vec<u8>,
    ) -> Result<(), FfiError> {
        connector::registry_entry_from_ffi(entry)?.verify_artifact(&bytes)?;
        Ok(())
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
        let build = || -> Result<mav_engine::ConnectorHost, FfiError> {
            let mut host = mav_engine::ConnectorHost::instantiate(
                &artifact,
                mav_connector_runtime::LimitProfile::mobile_v1(),
                mav_engine::Store::open(std::path::Path::new(&self.database_path))?,
                mav_engine::ConnectorHostConfig {
                    session_id: config.session_id,
                    device_id: config.device_id,
                    transport_capacity: config.transport_capacity,
                },
            )?;
            host.set_tap(Arc::new(mav_obs::RingLogTap(Arc::clone(&self.ring))));
            // Carry the user's power choice into the new session before it activates, so a
            // reconnect never silently returns to full power (ADR-030).
            host.set_low_power(
                self.low_power.load(core::sync::atomic::Ordering::Relaxed),
                Some(config.now_ms),
            )?;
            Ok(host)
        };

        // Resume from whatever the last session committed. A connector that cannot read its own
        // stored bytes — a downgrade, a corrupted row — starts fresh instead of failing the
        // session, and the unreadable row is dropped rather than left to fail every reconnect.
        // The wearer gets a connector that reconnects; the journal gets the reason.
        let mut host = build()?;
        match self.load_connector_state(&host)? {
            Some(bytes) => {
                if let Err(error) = host.start_restored(&bytes) {
                    self.discard_unreadable_state(&host, &error, config.now_ms);
                    // A part-way restore leaves the host failed, so the fresh start needs a fresh
                    // instance rather than a second `start` on this one.
                    host = build()?;
                    host.start()?;
                }
            }
            None => host.start()?,
        }

        let report = host.lifecycle_snapshot().into();
        let mut session = self.connector_session_lock()?;
        if let Some(previous) = session.as_mut() {
            // The outgoing session may have committed since its last write; give it the chance to
            // land before it is replaced, then start this one's revision count from zero.
            self.persist_connector_state(previous, config.now_ms);
            previous.terminate(mav_connector_abi::CancelReason::Update, Some(config.now_ms))?;
        }
        if let Ok(mut persisted) = self.persisted_state_revision.lock() {
            *persisted = 0;
        }
        // Activation itself can commit — a restored connector re-stating what it learned — so the
        // first persist happens here rather than waiting for the first transport event.
        self.persist_connector_state(&mut host, config.now_ms);
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
        let outcome = host.apply(connector::event_from_ffi(event), wall_time_ms)?;
        // Anything the connector committed while handling that event becomes durable here, so a
        // process death after this call resumes from it rather than from the last session.
        self.persist_connector_state(host, wall_time_ms.unwrap_or_default());
        Ok(outcome.into())
    }

    pub fn cancel_connector_session(
        &self,
        reason: ConnectorCancelReason,
        wall_time_ms: Option<i64>,
    ) -> Result<ConnectorLifecycleReport, FfiError> {
        let mut session = self.connector_session_lock()?;
        let host = session.as_mut().ok_or_else(no_connector_session)?;
        host.cancel(connector::cancel_from_ffi(reason), wall_time_ms)?;
        // A cancelled session still knows where it got to. Persisting here is what makes a
        // disconnect-and-reconnect resume instead of restart.
        self.persist_connector_state(host, wall_time_ms.unwrap_or_default());
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

    /// Signed capture declarations intersected with the capabilities active for this hardware.
    pub fn connector_capture_capabilities(
        &self,
    ) -> Result<Vec<ConnectorCaptureCapability>, FfiError> {
        let session = self.connector_session_lock()?;
        let host = session.as_ref().ok_or_else(no_connector_session)?;
        Ok(host
            .available_captures()
            .into_iter()
            .map(|capture| ConnectorCaptureCapability {
                stream: capture.stream,
                unit: capture.unit,
                minimum_sample_rate_hz: capture.minimum_sample_rate_hz,
                maximum_sample_rate_hz: capture.maximum_sample_rate_hz,
            })
            .collect())
    }

    pub fn start_connector_capture(&self, stream: String, now_ms: i64) -> Result<(), FfiError> {
        let mut session = self.connector_session_lock()?;
        let host = session.as_mut().ok_or_else(no_connector_session)?;
        host.start_capture(&stream, Some(now_ms))?;
        Ok(())
    }

    pub fn stop_connector_capture(&self, stream: String, now_ms: i64) -> Result<(), FfiError> {
        let mut session = self.connector_session_lock()?;
        let host = session.as_mut().ok_or_else(no_connector_session)?;
        host.stop_capture(&stream, Some(now_ms))?;
        Ok(())
    }

    /// The live capture state. `now_ms` is the host wall clock the calibration deadline is judged
    /// against, so a capture whose stream never arrived fails visibly instead of calibrating for
    /// as long as the screen stays open.
    pub fn ecg_capture_state(&self, now_ms: i64) -> Result<Option<EcgCaptureReport>, FfiError> {
        let mut session = self.connector_session_lock()?;
        let host = session.as_mut().ok_or_else(no_connector_session)?;
        Ok(host
            .ecg_capture_snapshot_at(Some(now_ms))?
            .map(ecg_capture_report))
    }

    pub fn ecg_inference_request(&self) -> Result<Option<EcgInferenceRequest>, FfiError> {
        let session = self.connector_session_lock()?;
        let host = session.as_ref().ok_or_else(no_connector_session)?;
        Ok(host
            .ecg_inference_request()
            .map(|request| EcgInferenceRequest {
                capture_id: request.capture_id.get(),
                tensors: request
                    .tensors
                    .into_iter()
                    .map(|values| EcgTensor { values })
                    .collect(),
            }))
    }

    pub fn submit_ecg_inference(
        &self,
        capture_id: u64,
        predictions: Vec<EcgPrediction>,
        model_sha256: String,
        now_ms: i64,
    ) -> Result<EcgResultReport, FfiError> {
        let predictions = predictions
            .into_iter()
            .map(|values| {
                [
                    values.sinus_rhythm,
                    values.atrial_fibrillation,
                    values.other_abnormal_rhythm,
                ]
            })
            .collect();
        let mut session = self.connector_session_lock()?;
        let host = session.as_mut().ok_or_else(no_connector_session)?;
        host.submit_ecg_inference(
            EcgCaptureId::new(capture_id),
            predictions,
            model_sha256,
            now_ms,
        )
        .map(ecg_result_report)
        .map_err(Into::into)
    }

    pub fn ecg_results(
        &self,
        device_id: u64,
        limit: u32,
    ) -> Result<Vec<EcgResultReport>, FfiError> {
        let store = self.reader_lock()?;
        let device = DeviceId::new(device_id);
        // Reinterpret any capture whose result went missing while its evidence survived. Older
        // installs cleared results as if they were derived from samples, so a wearer's history can
        // arrive here as evidence with nothing to show for it; interpretation is deterministic, so
        // rebuilding is a repair rather than a new claim.
        for capture_id in store.ecg_evidence_without_result(device)? {
            if let Some(evidence) = store.ecg_inference(capture_id)? {
                store
                    .upsert_ecg_result(&mav_engine::ecg_capture::interpret_evidence(&evidence)?)?;
            }
        }
        store
            .ecg_results(device, limit as usize)
            .map(|results| results.into_iter().map(ecg_result_report).collect())
            .map_err(Into::into)
    }

    /// Forget one reading entirely: result, evidence and the samples behind it.
    pub fn delete_ecg_capture(&self, capture_id: u64) -> Result<bool, FfiError> {
        let store = self.reader_lock()?;
        store
            .delete_ecg_capture(EcgCaptureId::new(capture_id))
            .map_err(Into::into)
    }

    pub fn ecg_report_payload(
        &self,
        capture_id: u64,
    ) -> Result<Option<EcgReportPayload>, FfiError> {
        let store = self.reader_lock()?;
        let capture_id = EcgCaptureId::new(capture_id);
        let Some(evidence) = store.ecg_inference(capture_id)? else {
            return Ok(None);
        };
        let Some(result) = store.ecg_result(capture_id)? else {
            return Ok(None);
        };
        let waveform = store
            .samples_between(
                evidence.device_id,
                StreamKind::Ecg,
                mav_model::time::WallTime::from_nanos(evidence.started_ns),
                mav_model::time::WallTime::from_nanos(evidence.ended_ns),
            )?
            .into_iter()
            .map(|sample| sample.value.as_f64() as f32)
            .collect();
        Ok(Some(EcgReportPayload {
            result: ecg_result_report(result),
            source_unit: evidence.source_unit,
            waveform,
        }))
    }

    /// Replace the timezone the analytics bucket days by. The platform owns the zone database and
    /// supplies explicit spans; an empty span list is refused rather than silently treated as UTC,
    /// because a wrong day boundary silently moves a night into the wrong snapshot.
    pub fn set_timezone_spans(
        &self,
        timezone_id: String,
        spans: Vec<TimezoneSpan>,
    ) -> Result<(), FfiError> {
        let spans = spans
            .into_iter()
            .map(|span| mav_engine::OffsetSpan {
                start_unix_seconds: span.start_unix_seconds,
                offset_seconds: span.offset_seconds,
            })
            .collect();
        let timezone = mav_engine::Timezone::new(timezone_id, spans)?;
        let store = self.reader_lock()?;
        self.spine_lock()?.set_timezone(&store, timezone)?;
        Ok(())
    }

    /// The analytics for the local day containing `wall_time_ms`, recomputing if the day is not
    /// cached and persisting what it computes.
    pub fn daily_snapshot(
        &self,
        device_id: u64,
        wall_time_ms: i64,
    ) -> Result<DailySnapshotReport, FfiError> {
        let spine = self.spine_lock()?;
        let day = spine.day_of(mav_model::time::WallTime::from_nanos(
            wall_time_ms.saturating_mul(1_000_000),
        ));
        let store = self.reader_lock()?;
        let snapshot = spine.snapshot(
            &store,
            DeviceId::new(device_id),
            day,
            wall_time_ms.saturating_mul(1_000_000),
        )?;
        Ok(snapshot_report(&snapshot))
    }

    /// One snapshot per local day in `[from_ms, to_ms]`, oldest first. The history surfaces need a
    /// range, and asking for one day at a time would recompute the longitudinal look-back once per
    /// day rendered.
    pub fn daily_snapshots(
        &self,
        device_id: u64,
        from_ms: i64,
        to_ms: i64,
    ) -> Result<Vec<DailySnapshotReport>, FfiError> {
        let spine = self.spine_lock()?;
        let store = self.reader_lock()?;
        let device = DeviceId::new(device_id);
        let day_of = |at_ms: i64| {
            spine
                .day_of(mav_model::time::WallTime::from_nanos(
                    at_ms.saturating_mul(1_000_000),
                ))
                .index()
        };
        let (first, last) = (day_of(from_ms.min(to_ms)), day_of(to_ms.max(from_ms)));
        (first..=last)
            .map(|index| {
                let day = mav_engine::LocalDay::from_index(index);
                Ok(snapshot_report(&spine.snapshot(
                    &store,
                    device,
                    day,
                    to_ms.saturating_mul(1_000_000),
                )?))
            })
            .collect()
    }

    /// Which analytics the day can serve, and why not for the rest. The same list the snapshot
    /// carries, for a caller that only needs to decide what to render.
    pub fn analytic_availability(
        &self,
        device_id: u64,
        wall_time_ms: i64,
    ) -> Result<Vec<AnalyticAvailabilityReport>, FfiError> {
        Ok(self.daily_snapshot(device_id, wall_time_ms)?.availability)
    }

    /// The Tanaka age estimate of maximum heart rate, and the five %HRmax band edges in bpm. One
    /// implementation of zone math, in the core, because two languages computing the same ladder is
    /// two answers waiting to disagree.
    pub fn heart_rate_zones(&self, age: f64, max_hr_override: Option<f64>) -> HeartRateZones {
        let zones = mav_analytic::hr_zones::zones_for_age(age, max_hr_override);
        HeartRateZones {
            max_hr: zones.max_hr,
            source: zones.source.clone(),
            lower_bpm: zones.zones.iter().map(|zone| zone.lower).collect(),
        }
    }

    /// The zone (1..=5) a reading falls in, or 0 below zone one.
    pub fn heart_rate_zone_for(&self, bpm: f64, age: f64, max_hr_override: Option<f64>) -> u8 {
        mav_analytic::hr_zones::zones_for_age(age, max_hr_override).zone_number(bpm)
    }

    /// Everything a bug report needs and nothing a user would not want to send.
    pub fn export_report_bundle(&self, limit: u32) -> Result<ReportBundle, FfiError> {
        let session = self.connector_session_lock()?;
        let lifecycle = session.as_ref().map(|host| host.lifecycle_snapshot());
        let connector_id = session.as_ref().map(|host| host.connector_id().to_owned());
        drop(session);

        Ok(ReportBundle {
            app_version: self.app_version.clone(),
            connector_id,
            session_id: lifecycle.as_ref().map(|state| state.session_id),
            trace_hash: lifecycle.as_ref().map(|state| state.trace_hash.clone()),
            samples_persisted: lifecycle
                .as_ref()
                .map_or(0, |state| state.samples_persisted),
            samples_duplicate: lifecycle
                .as_ref()
                .map_or(0, |state| state.samples_duplicate),
            recent_stages: self
                .ring
                .recent(limit as usize)
                .into_iter()
                .map(observed_stage)
                .collect(),
        })
    }

    /// The live readout for the open session.
    ///
    /// `now_ms` is what makes it live. A heart rate is a statement about this moment, so a reading
    /// older than [`LIVE_HEART_RATE_WINDOW_MS`] is reported as absent rather than as the current
    /// value — the newest row in the store is not the same claim as "your heart rate is this". A
    /// battery percentage and a wrist flag are slow-moving device *state*, so they survive a longer
    /// window and `last_sample_wall_time_ms` lets a screen say how old they are.
    /// Trade data density for battery on both the phone and the strap (ADR-030). The connector
    /// keeps its primary vitals stream either way; what it gives up is diagnostic subscriptions,
    /// raw streams, and how often it pulls the historical offload.
    ///
    /// Applies to the running session and is stated again to any session started afterwards, so a
    /// reconnect cannot silently return to full power. Returns whether the mode actually changed.
    pub fn set_low_power(&self, low_power: bool, now_ms: i64) -> Result<bool, FfiError> {
        self.low_power
            .store(low_power, core::sync::atomic::Ordering::Relaxed);
        let mut session = self.connector_session_lock()?;
        let Some(host) = session.as_mut() else {
            return Ok(true);
        };
        Ok(host.set_low_power(low_power, Some(now_ms))?)
    }

    /// The power policy currently in force, whether or not a session is running.
    pub fn low_power(&self) -> bool {
        self.low_power.load(core::sync::atomic::Ordering::Relaxed)
    }

    pub fn connector_telemetry(&self, now_ms: i64) -> Result<ConnectorTelemetrySnapshot, FfiError> {
        let session = self.connector_session_lock()?;
        let host = session.as_ref().ok_or_else(no_connector_session)?;
        let lifecycle = host.lifecycle_snapshot();
        let connector_id = host.connector_id().to_owned();
        let device_id = host.device_id();
        drop(session);

        let store = self.reader_lock()?;
        let device = DeviceId::new(device_id);
        let heart_rate = store.latest_sample(device, StreamKind::HeartRate)?;
        let battery = store.latest_sample(device, StreamKind::BatterySoc)?;
        let wrist = store.latest_sample(device, StreamKind::WristState)?;
        let last_sample_wall_time_ms = [&heart_rate, &battery, &wrist]
            .into_iter()
            .filter_map(|sample| sample.as_ref()?.wall_time())
            .map(|time| time.as_nanos().div_euclid(1_000_000))
            .max();

        let heart_rate = fresh(heart_rate, now_ms, LIVE_HEART_RATE_WINDOW_MS);
        let battery = fresh(battery, now_ms, DEVICE_STATE_WINDOW_MS);
        let wrist = fresh(wrist, now_ms, DEVICE_STATE_WINDOW_MS);

        Ok(ConnectorTelemetrySnapshot {
            connector_id,
            lifecycle: ConnectorLifecycleReport::from(lifecycle.clone()).lifecycle,
            session_id: lifecycle.session_id,
            cancellation_generation: lifecycle.cancellation_generation,
            device_id,
            heart_rate_bpm: bounded_sample(&heart_rate, 1, 300).map(|value| value as u16),
            battery_percent: bounded_sample(&battery, 0, 100).map(|value| value as u8),
            on_wrist: bounded_sample(&wrist, 0, 1).map(|value| value == 1),
            last_sample_wall_time_ms,
        })
    }
}

/// How recent a heart-rate sample must be to be shown as the current one. Ninety seconds is
/// several beats' worth of missed notifications, so a live link never flickers, and a strap that
/// has stopped reporting goes blank instead of freezing on its last number.
const LIVE_HEART_RATE_WINDOW_MS: i64 = 90_000;
/// Battery and wrist state change slowly and remain true between readings, so they survive a much
/// longer gap — but not an unbounded one, or a week-old percentage reads as current.
const DEVICE_STATE_WINDOW_MS: i64 = 6 * 60 * 60 * 1_000;

/// Keep a sample only if it is recent enough to still be a claim about now. A sample with no wall
/// time was never placed on the clock and cannot be judged, so it is dropped.
fn fresh(
    sample: Option<Sample<mav_model::raw::RawValue>>,
    now_ms: i64,
    window_ms: i64,
) -> Option<Sample<mav_model::raw::RawValue>> {
    let at = sample
        .as_ref()?
        .wall_time()?
        .as_nanos()
        .div_euclid(1_000_000);
    (now_ms.saturating_sub(at) <= window_ms).then_some(sample?)
}

fn snapshot_report(snapshot: &mav_engine::DailySnapshot) -> DailySnapshotReport {
    DailySnapshotReport {
        day: snapshot.day.clone(),
        day_index: snapshot.day_index,
        current_bpm: snapshot.heart_rate.current_bpm,
        mean_bpm: snapshot.heart_rate.mean_bpm,
        hr_sample_count: snapshot.heart_rate.sample_count,
        hr_excluded_count: snapshot.heart_rate.excluded_count,
        hrv: snapshot.hrv.as_ref().map(|hrv| HrvReport {
            label: hrv.label.clone(),
            mean_interval_ms: hrv.mean_interval_ms,
            rmssd_ms: hrv.rmssd_ms,
            sdnn_ms: hrv.sdnn_ms,
            pnn50_percent: hrv.pnn50_percent,
            sd1_ms: hrv.sd1_ms,
            sd2_ms: hrv.sd2_ms,
            alpha1: hrv.alpha1,
            interval_count: hrv.interval_count,
            excluded_count: hrv.excluded_count,
        }),
        hrv_spectrum: snapshot.hrv_spectrum.map(|bands| HrvSpectrumReport {
            vlf_power_ms2: bands.vlf_power_ms2,
            lf_power_ms2: bands.lf_power_ms2,
            hf_power_ms2: bands.hf_power_ms2,
            total_power_ms2: bands.total_power_ms2,
            lf_normalized: bands.lf_normalized,
            hf_normalized: bands.hf_normalized,
            lf_hf_ratio: bands.lf_hf_ratio,
            span_seconds: bands.span_seconds,
        }),
        readiness: snapshot.readiness.map(|readiness| ReadinessReport {
            tier: format!("{:?}", readiness.tier).to_lowercase(),
            baseline7_ms: readiness.baseline7_ms,
            normal_low_ms: readiness.normal_low_ms,
            normal_high_ms: readiness.normal_high_ms,
            overreaching_watch: readiness.overreaching_watch,
        }),
        availability: snapshot
            .availability
            .iter()
            .map(availability_report)
            .collect(),
        algorithms: snapshot
            .algorithms
            .iter()
            .map(|stamp| format!("{}@{}", stamp.id, stamp.version))
            .collect(),
        snapshot_hash: snapshot_hash(snapshot),
    }
}

fn availability_report(entry: &mav_analytic::AnalyticAvailability) -> AnalyticAvailabilityReport {
    let (reason, missing_streams) = match &entry.reason {
        Some(mav_analytic::UnavailableReason::MissingStreams { streams }) => (
            Some("missing_streams".to_owned()),
            streams
                .iter()
                .map(|stream| stream.name().to_owned())
                .collect(),
        ),
        Some(mav_analytic::UnavailableReason::AlgorithmNotAdmitted) => {
            (Some("algorithm_not_admitted".to_owned()), Vec::new())
        }
        None => (None, Vec::new()),
    };
    AnalyticAvailabilityReport {
        analytic: format!("{:?}", entry.analytic).to_lowercase(),
        available: entry.available,
        reason,
        missing_streams,
    }
}

/// A stable digest over the snapshot's canonical serialization. Computed in the shared core, so
/// both platforms read the same string from the same day — that equality is the parity contract,
/// and a divergence means a binding is lying rather than a metric differing.
fn snapshot_hash(snapshot: &mav_engine::DailySnapshot) -> String {
    let canonical = serde_json::to_string(snapshot).unwrap_or_default();
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in canonical.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

fn observed_stage(entry: mav_obs::RingEntry) -> ObservedStage {
    let (kind, count, detail) = match entry.kind {
        mav_obs::RingEntryKind::Produced { count, summary } => {
            ("produced", count as u32, summary.unwrap_or_default())
        }
        mav_obs::RingEntryKind::Rejected { code, message, .. } => {
            ("rejected", u32::from(code), message)
        }
        mav_obs::RingEntryKind::Transition { from, to } => {
            ("transition", 0, format!("{from} -> {to}"))
        }
    };
    ObservedStage {
        seq: entry.seq,
        stage: entry.stage.name().to_owned(),
        kind: kind.to_owned(),
        count,
        detail,
    }
}

fn bounded_sample(
    sample: &Option<Sample<mav_model::raw::RawValue>>,
    minimum: u32,
    maximum: u32,
) -> Option<u32> {
    let sample = sample.as_ref()?;
    if !sample.quality.is_usable() {
        return None;
    }
    let value = sample.value.as_f64();
    if !value.is_finite() || value < f64::from(minimum) || value > f64::from(maximum) {
        return None;
    }
    Some(value.round() as u32)
}

fn ecg_capture_report(snapshot: mav_engine::ecg_capture::EcgCaptureSnapshot) -> EcgCaptureReport {
    EcgCaptureReport {
        capture_id: snapshot.capture_id.get(),
        phase: snapshot.phase.name().to_owned(),
        progress_milli: snapshot.progress_milli,
        quality_milli: snapshot.quality_milli,
        quality_reason: snapshot.quality_reason,
        recorded_samples: snapshot.recorded_samples,
        target_samples: snapshot.target_samples,
    }
}

/// The four at-a-glance checks, derived once in the core.
///
/// The rate thresholds are the range this classifier was validated over; outside it the rhythm
/// call is not something the model has earned, which is why the rate checks are separate findings
/// rather than folded into the rhythm. A reading with no measurable rate reports both rate checks
/// as unknown rather than passing them by default.
fn ecg_checks(result: &mav_model::ecg::EcgResult) -> Vec<EcgCheck> {
    use mav_model::ecg::EcgRhythmClass;
    const LOW_BPM: u16 = 50;
    const HIGH_BPM: u16 = 120;
    let rate = result.mean_heart_rate_bpm;
    vec![
        EcgCheck {
            id: "afib".to_owned(),
            passed: result.rhythm != EcgRhythmClass::AtrialFibrillation,
            known: true,
        },
        EcgCheck {
            id: "high_heart_rate".to_owned(),
            passed: rate.is_some_and(|bpm| bpm <= HIGH_BPM),
            known: rate.is_some(),
        },
        EcgCheck {
            id: "low_heart_rate".to_owned(),
            passed: rate.is_some_and(|bpm| bpm >= LOW_BPM),
            known: rate.is_some(),
        },
        EcgCheck {
            id: "sinus_rhythm".to_owned(),
            passed: result.rhythm == EcgRhythmClass::SinusRhythm,
            known: true,
        },
    ]
}

fn ecg_result_report(result: mav_model::ecg::EcgResult) -> EcgResultReport {
    EcgResultReport {
        capture_id: result.capture_id.get(),
        device_id: result.device_id.get(),
        started_ns: result.started_ns,
        ended_ns: result.ended_ns,
        source_rate_hz: result.source_rate_hz,
        sample_count: result.sample_count,
        rhythm: match result.rhythm {
            mav_model::ecg::EcgRhythmClass::SinusRhythm => "sinus_rhythm",
            mav_model::ecg::EcgRhythmClass::AtrialFibrillation => "atrial_fibrillation",
            mav_model::ecg::EcgRhythmClass::OtherAbnormalRhythm => "other_abnormal_rhythm",
        }
        .to_owned(),
        sinus_probability: result.probabilities[0],
        atrial_fibrillation_probability: result.probabilities[1],
        other_abnormal_probability: result.probabilities[2],
        confidence_milli: result.confidence_milli,
        mean_heart_rate_bpm: result.mean_heart_rate_bpm,
        checks: ecg_checks(&result),
        quality_milli: result.quality_milli,
        explanation: result
            .explanation
            .into_iter()
            .map(|segment| EcgExplanation {
                start_second: segment.start_second,
                end_second: segment.end_second,
                importance_milli: segment.importance_milli,
            })
            .collect(),
        raw_sha256: result.raw_sha256,
        tensor_sha256: result.tensor_sha256,
        preprocessing_sha256: result.preprocessing_sha256,
        model_sha256: result.model_sha256,
        algorithm_id: result.algorithm_id,
        algorithm_version: result.algorithm_version,
        provisional: result.provisional,
    }
}

impl MavRuntime {
    pub(crate) fn spine_lock(&self) -> Result<MutexGuard<'_, mav_engine::Spine>, FfiError> {
        self.spine.lock().map_err(|_| poisoned("analytic spine"))
    }

    pub(crate) fn reader_lock(&self) -> Result<MutexGuard<'_, mav_engine::Store>, FfiError> {
        self.reader.lock().map_err(|_| poisoned("evidence store"))
    }

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
            // Terminate cancels, which is itself a chance for the connector to commit. Persist
            // before the host goes, or the last thing it learned dies with the session.
            self.persist_connector_state(host, now_ms);
        }
        *session = None;
        Ok(())
    }

    /// What the last session committed for this connector on this device, if anything.
    fn load_connector_state(
        &self,
        host: &mav_engine::ConnectorHost,
    ) -> Result<Option<Vec<u8>>, FfiError> {
        let namespace = Self::state_namespace(host);
        Ok(self
            .connectors_lock()?
            .load_state(&namespace)?
            .map(|state| state.bytes))
    }

    /// Drop state the connector could not read, and say why in the journal.
    ///
    /// Keeping it would make every future reconnect fail the same way, which turns one bad row
    /// into a permanently broken device. Dropping it costs whatever the connector had learned —
    /// a history cursor, a pairing step — all of which it can learn again.
    fn discard_unreadable_state(
        &self,
        host: &mav_engine::ConnectorHost,
        failure: &mav_model::error::MavError,
        now_ms: i64,
    ) {
        let namespace = Self::state_namespace(host);
        let dropped = self
            .connectors_lock()
            .and_then(|mut connectors| connectors.clear_state(&namespace).map_err(Into::into));
        let mut record = MavError::new(
            mav_model::error::codes::CONNECTOR_INSTALL_STATE_NAMESPACE,
            "connector refused its own stored state; starting fresh and dropping the row",
        )
        .context(format!("{failure:?}"));
        if let Err(error) = dropped {
            record = record.context(format!("and the row could not be dropped: {error:?}"));
        }
        if let Ok(store) = self.reader_lock() {
            let _ = store.record_error(&record, now_ms.saturating_mul(1_000_000));
        }
    }

    /// The state namespace one session writes to, from the manifest the store already trusts.
    fn state_namespace(host: &mav_engine::ConnectorHost) -> mav_connector_store::StateNamespace {
        let (publisher_key_id, state_schema) = host.state_namespace();
        mav_connector_store::StateNamespace {
            connector_id: host.connector_id().to_owned(),
            publisher_key_id: publisher_key_id.to_owned(),
            // The store keys state per device, and the host's device id is the only identity a
            // session has. Rendered rather than typed as a number because the column is a text
            // namespace shared with connector-chosen ids.
            device_id: host.device_id().to_string(),
            state_schema,
        }
    }

    /// Write the connector's own snapshot through to the install store, if it has moved.
    ///
    /// Called after anything that can dispatch an event. Cheap when nothing committed: the
    /// revision has not moved, so no connector code runs.
    ///
    /// Failures are recorded and swallowed on purpose. This runs after the event it belongs to has
    /// already been handled and its samples already committed; turning a state-write failure into a
    /// failed transport event would throw away good data to report a durability problem, and the
    /// next commit will try again. What must never happen is silence, so it goes to the journal.
    fn persist_connector_state(&self, host: &mut mav_engine::ConnectorHost, now_ms: i64) {
        let revision = host.state_revision();
        {
            let mut persisted = match self.persisted_state_revision.lock() {
                Ok(guard) => guard,
                Err(_) => return,
            };
            if revision == 0 || revision == *persisted {
                return;
            }
            *persisted = revision;
        }
        if let Err(error) = self.write_connector_state(host, now_ms) {
            self.journal_state_failure(&error, now_ms);
        }
    }

    /// A state write that failed goes to the durable journal, never nowhere.
    ///
    /// If the journal write fails too there is genuinely nothing left to do about it — the caller
    /// is a transport event that has already succeeded — so it is dropped here and only here.
    fn journal_state_failure(&self, failure: &FfiError, now_ms: i64) {
        let record = MavError::new(
            mav_model::error::codes::CONNECTOR_INSTALL_STATE_NAMESPACE,
            "connector state could not be made durable; the session continues and will retry \
             on its next commit",
        )
        .context(format!("{failure:?}"));
        if let Ok(store) = self.reader_lock() {
            let _ = store.record_error(&record, now_ms.saturating_mul(1_000_000));
        }
    }

    fn write_connector_state(
        &self,
        host: &mut mav_engine::ConnectorHost,
        now_ms: i64,
    ) -> Result<(), FfiError> {
        let bytes = host.snapshot_state()?;
        let namespace = Self::state_namespace(host);
        let mut connectors = self.connectors_lock()?;
        connectors
            .save_state(&mav_connector_store::StoredState::new(
                namespace, bytes, now_ms,
            ))
            .map_err(Into::into)
    }
}

pub(crate) fn poisoned(owner: &str) -> FfiError {
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
