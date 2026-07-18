use mav_connector_runtime::InspectionReport;
use mav_model::error::{codes, MavError, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    Bundled,
    Imported,
    Remote,
}

impl SourceKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Bundled => "bundled",
            Self::Imported => "imported",
            Self::Remote => "remote",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConnectorSource {
    pub kind: SourceKind,
    pub display_name: String,
    pub locator_digest: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalToken {
    pub(crate) binding: [u8; 32],
    pub(crate) expires_at_ms: i64,
}

impl ApprovalToken {
    pub fn to_bytes(&self) -> [u8; 40] {
        let mut bytes = [0_u8; 40];
        bytes[..32].copy_from_slice(&self.binding);
        bytes[32..].copy_from_slice(&self.expires_at_ms.to_be_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let binding = bytes.get(..32).ok_or_else(invalid_approval)?;
        let expiry = bytes.get(32..40).ok_or_else(invalid_approval)?;
        if bytes.len() != 40 {
            return Err(invalid_approval());
        }
        Ok(Self {
            binding: binding.try_into().map_err(|_| invalid_approval())?,
            expires_at_ms: i64::from_be_bytes(expiry.try_into().map_err(|_| invalid_approval())?),
        })
    }

    pub const fn expires_at_ms(&self) -> i64 {
        self.expires_at_ms
    }
}

fn invalid_approval() -> MavError {
    MavError::new(
        codes::CONNECTOR_INSTALL_APPROVAL_INVALID,
        "connector approval token must be exactly 40 bytes",
    )
}

#[cfg(test)]
mod tests {
    use super::ApprovalToken;
    use mav_model::error::codes;

    #[test]
    fn approval_token_has_a_fixed_round_trip_encoding() {
        let token = ApprovalToken {
            binding: [7; 32],
            expires_at_ms: -42,
        };
        let bytes = token.to_bytes();
        assert_eq!(bytes.len(), 40);
        assert_eq!(ApprovalToken::from_bytes(&bytes).ok(), Some(token));
        let error = ApprovalToken::from_bytes(&bytes[..39]).expect_err("short token rejected");
        assert_eq!(error.code, codes::CONNECTOR_INSTALL_APPROVAL_INVALID);
    }
}

#[derive(Clone, Debug)]
pub struct InspectionApproval {
    pub report: InspectionReport,
    pub fixture_count: u32,
    pub source: ConnectorSource,
    pub approval: ApprovalToken,
}

#[derive(Clone, Debug)]
pub struct InstallRequest {
    pub bytes: Vec<u8>,
    pub source: ConnectorSource,
    pub approval: ApprovalToken,
    pub activate: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledConnector {
    pub connector_id: String,
    pub version: String,
    pub publisher_key_id: String,
    pub state_schema: u32,
    pub artifact_digest: [u8; 32],
    pub source: ConnectorSource,
    pub installed_at_ms: i64,
    pub policy_revision: u64,
    pub revocation_revision: u64,
    pub fixture_count: u32,
    pub active: bool,
    pub disabled_reason: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemovalMode {
    DeleteState,
    QuarantineState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StateNamespace {
    pub connector_id: String,
    pub publisher_key_id: String,
    pub device_id: String,
    pub state_schema: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredState {
    pub namespace: StateNamespace,
    pub bytes: Vec<u8>,
    pub digest: [u8; 32],
    pub updated_at_ms: i64,
}
