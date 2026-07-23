//! Frozen domain types shared by every stage of the pipeline.
//!
//! Everything in this crate is an interface. Other crates depend on these types and agents build
//! against them in parallel, so a change here ripples everywhere at once. That is why a change to
//! this crate needs an ADR (docs/adr): the freeze is the thing that lets the swarm work without
//! stepping on each other.
//!
//! The pieces: stable identifiers (`ids`), the two clocks and the mapping between them (`time`),
//! the sample and quality vocabulary (`stream`), semantic versions for algorithms and schemas
//! (`version`), and the one error type the whole core returns (`error`).
#![forbid(unsafe_code)]

pub mod error;
pub mod ids;
pub mod raw;
pub mod stream;
pub mod time;
pub mod version;

pub use error::{Category, MavError, Result, Severity};
pub use ids::{DeviceId, FrameId, MetadataId, SessionId, StreamId};
pub use raw::{RawSample, RawSampleBatch, RawValue};
pub use stream::{Quality, RejectReason, Sample, StreamKind};
pub use time::{ClockMap, ClockSegment, DeviceTime, WallTime};
pub use version::Version;
