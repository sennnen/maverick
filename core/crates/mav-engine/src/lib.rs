//! Orchestration: the connection state machine, and (in later packets) the triggers, task graph,
//! and recompute cache that drive the synchronous pipeline. See docs/pipeline.md.
//!
//! The one piece here today is acquisition, the pipeline's entry stage. It is a pure state machine
//! fed by injected events, so the whole of it is testable without a radio; the thin native
//! transport shim that feeds it real bytes is the only part that waits for hardware.
#![forbid(unsafe_code)]

pub mod acquisition;
pub mod burst;
pub mod connector_host;
pub mod historical;
pub mod pipeline;
pub mod recompute;
pub mod snapshot;

pub use acquisition::{Acquisition, Command, Event, HandshakeConfig, State, StepOutcome};
pub use connector_host::{
    ApplyOutcome, ConnectorHost, ConnectorHostConfig, ConnectorLifecycleSnapshot,
    ConnectorTransportAction, ConnectorTransportRequest,
};
pub use historical::{
    CommandTemplate, HistoricalConfig, HistoricalController, HistoricalEvent, HistoricalOutcome,
    HistoricalReport, HistoricalState, ResponseResult, SyncTotals, HISTORICAL_STATUS_SCHEMA,
};
pub use recompute::{
    AffectedDays, CacheKey, LocalDay, OffsetSpan, RecomputeCache, RecomputeTrigger, SyncDays,
    Timezone,
};

pub use pipeline::{
    run_realtime, run_realtime_json, run_realtime_output, run_realtime_output_json, Capture,
    IngestStats, PipelineOutput, RealtimeProcessor,
};
pub use snapshot::{AnalyticsSnapshot, Snapshot, ANALYTICS_SNAPSHOT_SCHEMA, SNAPSHOT_SCHEMA};

pub use mav_codec::manifest::Manifest;
pub use mav_store::Store;
