//! Deterministic replay of the fixture suite embedded in one signed `.mavconn` artifact.
#![forbid(unsafe_code)]

use mav_connector_runtime::{
    Artifact, FixtureResult, KeyScope, KeyStatus, LimitProfile, PublisherKey, RevocationSet,
    TrustPolicy,
};
use mav_model::error::{codes, MavError, Result};
use std::path::Path;

pub struct Replay {
    pub connector_id: String,
    pub connector_version: String,
    pub fixtures: Vec<FixtureResult>,
}

pub fn replay_file(path: &Path, public_key: [u8; 32]) -> Result<Replay> {
    let bytes = std::fs::read(path).map_err(|source| {
        MavError::new(codes::STORAGE_OPEN, "could not read connector artifact")
            .context(path.display().to_string())
            .context(source.to_string())
    })?;
    replay_bytes(bytes, public_key)
}

pub fn replay_bytes(bytes: Vec<u8>, public_key: [u8; 32]) -> Result<Replay> {
    let artifact = Artifact::inspect(bytes)?;
    let key_id = artifact.report().signature.publisher_key_id.clone();
    artifact.verify(
        &TrustPolicy {
            revision: 1,
            allow_third_party: true,
            allow_development: true,
            keys: vec![PublisherKey {
                id: key_id,
                public_key,
                scope: KeyScope::Development,
                valid_from_ms: 0,
                valid_until_ms: None,
                status: KeyStatus::Active,
            }],
        },
        &RevocationSet {
            revision: 1,
            generated_at_ms: 0,
            valid_until_ms: i64::MAX,
            entries: Vec::new(),
        },
        0,
    )?;
    let connector_id = artifact.report().manifest.connector_id.as_str().to_owned();
    let connector_version = artifact.report().manifest.version.clone();
    let fixtures = artifact.run_fixtures(LimitProfile::mobile_v1())?;
    Ok(Replay {
        connector_id,
        connector_version,
        fixtures,
    })
}

pub fn decode_public_key(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 {
        return Err(MavError::new(
            codes::CONNECTOR_TRUST_POLICY_INVALID,
            "publisher public key must be 64 hexadecimal characters",
        ));
    }
    let mut key = [0_u8; 32];
    for (index, byte) in key.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|source| {
            MavError::new(
                codes::CONNECTOR_TRUST_POLICY_INVALID,
                "publisher public key contains invalid hexadecimal",
            )
            .context(source.to_string())
        })?;
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn both_packaged_connectors_replay_through_wasm_only() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/connectors");
        let mut artifacts: Vec<_> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                (path.extension()? == "mavconn").then_some(path)
            })
            .collect();
        artifacts.sort();
        assert!(!artifacts.is_empty(), "no packaged artifacts to replay");
        // The replay engine names no device: it exercises whatever the fixture directory carries.
        let key = "dfef1d92a685c9df623b8a321740b0a59de0de538bbfea9ddb703394a1e0f5bd";
        for artifact in artifacts {
            let replay = replay_file(&artifact, decode_public_key(key).unwrap()).unwrap();
            assert!(!replay.connector_id.is_empty());
            assert!(!replay.fixtures.is_empty());
            assert!(replay.fixtures.iter().all(|fixture| fixture.events_run > 0));
        }
    }

    #[test]
    fn malformed_public_key_is_typed() {
        let error = decode_public_key("nope").unwrap_err();
        assert_eq!(
            error.code,
            mav_model::error::codes::CONNECTOR_TRUST_POLICY_INVALID
        );
    }
}
