use crate::{Artifact, KeyStatus, PublisherKey, Revocation, RevocationSet, TrustPolicy};
use ed25519_dalek::{Signature, VerifyingKey};
use mav_model::error::{codes, MavError, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::BTreeSet;

const SCHEMA: &str = "mavconn-registry-index/v1";
const ALGORITHM: &str = "Ed25519";
const SIGNATURE_DOMAIN: &[u8] = b"mavconn-registry-index-v1\0";
const ROTATION_DOMAIN: &[u8] = b"mavconn-publisher-rotation-v1\0";
const MAX_INDEX_BYTES: usize = 1024 * 1024;
const MAX_ENTRIES: usize = 4_096;
const MAX_REVOCATIONS: usize = 4_096;
const MAX_ROTATIONS: usize = 256;
const MAX_ARTIFACT_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryRoot {
    pub registry_id: String,
    pub key_id: String,
    pub public_key: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryAbiRange {
    pub major: u16,
    pub min_minor: u16,
    pub max_minor: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryCoreRange {
    pub min_version: String,
    pub max_version: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryEntry {
    pub connector_id: String,
    pub version: String,
    #[serde(with = "hex32")]
    pub artifact_sha256: [u8; 32],
    pub artifact_url: String,
    pub artifact_size: u64,
    pub publisher_key_id: String,
    pub abi: RegistryAbiRange,
    pub core: RegistryCoreRange,
    pub channel: String,
    pub supersedes: Option<String>,
    pub revoked: bool,
}

impl RegistryEntry {
    pub fn verify_artifact(&self, bytes: &[u8]) -> Result<()> {
        let size = u64::try_from(bytes.len()).map_err(|_| {
            error(
                codes::CONNECTOR_REGISTRY_ARTIFACT_MISMATCH,
                "downloaded artifact size exceeds the host integer range",
            )
        })?;
        let digest: [u8; 32] = Sha256::digest(bytes).into();
        if size != self.artifact_size || digest != self.artifact_sha256 {
            return Err(error(
                codes::CONNECTOR_REGISTRY_ARTIFACT_MISMATCH,
                format!(
                    "downloaded artifact for {} {} differs from signed registry metadata",
                    self.connector_id, self.version
                ),
            ));
        }
        let artifact = Artifact::inspect(bytes.to_vec()).map_err(|_| {
            error(
                codes::CONNECTOR_REGISTRY_ARTIFACT_MISMATCH,
                "downloaded registry artifact is not an inspectable connector",
            )
        })?;
        let manifest = &artifact.report().manifest;
        if manifest.connector_id.as_str() != self.connector_id
            || manifest.version != self.version
            || manifest.publisher_key_id != self.publisher_key_id
            || manifest.abi.major != self.abi.major
            || manifest.abi.min_minor != self.abi.min_minor
            || manifest.abi.max_minor != self.abi.max_minor
            || manifest.core.min_version != self.core.min_version
            || manifest.core.max_version != self.core.max_version
            || manifest.update.channel != self.channel
        {
            return Err(error(
                codes::CONNECTOR_REGISTRY_ARTIFACT_MISMATCH,
                "downloaded connector metadata differs from its signed registry entry",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryRevocation {
    pub publisher_key_id: String,
    pub revoked_at_ms: i64,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryRotation {
    pub from_key_id: String,
    pub to_key_id: String,
    #[serde(with = "hex32")]
    pub to_public_key: [u8; 32],
    pub effective_at_ms: i64,
    #[serde(with = "hex64")]
    pub cross_signature: [u8; 64],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryIndex {
    pub schema: String,
    pub registry_id: String,
    pub revision: u64,
    pub generated_at_ms: i64,
    pub valid_until_ms: i64,
    #[serde(with = "option_hex32")]
    pub previous_index_sha256: Option<[u8; 32]>,
    pub revocation_revision: u64,
    pub entries: Vec<RegistryEntry>,
    pub revocations: Vec<RegistryRevocation>,
    pub rotations: Vec<RegistryRotation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistrySignature {
    algorithm: String,
    key_id: String,
    #[serde(with = "hex64")]
    signature: [u8; 64],
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryEnvelope {
    index: RegistryIndex,
    signature: RegistrySignature,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryCheckpoint {
    pub registry_id: String,
    pub revision: u64,
    pub digest: [u8; 32],
    pub revocation_revision: u64,
    pub revocations: Vec<RegistryRevocation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistrySnapshot {
    pub index: RegistryIndex,
    pub digest: [u8; 32],
    pub trust: TrustPolicy,
    pub revocations: RevocationSet,
}

impl RegistrySnapshot {
    pub fn checkpoint(&self) -> RegistryCheckpoint {
        RegistryCheckpoint {
            registry_id: self.index.registry_id.clone(),
            revision: self.index.revision,
            digest: self.digest,
            revocation_revision: self.index.revocation_revision,
            revocations: self.index.revocations.clone(),
        }
    }

    pub fn select_update(
        &self,
        connector_id: &str,
        current_version: &str,
        channel: &str,
        allow_downgrade: bool,
    ) -> Result<Option<&RegistryEntry>> {
        let current = Version::parse(current_version)?;
        let mut selected: Option<(&RegistryEntry, Version)> = None;
        for entry in self.index.entries.iter().filter(|entry| {
            entry.connector_id == connector_id && entry.channel == channel && !entry.revoked
        }) {
            let candidate = Version::parse(&entry.version)?;
            let relative = candidate.cmp(&current);
            if relative == Ordering::Equal || (!allow_downgrade && relative == Ordering::Less) {
                continue;
            }
            if selected
                .as_ref()
                .is_none_or(|(_, chosen)| candidate.cmp(chosen) == Ordering::Greater)
            {
                selected = Some((entry, candidate));
            }
        }
        Ok(selected.map(|(entry, _)| entry))
    }
}

pub fn registry_signing_digest(index: &RegistryIndex) -> Result<[u8; 32]> {
    let canonical = serde_json::to_vec(index).map_err(|source| {
        error(
            codes::CONNECTOR_REGISTRY_MALFORMED,
            format!("registry index cannot be encoded: {source}"),
        )
    })?;
    Ok(Sha256::new()
        .chain_update(SIGNATURE_DOMAIN)
        .chain_update(canonical)
        .finalize()
        .into())
}

pub fn encode_signed_registry(
    index: RegistryIndex,
    key_id: String,
    signature: [u8; 64],
) -> Result<Vec<u8>> {
    serde_json::to_vec(&RegistryEnvelope {
        index,
        signature: RegistrySignature {
            algorithm: ALGORITHM.to_owned(),
            key_id,
            signature,
        },
    })
    .map_err(|source| {
        error(
            codes::CONNECTOR_REGISTRY_MALFORMED,
            format!("registry envelope cannot be encoded: {source}"),
        )
    })
}

pub fn registry_rotation_digest(rotation: &RegistryRotation) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(ROTATION_DOMAIN);
    update_field(&mut digest, rotation.from_key_id.as_bytes());
    update_field(&mut digest, rotation.to_key_id.as_bytes());
    update_field(&mut digest, &rotation.to_public_key);
    digest.update(rotation.effective_at_ms.to_be_bytes());
    digest.finalize().into()
}

pub fn ingest_registry(
    bytes: &[u8],
    root: &RegistryRoot,
    previous: Option<&RegistryCheckpoint>,
    policy: &TrustPolicy,
    now_ms: i64,
) -> Result<RegistrySnapshot> {
    let envelope = verify_envelope(bytes, root, now_ms)?;
    validate_chain(&envelope.index, previous)?;
    snapshot(envelope.index, bytes, policy)
}

pub fn restore_registry(
    bytes: &[u8],
    root: &RegistryRoot,
    expected: &RegistryCheckpoint,
    policy: &TrustPolicy,
    now_ms: i64,
) -> Result<RegistrySnapshot> {
    let envelope = verify_envelope(bytes, root, now_ms)?;
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    if envelope.index.registry_id != expected.registry_id
        || envelope.index.revision != expected.revision
        || envelope.index.revocation_revision != expected.revocation_revision
        || envelope.index.revocations != expected.revocations
        || digest != expected.digest
    {
        return Err(error(
            codes::CONNECTOR_REGISTRY_CHAIN_INVALID,
            "cached registry bytes do not match the persisted verified checkpoint",
        ));
    }
    snapshot(envelope.index, bytes, policy)
}

fn verify_envelope(bytes: &[u8], root: &RegistryRoot, now_ms: i64) -> Result<RegistryEnvelope> {
    if bytes.len() > MAX_INDEX_BYTES {
        return Err(error(
            codes::CONNECTOR_REGISTRY_OVERSIZED,
            format!("registry envelope exceeds {MAX_INDEX_BYTES} bytes"),
        ));
    }
    let envelope: RegistryEnvelope = serde_json::from_slice(bytes).map_err(|source| {
        error(
            codes::CONNECTOR_REGISTRY_MALFORMED,
            format!("registry envelope is invalid JSON: {source}"),
        )
    })?;
    let canonical = serde_json::to_vec(&envelope).map_err(|source| {
        error(
            codes::CONNECTOR_REGISTRY_MALFORMED,
            format!("registry envelope cannot be canonicalized: {source}"),
        )
    })?;
    if canonical != bytes {
        return Err(error(
            codes::CONNECTOR_REGISTRY_MALFORMED,
            "registry envelope is not deterministic compact JSON",
        ));
    }
    validate_index(&envelope.index, root, now_ms)?;
    if envelope.signature.algorithm != ALGORITHM || envelope.signature.key_id != root.key_id {
        return Err(error(
            codes::CONNECTOR_REGISTRY_SIGNATURE_INVALID,
            "registry signature algorithm or key id does not match configured root",
        ));
    }
    let verifying_key = VerifyingKey::from_bytes(&root.public_key).map_err(|_| {
        error(
            codes::CONNECTOR_REGISTRY_SIGNATURE_INVALID,
            "configured registry root is not a valid Ed25519 key",
        )
    })?;
    verifying_key
        .verify_strict(
            &registry_signing_digest(&envelope.index)?,
            &Signature::from_bytes(&envelope.signature.signature),
        )
        .map_err(|_| {
            error(
                codes::CONNECTOR_REGISTRY_SIGNATURE_INVALID,
                "registry index signature verification failed",
            )
        })?;

    Ok(envelope)
}

fn snapshot(index: RegistryIndex, bytes: &[u8], policy: &TrustPolicy) -> Result<RegistrySnapshot> {
    let trust = apply_rotations(policy, &index.rotations)?;
    let revocations = RevocationSet {
        revision: index.revocation_revision,
        generated_at_ms: index.generated_at_ms,
        valid_until_ms: index.valid_until_ms,
        entries: index
            .revocations
            .iter()
            .map(|entry| Revocation {
                publisher_key_id: entry.publisher_key_id.clone(),
                revoked_at_ms: entry.revoked_at_ms,
                reason: entry.reason.clone(),
            })
            .collect(),
    };
    let digest = Sha256::digest(bytes).into();
    Ok(RegistrySnapshot {
        index,
        digest,
        trust,
        revocations,
    })
}

fn validate_index(index: &RegistryIndex, root: &RegistryRoot, now_ms: i64) -> Result<()> {
    if index.schema != SCHEMA || index.registry_id != root.registry_id {
        return Err(error(
            codes::CONNECTOR_REGISTRY_MALFORMED,
            "registry schema or identity does not match configured root",
        ));
    }
    logical(&index.registry_id, "registry id")?;
    logical(&root.key_id, "registry root key id")?;
    if index.valid_until_ms < index.generated_at_ms
        || now_ms < index.generated_at_ms
        || now_ms > index.valid_until_ms
    {
        return Err(error(
            codes::CONNECTOR_REGISTRY_STALE,
            format!(
                "registry revision {} is outside its freshness interval",
                index.revision
            ),
        ));
    }
    if index.entries.len() > MAX_ENTRIES
        || index.revocations.len() > MAX_REVOCATIONS
        || index.rotations.len() > MAX_ROTATIONS
    {
        return Err(error(
            codes::CONNECTOR_REGISTRY_OVERSIZED,
            "registry collection exceeds its signed cardinality limit",
        ));
    }
    validate_entries(&index.entries)?;
    validate_revocations(&index.revocations)?;
    validate_rotations(&index.rotations)?;
    Ok(())
}

fn validate_chain(index: &RegistryIndex, previous: Option<&RegistryCheckpoint>) -> Result<()> {
    match previous {
        Some(checkpoint) => {
            if checkpoint.registry_id != index.registry_id {
                return Err(error(
                    codes::CONNECTOR_REGISTRY_CHAIN_INVALID,
                    "registry identity changed across refresh",
                ));
            }
            if index.revision <= checkpoint.revision
                || index.revocation_revision < checkpoint.revocation_revision
                || (index.revocation_revision == checkpoint.revocation_revision
                    && index.revocations != checkpoint.revocations)
            {
                return Err(error(
                    codes::CONNECTOR_REGISTRY_ROLLBACK,
                    "registry or revocation revision did not advance monotonically",
                ));
            }
            if index.previous_index_sha256 != Some(checkpoint.digest) {
                return Err(error(
                    codes::CONNECTOR_REGISTRY_CHAIN_INVALID,
                    "registry predecessor digest does not match cached signed index",
                ));
            }
            for cached in &checkpoint.revocations {
                if !index.revocations.contains(cached) {
                    return Err(error(
                        codes::CONNECTOR_REGISTRY_ROLLBACK,
                        "refreshed registry removed a cached publisher revocation",
                    ));
                }
            }
        }
        None if index.previous_index_sha256.is_some() => {
            return Err(error(
                codes::CONNECTOR_REGISTRY_CHAIN_INVALID,
                "initial registry index unexpectedly names a predecessor",
            ));
        }
        None => {}
    }
    Ok(())
}

fn validate_entries(entries: &[RegistryEntry]) -> Result<()> {
    let mut identities = BTreeSet::new();
    for entry in entries {
        logical(&entry.connector_id, "connector id")?;
        logical(&entry.publisher_key_id, "publisher key id")?;
        logical(&entry.channel, "release channel")?;
        Version::parse(&entry.version)?;
        Version::parse(&entry.core.min_version)?;
        if let Some(maximum) = &entry.core.max_version {
            Version::parse(maximum)?;
        }
        if let Some(supersedes) = &entry.supersedes {
            Version::parse(supersedes)?;
        }
        if entry.abi.min_minor > entry.abi.max_minor
            || entry.artifact_size == 0
            || entry.artifact_size > MAX_ARTIFACT_BYTES
            || !entry.artifact_url.starts_with("https://")
            || entry.artifact_url.len() > 2_048
            || entry.artifact_url.chars().any(char::is_whitespace)
        {
            return Err(error(
                codes::CONNECTOR_REGISTRY_MALFORMED,
                format!(
                    "registry entry {} {} has invalid bounds",
                    entry.connector_id, entry.version
                ),
            ));
        }
        if !identities.insert((
            entry.connector_id.as_str(),
            entry.version.as_str(),
            entry.channel.as_str(),
        )) {
            return Err(error(
                codes::CONNECTOR_REGISTRY_MALFORMED,
                "registry contains a duplicate connector version and channel",
            ));
        }
    }
    Ok(())
}

fn validate_revocations(entries: &[RegistryRevocation]) -> Result<()> {
    let mut ids = BTreeSet::new();
    for entry in entries {
        logical(&entry.publisher_key_id, "revoked publisher key id")?;
        if entry.reason.is_empty()
            || entry.reason.len() > 256
            || !ids.insert(&entry.publisher_key_id)
        {
            return Err(error(
                codes::CONNECTOR_REGISTRY_MALFORMED,
                "registry revocation is duplicate or has an invalid reason",
            ));
        }
    }
    Ok(())
}

fn validate_rotations(rotations: &[RegistryRotation]) -> Result<()> {
    let mut sources = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for rotation in rotations {
        logical(&rotation.from_key_id, "rotation source key id")?;
        logical(&rotation.to_key_id, "rotation target key id")?;
        if rotation.from_key_id == rotation.to_key_id
            || !sources.insert(&rotation.from_key_id)
            || !targets.insert(&rotation.to_key_id)
        {
            return Err(error(
                codes::CONNECTOR_REGISTRY_ROTATION_INVALID,
                "publisher rotation source or target is ambiguous",
            ));
        }
    }
    Ok(())
}

fn apply_rotations(policy: &TrustPolicy, rotations: &[RegistryRotation]) -> Result<TrustPolicy> {
    let mut trust = policy.clone();
    for rotation in rotations {
        let source_index = trust
            .keys
            .iter()
            .position(|key| key.id == rotation.from_key_id)
            .ok_or_else(|| {
                error(
                    codes::CONNECTOR_REGISTRY_ROTATION_INVALID,
                    "publisher rotation source is not independently trusted",
                )
            })?;
        let source = &trust.keys[source_index];
        if let KeyStatus::Rotated {
            at_ms,
            replacement_id,
        } = &source.status
        {
            let target_matches = trust.keys.iter().any(|target| {
                target.id == rotation.to_key_id
                    && target.public_key == rotation.to_public_key
                    && target.scope == source.scope
                    && target.valid_from_ms == rotation.effective_at_ms
            });
            if *at_ms == rotation.effective_at_ms
                && replacement_id == &rotation.to_key_id
                && target_matches
            {
                continue;
            }
        }
        if !matches!(source.status, KeyStatus::Active)
            || trust.keys.iter().any(|key| key.id == rotation.to_key_id)
        {
            return Err(error(
                codes::CONNECTOR_REGISTRY_ROTATION_INVALID,
                "publisher rotation source or target conflicts with existing trust",
            ));
        }
        let verifying_key = VerifyingKey::from_bytes(&source.public_key).map_err(|_| {
            error(
                codes::CONNECTOR_REGISTRY_ROTATION_INVALID,
                "publisher rotation source key is invalid",
            )
        })?;
        verifying_key
            .verify_strict(
                &registry_rotation_digest(rotation),
                &Signature::from_bytes(&rotation.cross_signature),
            )
            .map_err(|_| {
                error(
                    codes::CONNECTOR_REGISTRY_ROTATION_INVALID,
                    "publisher rotation lacks a valid old-key cross-signature",
                )
            })?;
        let scope = source.scope;
        trust.keys[source_index].status = KeyStatus::Rotated {
            at_ms: rotation.effective_at_ms,
            replacement_id: rotation.to_key_id.clone(),
        };
        trust.keys.push(PublisherKey {
            id: rotation.to_key_id.clone(),
            public_key: rotation.to_public_key,
            scope,
            valid_from_ms: rotation.effective_at_ms,
            valid_until_ms: None,
            status: KeyStatus::Active,
        });
    }
    if !rotations.is_empty() {
        trust.revision = trust.revision.saturating_add(1);
    }
    Ok(trust)
}

fn logical(value: &str, field: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(error(
            codes::CONNECTOR_REGISTRY_MALFORMED,
            format!("registry {field} is invalid"),
        ));
    }
    Ok(())
}

fn update_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_be_bytes());
    digest.update(bytes);
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Option<String>,
}

impl Version {
    fn parse(value: &str) -> Result<Self> {
        let (numeric, prerelease) = value
            .split_once('-')
            .map_or((value, None), |(numeric, suffix)| (numeric, Some(suffix)));
        if value.contains('+') || prerelease.is_some_and(str::is_empty) {
            return Err(update_error(value));
        }
        let mut fields = numeric.split('.');
        let major = parse_version_field(fields.next(), value)?;
        let minor = parse_version_field(fields.next(), value)?;
        let patch = parse_version_field(fields.next(), value)?;
        if fields.next().is_some()
            || prerelease.is_some_and(|suffix| {
                suffix.len() > 128
                    || !suffix
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-'))
            })
        {
            return Err(update_error(value));
        }
        Ok(Self {
            major,
            minor,
            patch,
            prerelease: prerelease.map(str::to_owned),
        })
    }

    fn cmp(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
            .then_with(|| match (&self.prerelease, &other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => left.cmp(right),
            })
    }
}

fn parse_version_field(field: Option<&str>, full: &str) -> Result<u64> {
    let field = field.ok_or_else(|| update_error(full))?;
    if field.is_empty() || (field.len() > 1 && field.starts_with('0')) {
        return Err(update_error(full));
    }
    field.parse().map_err(|_| update_error(full))
}

fn update_error(version: &str) -> MavError {
    error(
        codes::CONNECTOR_REGISTRY_UPDATE_REJECTED,
        format!("registry version {version} is not supported semantic version syntax"),
    )
}

fn error(code: u16, message: impl Into<String>) -> MavError {
    MavError::new(code, message)
}

mod hex32 {
    use serde::{Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&super::encode_hex(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        super::decode_hex(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }

    use serde::Deserialize;
}

mod option_hex32 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &Option<[u8; 32]>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes
            .as_ref()
            .map(|value| super::encode_hex(value))
            .serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<[u8; 32]>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Option::<String>::deserialize(deserializer)?
            .map(|value| super::decode_hex(&value).map_err(serde::de::Error::custom))
            .transpose()
    }

    use serde::Serialize;
}

mod hex64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&super::encode_hex(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        super::decode_hex(&String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn decode_hex<const N: usize>(value: &str) -> std::result::Result<[u8; N], String> {
    if value.len() != N * 2 {
        return Err(format!(
            "hex value must contain exactly {} characters",
            N * 2
        ));
    }
    let mut bytes = [0_u8; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = u8::from_str_radix(&value[offset..offset + 2], 16)
            .map_err(|_| "hex value contains invalid characters".to_owned())?;
    }
    Ok(bytes)
}
