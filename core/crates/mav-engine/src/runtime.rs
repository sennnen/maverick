use crate::pipeline::{IngestStats, RealtimeProcessor};
use crate::snapshot::{fnv1a_64, AnalyticsSnapshot, Snapshot};
use mav_codec::codec::DeviceCodec;
use mav_codec::manifest::Manifest;
use mav_model::error::{codes, Category, MavError, Result, Severity};
use mav_model::ids::DeviceId;
use mav_model::raw::RawValue;
use mav_model::stream::StreamKind;
use mav_model::time::WallTime;
use mav_model::version::Version;
use mav_obs::stage::Stage;
use mav_obs::tap::{Tap, TapEvent};
use mav_store::{JournalEntry, Store};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::sync::Mutex;

pub const HOST_SNAPSHOT_SCHEMA: &str = "host-snapshot/v1";

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RuntimeConfig {
    pub database_path: String,
    pub timezone_id: String,
    pub transport_capacity: u32,
    pub app_version: String,
    pub app_build: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ConnectorRegistration {
    pub connector_id: String,
    pub connector_version: String,
    pub manifest_json: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionState {
    Disconnected,
    Scanning,
    Connecting,
    Subscribing,
    Streaming,
    Failed,
}

impl ConnectionState {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Disconnected => "disconnected",
            Self::Scanning => "scanning",
            Self::Connecting => "connecting",
            Self::Subscribing => "subscribing",
            Self::Streaming => "streaming",
            Self::Failed => "failed",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
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

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct HostConnection {
    pub state: ConnectionState,
    pub device_id: Option<u64>,
    pub connector_id: Option<String>,
    pub connector_version: Option<String>,
    pub display_name: Option<String>,
    pub battery_percent: Option<u8>,
    pub charging: Option<bool>,
    pub on_wrist: Option<bool>,
    pub last_sample_unix_ms: Option<i64>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct HostError {
    pub code: u16,
    pub category: Category,
    pub severity: Severity,
    pub message: String,
    pub context: Vec<String>,
    pub next_action: String,
    pub at_unix_ms: i64,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
struct HostSnapshotBody {
    schema: String,
    core_version: String,
    storage_schema: i64,
    as_of_unix_ms: i64,
    timezone_id: String,
    app_version: String,
    app_build: String,
    connection: HostConnection,
    session: Option<Snapshot>,
    analytics: Option<AnalyticsSnapshot>,
    historical: Option<serde_json::Value>,
    recent_errors: Vec<HostError>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct HostSnapshot {
    pub schema: String,
    pub core_version: String,
    pub storage_schema: i64,
    pub revision: u64,
    pub as_of_unix_ms: i64,
    pub timezone_id: String,
    pub app_version: String,
    pub app_build: String,
    pub connection: HostConnection,
    pub session: Option<Snapshot>,
    pub analytics: Option<AnalyticsSnapshot>,
    pub historical: Option<serde_json::Value>,
    pub recent_errors: Vec<HostError>,
}

impl HostSnapshot {
    pub fn canonical_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|error| {
            MavError::new(
                codes::STORAGE_SERIALIZE,
                "could not serialise the host snapshot",
            )
            .context(error.to_string())
        })
    }

    pub fn canonical_hash(&self) -> Result<String> {
        Ok(fnv1a_64(self.canonical_json()?.as_bytes()))
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct HostSnapshotResult {
    pub json: String,
    pub hash: String,
    pub revision: u64,
}

struct RegisteredConnector {
    version: Version,
    version_text: String,
    manifest: Manifest,
}

struct Session {
    connector_id: String,
    connector_version: String,
    device: DeviceId,
    native_device_id: Option<String>,
    display_name: Option<String>,
    pending_subscriptions: BTreeSet<String>,
    subscribed: BTreeSet<String>,
    last_sample_unix_ms: Option<i64>,
    processor: RealtimeProcessor,
}

/// Builds a fresh device-codec instance per session. Registered by the edge crate (the FFI, the
/// replay binary) at startup, because the engine never links a device crate (ADR-016).
pub type CodecFactory = Box<dyn Fn() -> Box<dyn DeviceCodec> + Send + Sync>;

pub struct HostRuntime {
    config: RuntimeConfig,
    store: Store,
    connectors: BTreeMap<String, RegisteredConnector>,
    codecs: BTreeMap<String, CodecFactory>,
    actions: VecDeque<TransportAction>,
    state: ConnectionState,
    session: Option<Session>,
    revision: u64,
    last_body_json: Option<String>,
    historical: crate::historical::HistoricalReport,
}

impl HostRuntime {
    pub fn open(config: RuntimeConfig) -> Result<Self> {
        validate_config(&config)?;
        let store = Store::open(Path::new(&config.database_path))?;
        Ok(Self {
            config,
            store,
            connectors: BTreeMap::new(),
            codecs: BTreeMap::new(),
            actions: VecDeque::new(),
            state: ConnectionState::Disconnected,
            session: None,
            revision: 0,
            last_body_json: None,
            historical: crate::historical::HistoricalReport::idle(),
        })
    }

    /// The progress and failure read model of the historical sync (`historical-status/v1`). Idle
    /// until a sync runs; the live transfer machinery updates it and hosts can only read it.
    pub fn historical_report(&self) -> &crate::historical::HistoricalReport {
        &self.historical
    }

    /// Register a device-codec factory under its id. The edge crate calls this once per built-in
    /// codec crate before installing connectors; a manifest whose `codec` names an id nothing
    /// registered does not install.
    pub fn register_codec(
        &mut self,
        id: &str,
        factory: impl Fn() -> Box<dyn DeviceCodec> + Send + Sync + 'static,
    ) {
        self.codecs.insert(id.to_owned(), Box::new(factory));
    }

    pub fn install_connector(&mut self, registration: ConnectorRegistration) -> Result<()> {
        if registration.connector_id.trim().is_empty() {
            return Err(runtime_state("connector id must not be empty"));
        }
        let version = registration
            .connector_version
            .parse::<Version>()
            .map_err(|error| {
                runtime_state("connector version must be semantic").context(error.to_string())
            })?;
        let manifest = Manifest::from_json(&registration.manifest_json)?;
        if let Some(codec_id) = manifest.codec.as_deref() {
            let factory = self.codecs.get(codec_id).ok_or_else(|| {
                MavError::new(
                    codes::DECODE_CODEC_UNAVAILABLE,
                    "manifest names a codec this runtime has not registered",
                )
                .context(codec_id.to_owned())
            })?;
            manifest.validate_against_codec(factory().as_ref())?;
        }
        if let Some(installed) = self.connectors.get(&registration.connector_id) {
            if version < installed.version {
                return Err(MavError::new(
                    codes::FFI_CONNECTOR_DOWNGRADE,
                    "connector downgrade refused",
                )
                .context(format!(
                    "{} {} -> {}",
                    registration.connector_id,
                    installed.version_text,
                    registration.connector_version
                )));
            }
        }
        self.connectors.insert(
            registration.connector_id,
            RegisteredConnector {
                version,
                version_text: registration.connector_version,
                manifest,
            },
        );
        Ok(())
    }

    pub fn start_scan(&mut self, connector_id: &str, device_id: u64) -> Result<()> {
        self.require_state(ConnectionState::Disconnected, "start_scan")?;
        let (service, version, display_name, manifest) = {
            let connector = self.connector(connector_id)?;
            (
                connector.manifest.gatt.service.clone(),
                connector.version_text.clone(),
                connector.manifest.identity.display_name.clone(),
                connector.manifest.clone(),
            )
        };
        let processor = match manifest.codec.as_deref() {
            None => RealtimeProcessor::new(manifest, DeviceId::new(device_id))?,
            Some(codec_id) => {
                let factory = self.codecs.get(codec_id).ok_or_else(|| {
                    MavError::new(
                        codes::DECODE_CODEC_UNAVAILABLE,
                        "manifest names a codec this runtime has not registered",
                    )
                    .context(codec_id.to_owned())
                })?;
                RealtimeProcessor::with_codec(manifest, DeviceId::new(device_id), factory())?
            }
        };
        self.enqueue_all(vec![TransportAction::StartScan {
            service_filters: vec![service],
        }])?;
        self.session = Some(Session {
            connector_id: connector_id.to_owned(),
            connector_version: version,
            device: DeviceId::new(device_id),
            native_device_id: None,
            display_name: Some(display_name),
            pending_subscriptions: BTreeSet::new(),
            subscribed: BTreeSet::new(),
            last_sample_unix_ms: None,
            processor,
        });
        self.state = ConnectionState::Scanning;
        Ok(())
    }

    pub fn device_discovered(
        &mut self,
        connector_id: &str,
        native_device_id: String,
        display_name: Option<String>,
    ) -> Result<()> {
        self.require_state(ConnectionState::Scanning, "device_discovered")?;
        let session = self.session_ref()?;
        if session.connector_id != connector_id {
            return Err(runtime_state(
                "discovered device belongs to a different connector",
            ));
        }
        self.enqueue_all(vec![
            TransportAction::StopScan,
            TransportAction::Connect {
                native_device_id: native_device_id.clone(),
            },
        ])?;
        let session = self.session_mut()?;
        session.native_device_id = Some(native_device_id);
        if display_name.is_some() {
            session.display_name = display_name;
        }
        self.state = ConnectionState::Connecting;
        Ok(())
    }

    pub fn connected(&mut self, native_device_id: &str) -> Result<()> {
        self.require_state(ConnectionState::Connecting, "connected")?;
        self.require_native_device(native_device_id)?;
        let connector_id = self.session_ref()?.connector_id.clone();
        let notify = self.connector(&connector_id)?.manifest.gatt.notify.clone();
        let actions = notify
            .iter()
            .cloned()
            .map(|characteristic| TransportAction::Subscribe { characteristic })
            .collect();
        self.enqueue_all(actions)?;
        let session = self.session_mut()?;
        session.pending_subscriptions = notify.into_iter().collect();
        self.state = if session.pending_subscriptions.is_empty() {
            ConnectionState::Streaming
        } else {
            ConnectionState::Subscribing
        };
        Ok(())
    }

    pub fn subscribed(&mut self, characteristic: &str) -> Result<()> {
        self.require_state(ConnectionState::Subscribing, "subscribed")?;
        let session = self.session_mut()?;
        if !session.pending_subscriptions.remove(characteristic) {
            return Err(
                runtime_state("subscription acknowledgement was not pending")
                    .context(characteristic.to_owned()),
            );
        }
        session.subscribed.insert(characteristic.to_owned());
        if session.pending_subscriptions.is_empty() {
            self.state = ConnectionState::Streaming;
        }
        Ok(())
    }

    pub fn notification(
        &mut self,
        characteristic: &str,
        bytes: &[u8],
        at_unix_ms: i64,
    ) -> Result<IngestStats> {
        self.require_state(ConnectionState::Streaming, "notification")?;
        let wall = wall_time(at_unix_ms)?;
        let tap = ErrorCollectingTap::default();
        let result = {
            let store = &self.store;
            let session = self
                .session
                .as_mut()
                .ok_or_else(|| runtime_state("no active session"))?;
            if !session.subscribed.contains(characteristic) {
                return Err(
                    runtime_state("notification came from an unsubscribed characteristic")
                        .context(characteristic.to_owned()),
                );
            }
            session.processor.ingest_chunk(bytes, wall, store, &tap)
        };
        self.persist_tap_errors(&tap, at_unix_ms)?;
        let stats = match result {
            Ok(stats) => stats,
            Err(error) => {
                self.record_error(&error, at_unix_ms)?;
                return Err(error);
            }
        };
        if stats.inserted > 0 {
            self.session_mut()?.last_sample_unix_ms = Some(at_unix_ms);
        }
        Ok(stats)
    }

    pub fn transport_failed(
        &mut self,
        operation: &str,
        native_code: &str,
        safe_message: &str,
        at_unix_ms: i64,
    ) -> Result<()> {
        let error = MavError::new(
            codes::TRANSPORT_NATIVE_FAILURE,
            "native transport operation failed",
        )
        .context(operation.to_owned())
        .context(native_code.to_owned())
        .context(safe_message.to_owned());
        self.record_error(&error, at_unix_ms)?;
        self.state = ConnectionState::Failed;
        Ok(())
    }

    pub fn disconnected(&mut self, native_device_id: &str) -> Result<()> {
        self.require_native_device(native_device_id)?;
        self.state = ConnectionState::Disconnected;
        if let Some(session) = &mut self.session {
            session.pending_subscriptions.clear();
            session.subscribed.clear();
        }
        Ok(())
    }

    pub fn drain_actions(&mut self, limit: u32) -> Vec<TransportAction> {
        let take = usize::try_from(limit)
            .unwrap_or(usize::MAX)
            .min(self.actions.len());
        self.actions.drain(..take).collect()
    }

    pub fn connection_state(&self) -> ConnectionState {
        self.state
    }

    pub fn host_snapshot(&mut self, at_unix_ms: i64) -> Result<HostSnapshotResult> {
        wall_time(at_unix_ms)?;
        let tap = ErrorCollectingTap::default();
        let output = match &self.session {
            Some(session) => Some(session.processor.output(&self.store, &tap)),
            None => None,
        };
        self.persist_tap_errors(&tap, at_unix_ms)?;
        let output = match output {
            Some(Ok(output)) => Some(output),
            Some(Err(error)) => {
                self.record_error(&error, at_unix_ms)?;
                return Err(error);
            }
            None => None,
        };
        let recent_errors = self
            .store
            .recent_errors(32)?
            .into_iter()
            .map(host_error)
            .collect();
        let connection = self.connection_snapshot()?;
        let body = HostSnapshotBody {
            schema: HOST_SNAPSHOT_SCHEMA.to_owned(),
            core_version: env!("CARGO_PKG_VERSION").to_owned(),
            storage_schema: self.store.schema_version()?,
            as_of_unix_ms: at_unix_ms,
            timezone_id: self.config.timezone_id.clone(),
            app_version: self.config.app_version.clone(),
            app_build: self.config.app_build.clone(),
            connection,
            session: output.as_ref().map(|value| value.snapshot.clone()),
            analytics: output.map(|value| value.analytics),
            historical: None,
            recent_errors,
        };
        let body_json = serde_json::to_string(&body).map_err(|error| {
            MavError::new(
                codes::STORAGE_SERIALIZE,
                "could not serialise the host snapshot body",
            )
            .context(error.to_string())
        })?;
        if self.last_body_json.as_deref() != Some(body_json.as_str()) {
            self.revision = self.revision.saturating_add(1);
            self.last_body_json = Some(body_json);
        }
        let snapshot = HostSnapshot {
            schema: body.schema,
            core_version: body.core_version,
            storage_schema: body.storage_schema,
            revision: self.revision,
            as_of_unix_ms: body.as_of_unix_ms,
            timezone_id: body.timezone_id,
            app_version: body.app_version,
            app_build: body.app_build,
            connection: body.connection,
            session: body.session,
            analytics: body.analytics,
            historical: body.historical,
            recent_errors: body.recent_errors,
        };
        Ok(HostSnapshotResult {
            json: snapshot.canonical_json()?,
            hash: snapshot.canonical_hash()?,
            revision: snapshot.revision,
        })
    }

    fn connector(&self, connector_id: &str) -> Result<&RegisteredConnector> {
        self.connectors.get(connector_id).ok_or_else(|| {
            MavError::new(
                codes::FFI_CONNECTOR_NOT_FOUND,
                "connector is not registered",
            )
            .context(connector_id.to_owned())
        })
    }

    fn enqueue_all(&mut self, actions: Vec<TransportAction>) -> Result<()> {
        let needed = self.actions.len().saturating_add(actions.len());
        if needed > self.config.transport_capacity as usize {
            return Err(MavError::new(
                codes::FFI_ACTION_QUEUE_FULL,
                "transport action queue is full",
            )
            .context(format!(
                "queued {}, adding {}, capacity {}",
                self.actions.len(),
                actions.len(),
                self.config.transport_capacity
            )));
        }
        self.actions.extend(actions);
        Ok(())
    }

    fn require_state(&self, expected: ConnectionState, operation: &str) -> Result<()> {
        if self.state == expected {
            return Ok(());
        }
        Err(
            runtime_state("operation is invalid in the current connection state").context(format!(
                "operation {operation}, state {}, expected {}",
                self.state.name(),
                expected.name()
            )),
        )
    }

    fn require_native_device(&self, native_device_id: &str) -> Result<()> {
        let session = self.session_ref()?;
        if session.native_device_id.as_deref() == Some(native_device_id) {
            return Ok(());
        }
        Err(runtime_state(
            "native device id does not match the active session",
        ))
    }

    fn session_ref(&self) -> Result<&Session> {
        self.session
            .as_ref()
            .ok_or_else(|| runtime_state("no active session"))
    }

    fn session_mut(&mut self) -> Result<&mut Session> {
        self.session
            .as_mut()
            .ok_or_else(|| runtime_state("no active session"))
    }

    fn record_error(&self, error: &MavError, at_unix_ms: i64) -> Result<()> {
        self.store.record_error(error, millis_to_nanos(at_unix_ms)?)
    }

    fn persist_tap_errors(&self, tap: &ErrorCollectingTap, at_unix_ms: i64) -> Result<()> {
        let errors = tap.take();
        for error in errors {
            self.record_error(&error, at_unix_ms)?;
        }
        Ok(())
    }

    fn connection_snapshot(&self) -> Result<HostConnection> {
        let session = self.session.as_ref();
        // Device status is read from the stored event stream, not held in memory, so a snapshot
        // after a restart still surfaces the last battery and wrist reading the device sent. There
        // is no admitted charging decode yet, so `charging` stays honest at `None`.
        let (battery_percent, on_wrist) = match session {
            Some(active) => (
                self.latest_battery_percent(active.device)?,
                self.latest_wrist_state(active.device)?,
            ),
            None => (None, None),
        };
        Ok(HostConnection {
            state: self.state,
            device_id: session.map(|value| value.device.get()),
            connector_id: session.map(|value| value.connector_id.clone()),
            connector_version: session.map(|value| value.connector_version.clone()),
            display_name: session.and_then(|value| value.display_name.clone()),
            battery_percent,
            charging: None,
            on_wrist,
            last_sample_unix_ms: session.and_then(|value| value.last_sample_unix_ms),
        })
    }

    /// The most recent battery reading as a whole percent, clamped to `0..=100`. A `BatterySoc`
    /// sample carries a converted percent; a non-converted value is treated as absent rather than
    /// coerced.
    fn latest_battery_percent(&self, device: DeviceId) -> Result<Option<u8>> {
        let Some(sample) = self.store.latest_sample(device, StreamKind::BatterySoc)? else {
            return Ok(None);
        };
        let RawValue::Converted(percent) = sample.value else {
            return Ok(None);
        };
        Ok(Some(percent.round().clamp(0.0, 100.0) as u8))
    }

    /// The most recent wrist state: `true` on-wrist, `false` off-wrist, `None` if never reported.
    fn latest_wrist_state(&self, device: DeviceId) -> Result<Option<bool>> {
        let Some(sample) = self.store.latest_sample(device, StreamKind::WristState)? else {
            return Ok(None);
        };
        match sample.value {
            RawValue::U8(state) => Ok(Some(state != 0)),
            _ => Ok(None),
        }
    }
}

#[derive(Default)]
struct ErrorCollectingTap {
    errors: Mutex<Vec<MavError>>,
}

impl ErrorCollectingTap {
    fn take(&self) -> Vec<MavError> {
        match self.errors.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }
}

impl Tap for ErrorCollectingTap {
    fn on_stage(&self, _stage: Stage, event: TapEvent) {
        if let TapEvent::Rejected { error, .. } = event {
            match self.errors.lock() {
                Ok(mut guard) => guard.push(error),
                Err(poisoned) => poisoned.into_inner().push(error),
            }
        }
    }
}

fn validate_config(config: &RuntimeConfig) -> Result<()> {
    if config.database_path.trim().is_empty() {
        return Err(runtime_state("database path must not be empty"));
    }
    if config.timezone_id.trim().is_empty() {
        return Err(runtime_state("timezone id must not be empty"));
    }
    if config.transport_capacity == 0 {
        return Err(runtime_state("transport capacity must be positive"));
    }
    Ok(())
}

fn runtime_state(message: &str) -> MavError {
    MavError::new(codes::FFI_RUNTIME_STATE, message.to_owned())
}

fn wall_time(at_unix_ms: i64) -> Result<WallTime> {
    Ok(WallTime::from_nanos(millis_to_nanos(at_unix_ms)?))
}

fn millis_to_nanos(at_unix_ms: i64) -> Result<i64> {
    at_unix_ms.checked_mul(1_000_000).ok_or_else(|| {
        runtime_state("unix millisecond timestamp overflows nanoseconds")
            .context(at_unix_ms.to_string())
    })
}

fn host_error(entry: JournalEntry) -> HostError {
    HostError {
        code: entry.code,
        category: entry.category,
        severity: entry.severity,
        message: entry.message,
        context: entry.context,
        next_action: next_action(entry.category).to_owned(),
        at_unix_ms: entry.created_ns / 1_000_000,
    }
}

fn next_action(category: Category) -> &'static str {
    match category {
        Category::Transport => "retry_connection",
        Category::Frame | Category::Decode | Category::Timeline => "inspect_diagnostics",
        Category::Storage => "check_local_storage",
        Category::Feature | Category::Analytic | Category::Ml => "inspect_diagnostics",
        Category::Ffi | Category::Connector | Category::Internal => "restart_and_report",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Capture;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_DB: AtomicU64 = AtomicU64::new(1);

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/replay")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
    }

    fn db_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "mav-runtime-{}-{}.sqlite",
            std::process::id(),
            NEXT_DB.fetch_add(1, Ordering::Relaxed)
        ))
    }

    fn runtime(path: &Path) -> HostRuntime {
        HostRuntime::open(RuntimeConfig {
            database_path: path.to_string_lossy().into_owned(),
            timezone_id: "Europe/London".to_owned(),
            transport_capacity: 16,
            app_version: "0.1.0".to_owned(),
            app_build: "test".to_owned(),
        })
        .unwrap()
    }

    fn install(runtime: &mut HostRuntime) {
        runtime
            .install_connector(ConnectorRegistration {
                connector_id: "fixture".to_owned(),
                connector_version: "1.0.0".to_owned(),
                manifest_json: fixture("realtime_hr_v2.manifest.json"),
            })
            .unwrap();
    }

    fn reach_streaming(runtime: &mut HostRuntime) {
        runtime.start_scan("fixture", 1).unwrap();
        assert!(matches!(
            runtime.drain_actions(8).as_slice(),
            [TransportAction::StartScan { .. }]
        ));
        runtime
            .device_discovered("fixture", "native-1".to_owned(), Some("MG".to_owned()))
            .unwrap();
        assert_eq!(
            runtime.drain_actions(8),
            vec![
                TransportAction::StopScan,
                TransportAction::Connect {
                    native_device_id: "native-1".to_owned()
                }
            ]
        );
        runtime.connected("native-1").unwrap();
        let actions = runtime.drain_actions(8);
        assert_eq!(
            actions,
            vec![TransportAction::Subscribe {
                characteristic: "n".to_owned()
            }]
        );
        runtime.subscribed("n").unwrap();
        assert_eq!(runtime.connection_state(), ConnectionState::Streaming);
    }

    #[test]
    fn incremental_runtime_reproduces_the_frozen_session() {
        let path = db_path();
        let mut runtime = runtime(&path);
        install(&mut runtime);
        reach_streaming(&mut runtime);
        let capture = Capture::from_json(&fixture("realtime_hr_v2.capture.json")).unwrap();
        for chunk in capture.chunks {
            runtime
                .notification("n", &chunk, 1_752_600_500_000)
                .unwrap();
        }
        let result = runtime.host_snapshot(1_752_600_500_000).unwrap();
        let snapshot: HostSnapshot = serde_json::from_str(&result.json).unwrap();
        let session = snapshot.session.unwrap();
        assert_eq!(session.canonical_hash().unwrap(), "33143ef069a85a38");
        assert_eq!(session.current_bpm, Some(63));
        assert_eq!(snapshot.connection.display_name.as_deref(), Some("MG"));
        assert_eq!(
            snapshot.connection.last_sample_unix_ms,
            Some(1_752_600_500_000)
        );
        let _ = std::fs::remove_file(path);
    }

    /// Pins the exact canonical `host-snapshot/v1` bytes the platform decoders consume. The Swift
    /// and Kotlin decode tests read the same fixture file, so a change here is a change on both
    /// platforms through one seam. Regenerate with MAV_BLESS=1 (never edit by hand), then re-run
    /// plain to confirm, and eyeball the values against fixtures/replay/realtime_rr_prv_v2.
    #[test]
    fn host_snapshot_reproduces_the_platform_fixture() {
        let path = db_path();
        let mut runtime = HostRuntime::open(RuntimeConfig {
            database_path: path.to_string_lossy().into_owned(),
            timezone_id: "Europe/London".to_owned(),
            transport_capacity: 16,
            app_version: "0.1.0".to_owned(),
            app_build: "fixture".to_owned(),
        })
        .unwrap();
        runtime
            .install_connector(ConnectorRegistration {
                connector_id: "fixture".to_owned(),
                connector_version: "1.0.0".to_owned(),
                manifest_json: fixture("realtime_rr_prv_v2.manifest.json"),
            })
            .unwrap();
        reach_streaming(&mut runtime);
        let capture = Capture::from_json(&fixture("realtime_rr_prv_v2.capture.json")).unwrap();
        for chunk in capture.chunks {
            runtime
                .notification("n", &chunk, 1_752_600_500_000)
                .unwrap();
        }
        let result = runtime.host_snapshot(1_752_600_500_000).unwrap();

        let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/platform/host_snapshot_v1.expected.json");
        if std::env::var_os("MAV_BLESS").is_some() {
            let body = serde_json::json!({
                "schema": "host-snapshot-fixture/v1",
                "source_capture": "fixtures/replay/realtime_rr_prv_v2.capture.json",
                "generator": "MAV_BLESS=1 cargo test -p mav-engine host_snapshot_reproduces_the_platform_fixture",
                "algorithm_versions": {
                    "hr_feature": mav_feature::hr::HR_FEATURE_VERSION.to_string(),
                    "time_domain_interval_variability": mav_analytic::HRV_VERSION.to_string(),
                },
                "json": result.json,
                "hash": result.hash,
            });
            let mut text = serde_json::to_string_pretty(&body).unwrap();
            text.push('\n');
            std::fs::write(&fixture_path, text).unwrap();
            let _ = std::fs::remove_file(path);
            return;
        }
        let expected: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&fixture_path)
                .unwrap_or_else(|error| panic!("cannot read {}: {error}", fixture_path.display())),
        )
        .unwrap();
        assert_eq!(expected["json"].as_str().unwrap(), result.json);
        assert_eq!(expected["hash"].as_str().unwrap(), result.hash);
        assert_eq!(result.revision, 1);
        let _ = std::fs::remove_file(path);
    }

    /// The manifest for a device that streams battery and wrist events (WHOOP packet 48). Same
    /// gatt as the other fixtures so `reach_streaming` drives it unchanged.
    const EVENT_MANIFEST: &str = r#"{
        "schema": "connector-manifest/v1",
        "identity": { "family": "fixture-events", "display_name": "Fixture events", "models": ["FIXTURE"] },
        "gatt": { "service": "s", "command": "c", "notify": ["n"] },
        "frame": { "wire_format": "gen5", "max_frame_bytes": 8192 },
        "codec": "whoop",
        "packets": { "48": "event" },
        "event_vocabulary": "whoop",
        "capabilities": ["battery_soc", "wrist_state"]
    }"#;

    fn event_frame(number: u8, unix: u32, soc_deci: Option<u16>) -> Vec<u8> {
        // Inner event record: [0]=48, [2]=number, [4..8]=RTC unix, [13..15]=battery deci-percent.
        let mut payload = vec![0u8; 24];
        payload[0] = 48;
        payload[1] = 1;
        payload[2] = number;
        payload[4..8].copy_from_slice(&unix.to_le_bytes());
        if let Some(deci) = soc_deci {
            payload[13..15].copy_from_slice(&deci.to_le_bytes());
        }
        mav_frame::frame::build_frame(mav_frame::frame::WireFormat::Gen5, &payload).unwrap()
    }

    #[test]
    fn host_snapshot_surfaces_the_latest_battery_and_wrist_state() {
        let path = db_path();
        let mut runtime = runtime(&path);
        // The same registration the FFI performs: the edge supplies the device codec by id.
        runtime.register_codec("whoop", || Box::new(mav_connector_whoop::WhoopCodec::new()));
        runtime
            .install_connector(ConnectorRegistration {
                connector_id: "fixture".to_owned(),
                connector_version: "1.0.0".to_owned(),
                manifest_json: EVENT_MANIFEST.to_owned(),
            })
            .unwrap();
        reach_streaming(&mut runtime);

        // Before any event the device status is honestly unknown.
        let before: HostSnapshot =
            serde_json::from_str(&runtime.host_snapshot(1_752_600_500_000).unwrap().json).unwrap();
        assert_eq!(before.connection.battery_percent, None);
        assert_eq!(before.connection.on_wrist, None);

        // A stale 90% reading, then a newer 81.2% reading: the newer one must win.
        runtime
            .notification(
                "n",
                &event_frame(3, 1_752_600_000, Some(900)),
                1_752_600_400_000,
            )
            .unwrap();
        runtime
            .notification(
                "n",
                &event_frame(3, 1_752_600_100, Some(812)),
                1_752_600_450_000,
            )
            .unwrap();
        runtime
            .notification("n", &event_frame(9, 1_752_600_120, None), 1_752_600_460_000)
            .unwrap();

        let after: HostSnapshot =
            serde_json::from_str(&runtime.host_snapshot(1_752_600_500_000).unwrap().json).unwrap();
        assert_eq!(after.connection.battery_percent, Some(81));
        assert_eq!(after.connection.on_wrist, Some(true));
        // No admitted charging decode, so it stays None even when battery is known.
        assert_eq!(after.connection.charging, None);

        // A later wrist-off flips the state.
        runtime
            .notification(
                "n",
                &event_frame(10, 1_752_600_200, None),
                1_752_600_470_000,
            )
            .unwrap();
        let off: HostSnapshot =
            serde_json::from_str(&runtime.host_snapshot(1_752_600_500_000).unwrap().json).unwrap();
        assert_eq!(off.connection.on_wrist, Some(false));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn action_queue_overflow_changes_nothing() {
        let path = db_path();
        let mut runtime = HostRuntime::open(RuntimeConfig {
            database_path: path.to_string_lossy().into_owned(),
            timezone_id: "UTC".to_owned(),
            transport_capacity: 1,
            app_version: "0.1.0".to_owned(),
            app_build: "test".to_owned(),
        })
        .unwrap();
        install(&mut runtime);
        runtime.start_scan("fixture", 1).unwrap();
        let error = runtime
            .device_discovered("fixture", "native-1".to_owned(), None)
            .unwrap_err();
        assert_eq!(error.code, codes::FFI_ACTION_QUEUE_FULL);
        assert_eq!(runtime.connection_state(), ConnectionState::Scanning);
        assert!(matches!(
            runtime.drain_actions(8).as_slice(),
            [TransportAction::StartScan { .. }]
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn persistent_samples_survive_runtime_restart() {
        let path = db_path();
        {
            let mut first = runtime(&path);
            install(&mut first);
            reach_streaming(&mut first);
            let capture = Capture::from_json(&fixture("realtime_hr_v2.capture.json")).unwrap();
            for chunk in capture.chunks {
                first.notification("n", &chunk, 1_752_600_500_000).unwrap();
            }
        }
        let mut second = runtime(&path);
        install(&mut second);
        reach_streaming(&mut second);
        let result = second.host_snapshot(1_752_600_500_000).unwrap();
        let snapshot: HostSnapshot = serde_json::from_str(&result.json).unwrap();
        assert_eq!(
            snapshot.session.unwrap().canonical_hash().unwrap(),
            "33143ef069a85a38"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_fresh_runtime_reports_an_idle_historical_sync() {
        let path = db_path();
        let runtime = runtime(&path);
        let report = runtime.historical_report();
        assert_eq!(report.state, "historical_idle");
        assert_eq!(report.records_seen, 0);
        assert_eq!(report.records_inserted, 0);
        assert_eq!(report.last_cursor_hash, None);
        assert_eq!(report.failure_code, None);
        assert!(report.affected_days.is_empty());
        assert_eq!(
            report.canonical_hash().unwrap(),
            runtime.historical_report().canonical_hash().unwrap()
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn repeated_snapshot_query_is_byte_stable() {
        let path = db_path();
        let mut runtime = runtime(&path);
        let first = runtime.host_snapshot(1_752_600_500_000).unwrap();
        let second = runtime.host_snapshot(1_752_600_500_000).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.revision, 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn invalid_transition_returns_stable_error() {
        let path = db_path();
        let mut runtime = runtime(&path);
        let error = runtime.connected("native-1").unwrap_err();
        assert_eq!(error.code, codes::FFI_RUNTIME_STATE);
        assert_eq!(runtime.connection_state(), ConnectionState::Disconnected);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn rejected_frame_is_durable_and_visible_in_snapshot() {
        let path = db_path();
        let mut runtime = runtime(&path);
        install(&mut runtime);
        reach_streaming(&mut runtime);
        let capture = Capture::from_json(&fixture("realtime_hr_v2.capture.json")).unwrap();
        let mut corrupt = capture.chunks[0].clone();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 0xff;

        let stats = runtime
            .notification("n", &corrupt, 1_752_600_500_000)
            .unwrap();
        assert_eq!(stats, IngestStats::default());
        let result = runtime.host_snapshot(1_752_600_500_000).unwrap();
        let snapshot: HostSnapshot = serde_json::from_str(&result.json).unwrap();
        let mut codes_seen: Vec<_> = snapshot
            .recent_errors
            .iter()
            .map(|error| error.code)
            .collect();
        codes_seen.sort_unstable();
        assert_eq!(
            codes_seen,
            vec![
                codes::FRAME_PAYLOAD_CRC_MISMATCH,
                codes::FRAME_GARBAGE_SKIPPED
            ]
        );
        assert!(snapshot
            .recent_errors
            .iter()
            .all(|error| error.next_action == "inspect_diagnostics"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn connector_downgrade_is_refused() {
        let path = db_path();
        let mut runtime = runtime(&path);
        install(&mut runtime);
        let error = runtime
            .install_connector(ConnectorRegistration {
                connector_id: "fixture".to_owned(),
                connector_version: "0.9.0".to_owned(),
                manifest_json: fixture("realtime_hr_v2.manifest.json"),
            })
            .unwrap_err();
        assert_eq!(error.code, codes::FFI_CONNECTOR_DOWNGRADE);
        let _ = std::fs::remove_file(path);
    }
}
