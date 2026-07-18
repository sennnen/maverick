//! Inspection and trust enforcement for hostile `.mavconn` artifacts.
#![forbid(unsafe_code)]

mod artifact;
mod trust;

pub use artifact::{signature_digest, Artifact, CanonicalUnsigned, InspectionReport};
pub use trust::{KeyScope, KeyStatus, PublisherKey, Revocation, RevocationSet, TrustPolicy};
