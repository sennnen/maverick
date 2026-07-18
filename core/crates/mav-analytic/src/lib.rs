//! Time-domain interval variability, capability negotiation, and the ported physiological
//! algorithm library. Analytics declare their input streams and admission state as data; an
//! unavailable dependency stays unavailable rather than producing a plausible-looking number from
//! missing evidence. See docs/analytics.md and ADR-005.
//!
//! The `time_domain` calculation in `hrv` is the one admitted, snapshot-emitting analytic. The
//! WHOOP-P6 modules below (imported from tanarchytan/whoop-rs, `[WRS]`) are brand-neutral,
//! pure-function ports: plain values in, wellness estimates out, no wire types, no IO, absent
//! signal returns `None`. Each is pinned by the upstream's own property/recovered-value tests, so
//! it satisfies the ADR-009 admission bar (a genuinely-failable test) even without a real capture
//! — but none is wired into the live snapshot or the capability graph yet. That wiring is a
//! separate packet, gated on real fixtures per analytic; until then these are a reviewed library
//! the FFI and future features draw on, not a claim that any number has been validated on Maverick
//! hardware. `recovery` and `strain` additionally carry a compatibility-estimate label (their
//! denominators/weights are the vendor's, not physiological ground truth).
#![forbid(unsafe_code)]

pub mod calibration;
pub mod capability;
pub mod hr_anomaly;
pub mod hr_zones;
pub mod hrv;
pub mod imu_features;
pub mod ppg_hr;
pub mod readiness;
pub mod recovery;
pub mod respiratory_rate;
pub mod resting_hr;
pub mod spo2;
pub mod stats;
pub mod strain;
pub mod stress;
pub mod vo2max;

pub use capability::{negotiate, AnalyticAvailability, AnalyticId, UnavailableReason, ANALYTICS};
pub use hrv::{
    time_domain, IntervalSource, TimeDomainHrv, HRV_ALGORITHM, HRV_VERSION, MIN_INTERVALS,
};
pub use readiness::{HrvReadiness, HrvReadinessResult, ReadinessTier, SECS_PER_DAY};
pub use stats::{linear_fit, pearson, LinearFit};
