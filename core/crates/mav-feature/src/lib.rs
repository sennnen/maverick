//! The feature engine: primitive, derived, and aggregate features, each a pure computation over
//! sample slices. The stage is deliberately pure. It does not read storage; the engine reads the
//! stored samples and hands them in, which is what keeps the stage crates from depending on each
//! other (see docs/architecture.md). Every feature carries a `MetadataId` so a value on screen can
//! be walked back to what produced it, and declares an algorithm version so a recompute cache and
//! a provenance row can tell one version's output from another's.
#![forbid(unsafe_code)]

pub mod hr;

pub use hr::{HrSummary, HR_FEATURE_ALGORITHM, HR_FEATURE_VERSION};
