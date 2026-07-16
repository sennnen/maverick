//! Orchestration: the connection state machine, and (in later packets) the triggers, task graph,
//! and recompute cache that drive the synchronous pipeline. See docs/pipeline.md.
//!
//! The one piece here today is acquisition, the pipeline's entry stage. It is a pure state machine
//! fed by injected events, so the whole of it is testable without a radio; the thin native
//! transport shim that feeds it real bytes is the only part that waits for hardware.
#![forbid(unsafe_code)]

pub mod acquisition;
pub mod historical;
pub mod pipeline;
pub mod runtime;
pub mod snapshot;

pub use acquisition::{Acquisition, Command, Event, HandshakeConfig, State, StepOutcome};
pub use historical::{
    CommandTemplate, HistoricalConfig, HistoricalController, HistoricalEvent, HistoricalOutcome,
    HistoricalState, ResponseResult,
};
pub use pipeline::{
    run_realtime, run_realtime_json, run_realtime_output, run_realtime_output_json, Capture,
    IngestStats, PipelineOutput, RealtimeProcessor,
};
pub use runtime::{
    ConnectionState, ConnectorRegistration, HostConnection, HostError, HostRuntime, HostSnapshot,
    HostSnapshotResult, RuntimeConfig, TransportAction, HOST_SNAPSHOT_SCHEMA,
};
pub use snapshot::{AnalyticsSnapshot, Snapshot, ANALYTICS_SNAPSHOT_SCHEMA, SNAPSHOT_SCHEMA};

/// Re-exported so `mav-replay` and the FFI can name a device manifest without depending on
/// `mav-codec` directly; the engine is the one crate above the stages that assembles them.
pub use mav_codec::manifest::Manifest;
pub use mav_store::Store;
