use mav_connector_runtime::InspectionReport;

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
