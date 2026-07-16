//! Orchestration: the connection state machine, and (in later packets) the triggers, task graph,
//! and recompute cache that drive the synchronous pipeline. See docs/pipeline.md.
//!
//! The one piece here today is acquisition, the pipeline's entry stage. It is a pure state machine
//! fed by injected events, so the whole of it is testable without a radio; the thin native
//! transport shim that feeds it real bytes is the only part that waits for hardware.
#![forbid(unsafe_code)]

pub mod acquisition;
pub mod pipeline;
pub mod snapshot;

pub use acquisition::{Acquisition, Command, Event, HandshakeConfig, State, StepOutcome};
pub use pipeline::{run_realtime, run_realtime_json, Capture};
pub use snapshot::{Snapshot, SNAPSHOT_SCHEMA};

/// Re-exported so `mav-replay` and the FFI can name a device manifest without depending on
/// `mav-codec` directly; the engine is the one crate above the stages that assembles them.
pub use mav_codec::manifest::Manifest;
pub use mav_store::Store;
