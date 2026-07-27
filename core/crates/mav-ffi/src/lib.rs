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
use mav_model::ids::DeviceId;
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

#[derive(uniffi::Object)]
pub struct MavRuntime {
    connectors: Mutex<mav_connector_store::ConnectorRepository>,
    connector_session: Mutex<Option<mav_engine::ConnectorHost>>,
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
            reader: Mutex::new(reader),
            spine: Mutex::new(mav_engine::Spine::new(mav_engine::Timezone::fixed(
                &config.timezone_id,
                0,
            ))),
            database_path,
            ring: Arc::new(mav_obs::RingLog::new(RING_CAPACITY)),
            app_version: config.app_version,
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
        host.set_tap(Arc::new(mav_obs::RingLogTap(Arc::clone(&self.ring))));
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

impl MavRuntime {
    fn spine_lock(&self) -> Result<MutexGuard<'_, mav_engine::Spine>, FfiError> {
        self.spine.lock().map_err(|_| poisoned("analytic spine"))
    }

    fn reader_lock(&self) -> Result<MutexGuard<'_, mav_engine::Store>, FfiError> {
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
