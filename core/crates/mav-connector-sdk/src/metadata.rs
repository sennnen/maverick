use crate::abi::{encode_canonical, AbiDescriptor, FixtureSet, Manifest};
use crate::ConnectorError;
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactMetadata {
    pub manifest: Manifest,
    pub abi: AbiDescriptor,
    pub fixtures: FixtureSet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedMetadata {
    pub manifest: Vec<u8>,
    pub abi: Vec<u8>,
    pub fixtures: Vec<u8>,
}

impl ArtifactMetadata {
    pub fn new(mut manifest: Manifest, abi: AbiDescriptor, fixtures: FixtureSet) -> Self {
        if let Ok(bytes) = encode_canonical(&fixtures) {
            manifest.fixture_set_hash = Sha256::digest(bytes).into();
        }
        Self {
            manifest,
            abi,
            fixtures,
        }
    }

    pub fn encode(&self) -> Result<EncodedMetadata, ConnectorError> {
        let fixtures = encode_canonical(&self.fixtures)?;
        let expected: [u8; 32] = Sha256::digest(&fixtures).into();
        if self.manifest.fixture_set_hash != expected {
            return Err(ConnectorError::InvalidWire(
                "manifest fixture-set hash differs from fixtures".to_owned(),
            ));
        }
        Ok(EncodedMetadata {
            manifest: encode_canonical(&self.manifest)?,
            abi: encode_canonical(&self.abi)?,
            fixtures,
        })
    }
}
