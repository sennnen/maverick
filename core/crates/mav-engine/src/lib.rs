//! Orchestration for the runtime-loaded connector event/action loop and recompute cache. Device
//! protocol, framing, retry, and historical state machines execute inside signed artifacts.
#![forbid(unsafe_code)]

pub mod connector_host;
pub mod recompute;
pub mod spine;

pub use connector_host::{
    ApplyOutcome, ConnectorHost, ConnectorHostConfig, ConnectorLifecycleSnapshot,
    ConnectorTransportAction, ConnectorTransportRequest,
};
pub use mav_store::Store;
pub use recompute::{AffectedDays, CacheKey, LocalDay, OffsetSpan, RecomputeCache, Timezone};
pub use spine::{AlgorithmStamp, DailySnapshot, Spine};
