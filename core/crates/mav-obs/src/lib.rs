//! Observability: the Tap trait invoked at every pipeline boundary, the always-on ring log, the
//! counters tap, and the tracing span helper. The sinks and the policy are described in
//! docs/errors.md; the pipeline boundaries this watches are described in docs/pipeline.md.
//!
//! The SQLite error journal and the report bundle are the durable siblings of the ring log; they
//! arrive with mav-store in M1 and read from the same entry shapes defined here.
#![forbid(unsafe_code)]

pub mod ring;
pub mod stage;
pub mod tap;
pub mod trace;

pub use ring::{RingEntry, RingEntryKind, RingLog, RingLogTap};
pub use stage::Stage;
pub use tap::{debug_summary, CountersTap, EventKind, Ids, Tap, TapEvent, Taps};
pub use trace::stage_span;
