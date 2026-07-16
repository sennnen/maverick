//! Time-domain interval variability and capability negotiation. Analytics declare their input
//! streams and admission state as data; an unavailable dependency stays unavailable rather than
//! producing a plausible-looking number from missing evidence. See docs/analytics.md and ADR-005.
#![forbid(unsafe_code)]

pub mod capability;
pub mod hrv;

pub use capability::{negotiate, AnalyticAvailability, AnalyticId, UnavailableReason, ANALYTICS};
pub use hrv::{
    time_domain, IntervalSource, TimeDomainHrv, HRV_ALGORITHM, HRV_VERSION, MIN_INTERVALS,
};
