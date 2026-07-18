//! Inspection and trust enforcement for hostile `.mavconn` artifacts.
#![forbid(unsafe_code)]

mod artifact;
mod engine;
mod fixtures;
mod instance;
mod limits;
mod memory;
mod trust;

pub use artifact::{signature_digest, Artifact, CanonicalUnsigned, InspectionReport};
pub use fixtures::FixtureResult;
pub use instance::ConnectorInstance;
pub use limits::LimitProfile;
pub use trust::{KeyScope, KeyStatus, PublisherKey, Revocation, RevocationSet, TrustPolicy};
