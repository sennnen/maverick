//! Inspection and trust enforcement for hostile `.mavconn` artifacts.
#![forbid(unsafe_code)]

mod artifact;
mod engine;
mod fixtures;
mod instance;
mod limits;
mod memory;
mod registry;
mod trust;

pub use artifact::{signature_digest, Artifact, CanonicalUnsigned, InspectionReport};
pub use fixtures::FixtureResult;
pub use instance::ConnectorInstance;
pub use limits::LimitProfile;
pub use registry::{
    encode_signed_registry, ingest_registry, registry_rotation_digest, registry_signing_digest,
    restore_registry, RegistryAbiRange, RegistryCheckpoint, RegistryCoreRange, RegistryEntry,
    RegistryIndex, RegistryRevocation, RegistryRoot, RegistryRotation, RegistrySnapshot,
};
pub use trust::{KeyScope, KeyStatus, PublisherKey, Revocation, RevocationSet, TrustPolicy};
