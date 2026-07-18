use crate::Artifact;
use ed25519_dalek::{Signature, VerifyingKey};
use mav_model::error::{codes, MavError, Result};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyScope {
    Official,
    ThirdParty,
    Development,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyStatus {
    Active,
    Revoked { at_ms: i64, reason: String },
    Rotated { at_ms: i64, replacement_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublisherKey {
    pub id: String,
    pub public_key: [u8; 32],
    pub scope: KeyScope,
    pub valid_from_ms: i64,
    pub valid_until_ms: Option<i64>,
    pub status: KeyStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustPolicy {
    pub revision: u64,
    pub allow_third_party: bool,
    pub allow_development: bool,
    pub keys: Vec<PublisherKey>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revocation {
    pub publisher_key_id: String,
    pub revoked_at_ms: i64,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevocationSet {
    pub revision: u64,
    pub generated_at_ms: i64,
    pub valid_until_ms: i64,
    pub entries: Vec<Revocation>,
}

impl Artifact {
    pub fn verify(
        &self,
        policy: &TrustPolicy,
        revocations: &RevocationSet,
        now_ms: i64,
    ) -> Result<()> {
        let key_id = &self.report().signature.publisher_key_id;
        let mut ids = BTreeSet::new();
        if policy.keys.iter().any(|key| !ids.insert(&key.id)) {
            return Err(error(
                codes::CONNECTOR_TRUST_POLICY_INVALID,
                format!(
                    "trust policy {} contains duplicate key ids",
                    policy.revision
                ),
            ));
        }
        let key = policy
            .keys
            .iter()
            .find(|key| &key.id == key_id)
            .ok_or_else(|| {
                error(
                    codes::CONNECTOR_TRUST_UNKNOWN_PUBLISHER,
                    format!(
                        "publisher key {key_id} is not in trust policy {}",
                        policy.revision
                    ),
                )
            })?;
        if key
            .valid_until_ms
            .is_some_and(|until| until < key.valid_from_ms)
        {
            return Err(error(
                codes::CONNECTOR_TRUST_POLICY_INVALID,
                format!("publisher key {key_id} has an inverted validity interval"),
            ));
        }
        if now_ms < key.valid_from_ms {
            return Err(error(
                codes::CONNECTOR_TRUST_KEY_NOT_YET_VALID,
                format!("publisher key {key_id} is not valid yet"),
            ));
        }
        if key.valid_until_ms.is_some_and(|until| now_ms > until) {
            return Err(error(
                codes::CONNECTOR_TRUST_KEY_EXPIRED,
                format!("publisher key {key_id} has expired"),
            ));
        }
        if revocations.valid_until_ms < revocations.generated_at_ms
            || now_ms < revocations.generated_at_ms
            || now_ms > revocations.valid_until_ms
        {
            return Err(error(
                codes::CONNECTOR_TRUST_REVOCATION_STALE,
                format!(
                    "revocation set {} is outside its validity interval",
                    revocations.revision
                ),
            ));
        }
        match &key.status {
            KeyStatus::Active => {}
            KeyStatus::Revoked { at_ms, reason } if now_ms >= *at_ms => {
                return Err(error(
                    codes::CONNECTOR_TRUST_KEY_REVOKED,
                    format!("publisher key {key_id} is revoked: {reason}"),
                ));
            }
            KeyStatus::Rotated {
                at_ms,
                replacement_id,
            } if now_ms >= *at_ms => {
                return Err(error(
                    codes::CONNECTOR_TRUST_KEY_ROTATED,
                    format!("publisher key {key_id} rotated to {replacement_id}"),
                ));
            }
            KeyStatus::Revoked { .. } | KeyStatus::Rotated { .. } => {}
        }
        if revocations
            .entries
            .iter()
            .any(|entry| entry.publisher_key_id == *key_id && now_ms >= entry.revoked_at_ms)
        {
            return Err(error(
                codes::CONNECTOR_TRUST_KEY_REVOKED,
                format!(
                    "publisher key {key_id} is in revocation set {}",
                    revocations.revision
                ),
            ));
        }
        let scope_allowed = match key.scope {
            KeyScope::Official => true,
            KeyScope::ThirdParty => policy.allow_third_party,
            KeyScope::Development => policy.allow_development,
        };
        if !scope_allowed {
            return Err(error(
                codes::CONNECTOR_TRUST_SCOPE_REJECTED,
                format!("publisher key {key_id} scope is not allowed"),
            ));
        }
        let verifying_key = VerifyingKey::from_bytes(&key.public_key).map_err(|source| {
            error(
                codes::CONNECTOR_TRUST_SIGNATURE_INVALID,
                format!("publisher key {key_id} is invalid: {source}"),
            )
        })?;
        let signature = Signature::from_bytes(&self.report().signature.signature);
        verifying_key
            .verify_strict(&self.report().signed_digest, &signature)
            .map_err(|_| {
                error(
                    codes::CONNECTOR_TRUST_SIGNATURE_INVALID,
                    format!("signature verification failed for publisher key {key_id}"),
                )
            })
    }
}

fn error(code: u16, message: impl Into<String>) -> MavError {
    MavError::new(code, message)
}
