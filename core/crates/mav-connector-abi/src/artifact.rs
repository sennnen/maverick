use crate::{
    bounds, ActionBatch, ConnectorEvent, ConnectorId, LimitsProfileId, Validate, WireError,
};
use minicbor::{Decode, Encode};

pub const MANIFEST_SCHEMA: &str = "mavconn-manifest/v1";
pub const MANIFEST_SCHEMA_V2: &str = "mavconn-manifest/v2";
pub const ABI_SCHEMA: &str = "mavconn-abi/v1";
pub const FIXTURES_SCHEMA: &str = "mavconn-fixtures/v1";
pub const SIGNATURE_SCHEMA: &str = "mavconn-signature/v1";

// SHA-256 of schema/abi-v1.cddl. Tests freeze every constant against its source bytes.
pub const ABI_V1_SCHEMA_HASH: [u8; 32] = [
    0xb9, 0x01, 0xe5, 0xa7, 0x01, 0xe7, 0xaf, 0x57, 0x94, 0xb7, 0x4f, 0xf5, 0xbe, 0xb0, 0x55, 0x12,
    0xa1, 0xe6, 0xfa, 0x0e, 0x3e, 0x76, 0xcc, 0x7c, 0x97, 0xdc, 0x72, 0xf8, 0xb6, 0x6d, 0x2e, 0xa8,
];
pub const FIXTURES_V1_SCHEMA_HASH: [u8; 32] = [
    0x1d, 0xaa, 0xa3, 0xa4, 0xea, 0x07, 0xe1, 0xc1, 0x30, 0x46, 0x1c, 0x61, 0xfc, 0x9a, 0x0e, 0x0d,
    0x84, 0x33, 0xdb, 0x60, 0xac, 0x56, 0xf8, 0xb9, 0xbb, 0xc1, 0x07, 0x3b, 0xa9, 0xcb, 0xf1, 0xff,
];
pub const MANIFEST_V1_SCHEMA_HASH: [u8; 32] = [
    0x4e, 0xbe, 0xb1, 0x26, 0xd4, 0xc1, 0x7e, 0xea, 0xcc, 0xda, 0xb6, 0x93, 0x20, 0xcb, 0x6d, 0x08,
    0x5d, 0x3b, 0x06, 0x0a, 0x3d, 0x41, 0x3c, 0x1e, 0x3b, 0xc8, 0xc8, 0x36, 0x2e, 0xc7, 0x91, 0x2b,
];
pub const MANIFEST_V2_SCHEMA_HASH: [u8; 32] = [
    0xd0, 0x37, 0x91, 0x90, 0x69, 0xeb, 0x87, 0xec, 0x4c, 0xab, 0x2c, 0x31, 0xdc, 0x64, 0x50, 0xf8,
    0x14, 0xfa, 0x77, 0x0c, 0x00, 0x96, 0xe0, 0xb4, 0xb7, 0xa6, 0x59, 0xdb, 0x6a, 0x31, 0xf1, 0xcc,
];
pub const SIGNATURE_V1_SCHEMA_HASH: [u8; 32] = [
    0xbe, 0x85, 0x08, 0xdc, 0xc5, 0xfb, 0x10, 0x89, 0x82, 0x8d, 0xdb, 0x7b, 0xeb, 0x9f, 0xdc, 0xd5,
    0x30, 0x3d, 0xfa, 0xa8, 0xa9, 0x5b, 0xf5, 0xc4, 0xc5, 0x2f, 0x21, 0xcd, 0x57, 0x51, 0x58, 0x7e,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct AbiVersion {
    #[n(0)]
    pub major: u16,
    #[n(1)]
    pub minor: u16,
}

impl Validate for AbiVersion {
    fn validate(&self) -> Result<(), WireError> {
        if self.major != 1 {
            return Err(WireError::Schema("ABI major"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct AbiRange {
    #[n(0)]
    pub major: u16,
    #[n(1)]
    pub min_minor: u16,
    #[n(2)]
    pub max_minor: u16,
}

impl Validate for AbiRange {
    fn validate(&self) -> Result<(), WireError> {
        if self.major != 1 || self.min_minor > self.max_minor {
            return Err(WireError::Schema("ABI range"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct CoreRange {
    #[n(0)]
    pub min_version: String,
    #[n(1)]
    pub max_version: Option<String>,
}

impl Validate for CoreRange {
    fn validate(&self) -> Result<(), WireError> {
        version(&self.min_version, "minimum core version")?;
        if let Some(value) = &self.max_version {
            version(value, "maximum core version")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(index_only)]
pub enum Permission {
    #[n(0)]
    Ble,
}

impl Validate for Permission {
    fn validate(&self) -> Result<(), WireError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(index_only)]
pub enum CharacteristicProperty {
    #[n(0)]
    Read,
    #[n(1)]
    Write,
    #[n(2)]
    WriteWithoutResponse,
    #[n(3)]
    Notify,
    #[n(4)]
    Indicate,
}

impl Validate for CharacteristicProperty {
    fn validate(&self) -> Result<(), WireError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(index_only)]
pub enum TransportCapability {
    #[n(0)]
    Scan,
    #[n(1)]
    Connect,
    #[n(2)]
    Pair,
    #[n(3)]
    Discover,
    #[n(4)]
    Subscribe,
    #[n(5)]
    Read,
    #[n(6)]
    Write,
}

impl Validate for TransportCapability {
    fn validate(&self) -> Result<(), WireError> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(index_only)]
pub enum DowngradePolicy {
    #[n(0)]
    Reject,
    #[n(1)]
    ExplicitDeveloperApproval,
}

impl Validate for DowngradePolicy {
    fn validate(&self) -> Result<(), WireError> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct UpdatePolicy {
    #[n(0)]
    pub channel: String,
    #[n(1)]
    pub downgrade: DowngradePolicy,
}

impl Validate for UpdatePolicy {
    fn validate(&self) -> Result<(), WireError> {
        bounds::text(
            &self.channel,
            bounds::MAX_LOGICAL_ID_BYTES,
            "update channel",
        )?;
        self.downgrade.validate()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct DeviceFamily {
    #[n(0)]
    pub id: String,
    #[n(1)]
    pub name_prefixes: Vec<String>,
    #[n(2)]
    pub service_uuids: Vec<String>,
    #[n(3)]
    pub manufacturer_id: Option<u16>,
    #[n(4)]
    #[cbor(with = "minicbor::bytes")]
    pub manufacturer_mask: Vec<u8>,
    #[n(5)]
    #[cbor(with = "minicbor::bytes")]
    pub manufacturer_value: Vec<u8>,
}

impl Validate for DeviceFamily {
    fn validate(&self) -> Result<(), WireError> {
        logical(&self.id, "device family id")?;
        bounds::count(self.name_prefixes.len(), 16, "device name prefixes")?;
        for prefix in &self.name_prefixes {
            bounds::text(prefix, bounds::MAX_LABEL_BYTES, "device name prefix")?;
        }
        uuid_list(&self.service_uuids, "device service UUIDs")?;
        bounds::bytes(&self.manufacturer_mask, 64, "manufacturer mask")?;
        bounds::bytes(&self.manufacturer_value, 64, "manufacturer value")?;
        if self.manufacturer_mask.len() != self.manufacturer_value.len() {
            return Err(WireError::Schema("manufacturer mask length"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct CharacteristicDecl {
    #[n(0)]
    pub id: String,
    #[n(1)]
    pub uuid: String,
    #[n(2)]
    pub properties: Vec<CharacteristicProperty>,
    #[n(3)]
    pub sensitive: bool,
    #[n(4)]
    pub confirmed_write_required: bool,
}

impl Validate for CharacteristicDecl {
    fn validate(&self) -> Result<(), WireError> {
        logical(&self.id, "characteristic id")?;
        bounds::text(&self.uuid, bounds::MAX_UUID_BYTES, "characteristic UUID")?;
        bounds::count(self.properties.len(), 5, "characteristic properties")?;
        bounds::all(&self.properties)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct ServiceDecl {
    #[n(0)]
    pub id: String,
    #[n(1)]
    pub uuid: String,
    #[n(2)]
    pub characteristics: Vec<CharacteristicDecl>,
}

impl Validate for ServiceDecl {
    fn validate(&self) -> Result<(), WireError> {
        logical(&self.id, "service id")?;
        bounds::text(&self.uuid, bounds::MAX_UUID_BYTES, "service UUID")?;
        bounds::count(
            self.characteristics.len(),
            bounds::MAX_CHARACTERISTICS,
            "service characteristics",
        )?;
        bounds::all(&self.characteristics)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct CapabilityDecl {
    #[n(0)]
    pub stream: String,
    #[n(1)]
    pub transport: Vec<TransportCapability>,
}

impl Validate for CapabilityDecl {
    fn validate(&self) -> Result<(), WireError> {
        logical(&self.stream, "capability stream")?;
        bounds::count(self.transport.len(), 7, "transport capabilities")?;
        bounds::all(&self.transport)
    }
}

/// A user-initiated bounded recording the host may request from a connector.
///
/// This is maximum signed authority. The host exposes it only while `DeclareCapabilities` also
/// names the stream for the connected hardware session (ADR-033).
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct CaptureDecl {
    #[n(0)]
    pub stream: String,
    #[n(1)]
    pub unit: String,
    #[n(2)]
    pub minimum_sample_rate_hz: u16,
    #[n(3)]
    pub maximum_sample_rate_hz: u16,
}

impl Validate for CaptureDecl {
    fn validate(&self) -> Result<(), WireError> {
        logical(&self.stream, "capture stream")?;
        logical(&self.unit, "capture unit")?;
        if self.minimum_sample_rate_hz == 0
            || self.minimum_sample_rate_hz > self.maximum_sample_rate_hz
            || self.maximum_sample_rate_hz > 4_096
        {
            return Err(WireError::Schema("capture sample-rate range"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct Entrypoints {
    #[n(0)]
    pub abi_version: String,
    #[n(1)]
    pub alloc: String,
    #[n(2)]
    pub dealloc: String,
    #[n(3)]
    pub init: String,
    #[n(4)]
    pub handle: String,
    #[n(5)]
    pub snapshot: String,
}

impl Default for Entrypoints {
    fn default() -> Self {
        Self {
            abi_version: "mav_abi_version".to_owned(),
            alloc: "mav_alloc".to_owned(),
            dealloc: "mav_dealloc".to_owned(),
            init: "mav_init".to_owned(),
            handle: "mav_handle".to_owned(),
            snapshot: "mav_snapshot".to_owned(),
        }
    }
}

impl Validate for Entrypoints {
    fn validate(&self) -> Result<(), WireError> {
        if self != &Self::default() {
            return Err(WireError::Schema("ABI entrypoints"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct Manifest {
    #[n(0)]
    pub schema: String,
    #[n(1)]
    pub connector_id: ConnectorId,
    #[n(2)]
    pub version: String,
    #[n(3)]
    pub display_name: String,
    #[n(4)]
    pub description: String,
    #[n(5)]
    pub publisher_key_id: String,
    #[n(6)]
    pub abi: AbiRange,
    #[n(7)]
    pub core: CoreRange,
    #[n(8)]
    pub state_schema: u32,
    #[n(9)]
    pub artifact_limits_profile: LimitsProfileId,
    #[n(10)]
    pub device_families: Vec<DeviceFamily>,
    #[n(11)]
    pub services: Vec<ServiceDecl>,
    #[n(12)]
    pub capabilities: Vec<CapabilityDecl>,
    #[n(13)]
    pub permissions: Vec<Permission>,
    #[n(14)]
    pub entrypoints: Entrypoints,
    #[n(15)]
    #[cbor(with = "minicbor::bytes")]
    pub fixture_set_hash: [u8; 32],
    #[n(16)]
    pub update: UpdatePolicy,
    /// Absent in v1 manifests. A v2 manifest may declare a bounded set of user-initiated streams.
    #[n(17)]
    pub captures: Option<Vec<CaptureDecl>>,
}

impl Validate for Manifest {
    fn validate(&self) -> Result<(), WireError> {
        if self.schema != MANIFEST_SCHEMA && self.schema != MANIFEST_SCHEMA_V2 {
            return Err(WireError::Schema("manifest schema"));
        }
        self.connector_id.validate()?;
        version(&self.version, "connector version")?;
        bounds::text(&self.display_name, bounds::MAX_LABEL_BYTES, "display name")?;
        bounds::text(&self.description, bounds::MAX_TEXT_BYTES, "description")?;
        logical(&self.publisher_key_id, "publisher key id")?;
        self.abi.validate()?;
        self.core.validate()?;
        self.artifact_limits_profile.validate()?;
        bounds::count(
            self.device_families.len(),
            bounds::MAX_DEVICE_FAMILIES,
            "device families",
        )?;
        if self.device_families.is_empty() {
            return Err(WireError::Schema("device families"));
        }
        bounds::all(&self.device_families)?;
        bounds::count(self.services.len(), bounds::MAX_SERVICES, "services")?;
        if self.services.is_empty() {
            return Err(WireError::Schema("services"));
        }
        bounds::all(&self.services)?;
        bounds::count(
            self.capabilities.len(),
            bounds::MAX_CAPABILITIES,
            "capabilities",
        )?;
        if self.capabilities.is_empty() {
            return Err(WireError::Schema("capabilities"));
        }
        bounds::all(&self.capabilities)?;
        let captures = self.captures.as_deref().unwrap_or_default();
        if self.schema == MANIFEST_SCHEMA && !captures.is_empty() {
            return Err(WireError::Schema("manifest v1 capture declarations"));
        }
        bounds::count(captures.len(), bounds::MAX_CAPTURES, "capture declarations")?;
        bounds::all(captures)?;
        for capture in captures {
            if !self.capabilities.iter().any(|capability| {
                capability.stream == capture.stream
                    && capability.transport.contains(&TransportCapability::Write)
            }) {
                return Err(WireError::Schema("capture stream capability"));
            }
        }
        if self.permissions != [Permission::Ble] {
            return Err(WireError::Schema("manifest permissions"));
        }
        self.entrypoints.validate()?;
        self.update.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(index_only)]
pub enum WasmFeature {
    #[n(0)]
    MutableGlobals,
    #[n(1)]
    SignExtension,
    #[n(2)]
    BulkMemory,
}

impl Validate for WasmFeature {
    fn validate(&self) -> Result<(), WireError> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct AbiDescriptor {
    #[n(0)]
    pub schema: String,
    #[n(1)]
    pub version: AbiVersion,
    #[n(2)]
    #[cbor(with = "minicbor::bytes")]
    pub schema_hash: [u8; 32],
    #[n(3)]
    pub required_exports: Vec<String>,
    #[n(4)]
    pub required_imports: Vec<String>,
    #[n(5)]
    pub wasm_features: Vec<WasmFeature>,
    #[n(6)]
    pub sdk_version: String,
}

impl Validate for AbiDescriptor {
    fn validate(&self) -> Result<(), WireError> {
        schema(&self.schema, ABI_SCHEMA, "ABI schema")?;
        self.version.validate()?;
        if self.schema_hash != ABI_V1_SCHEMA_HASH {
            return Err(WireError::Schema("ABI schema hash"));
        }
        if self.required_exports
            != [
                "memory",
                "mav_abi_version",
                "mav_alloc",
                "mav_dealloc",
                "mav_init",
                "mav_handle",
                "mav_snapshot",
            ]
        {
            return Err(WireError::Schema("ABI exports"));
        }
        if !self.required_imports.is_empty() {
            return Err(WireError::Schema("ABI imports"));
        }
        bounds::all(&self.wasm_features)?;
        version(&self.sdk_version, "SDK version")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct FixtureCase {
    #[n(0)]
    pub name: String,
    #[n(1)]
    #[cbor(with = "minicbor::bytes")]
    pub initial_state: Vec<u8>,
    #[n(2)]
    pub events: Vec<ConnectorEvent>,
    #[n(3)]
    pub expected: Vec<ActionBatch>,
    #[n(4)]
    #[cbor(with = "minicbor::bytes")]
    pub expected_state_hash: [u8; 32],
    #[n(5)]
    pub max_fuel: u64,
    #[n(6)]
    pub expected_samples: Option<Vec<crate::WireSample>>,
    #[n(7)]
    pub expected_diagnostics: Option<Vec<ExpectedDiagnostic>>,
}

impl Validate for FixtureCase {
    fn validate(&self) -> Result<(), WireError> {
        bounds::text(&self.name, bounds::MAX_LABEL_BYTES, "fixture name")?;
        bounds::bytes(
            &self.initial_state,
            bounds::MAX_STATE_BYTES,
            "fixture initial state",
        )?;
        bounds::count(
            self.events.len(),
            bounds::MAX_FIXTURE_EVENTS,
            "fixture events",
        )?;
        bounds::all(&self.events)?;
        bounds::count(
            self.expected.len(),
            bounds::MAX_FIXTURE_ACTIONS,
            "fixture action batches",
        )?;
        bounds::all(&self.expected)?;
        if self.max_fuel == 0 {
            return Err(WireError::Bounds("fixture fuel"));
        }
        if let Some(samples) = &self.expected_samples {
            bounds::count(
                samples.len(),
                bounds::MAX_SAMPLES_PER_ACTION,
                "fixture expected samples",
            )?;
            bounds::all(samples)?;
        }
        if let Some(diagnostics) = &self.expected_diagnostics {
            bounds::count(diagnostics.len(), 128, "fixture expected diagnostics")?;
            bounds::all(diagnostics)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct ExpectedDiagnostic {
    #[n(0)]
    pub code: String,
    #[n(1)]
    pub message: String,
}

impl Validate for ExpectedDiagnostic {
    fn validate(&self) -> Result<(), WireError> {
        logical(&self.code, "fixture diagnostic code")?;
        bounds::text(
            &self.message,
            bounds::MAX_DIAGNOSTIC_BYTES,
            "fixture diagnostic message",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct FixtureSet {
    #[n(0)]
    pub schema: String,
    #[n(1)]
    pub cases: Vec<FixtureCase>,
}

impl Validate for FixtureSet {
    fn validate(&self) -> Result<(), WireError> {
        schema(&self.schema, FIXTURES_SCHEMA, "fixtures schema")?;
        bounds::count(self.cases.len(), bounds::MAX_FIXTURES, "fixture cases")?;
        if self.cases.is_empty() {
            return Err(WireError::Schema("fixture cases"));
        }
        bounds::all(&self.cases)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(index_only)]
pub enum SignatureAlgorithm {
    #[n(0)]
    Ed25519,
}

impl Validate for SignatureAlgorithm {
    fn validate(&self) -> Result<(), WireError> {
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode)]
#[cbor(map)]
pub struct SignatureRecord {
    #[n(0)]
    pub schema: String,
    #[n(1)]
    pub algorithm: SignatureAlgorithm,
    #[n(2)]
    pub publisher_key_id: String,
    #[n(3)]
    #[cbor(with = "minicbor::bytes")]
    pub digest: [u8; 32],
    #[n(4)]
    #[cbor(with = "minicbor::bytes")]
    pub signature: [u8; 64],
}

impl Validate for SignatureRecord {
    fn validate(&self) -> Result<(), WireError> {
        schema(&self.schema, SIGNATURE_SCHEMA, "signature schema")?;
        self.algorithm.validate()?;
        logical(&self.publisher_key_id, "signature publisher key id")
    }
}

fn schema(value: &str, expected: &str, field: &'static str) -> Result<(), WireError> {
    if value != expected {
        return Err(WireError::Schema(field));
    }
    Ok(())
}

fn logical(value: &str, field: &'static str) -> Result<(), WireError> {
    bounds::identifier(value, bounds::MAX_LOGICAL_ID_BYTES, field)
}

fn uuid_list(values: &[String], field: &'static str) -> Result<(), WireError> {
    bounds::count(values.len(), bounds::MAX_SCAN_FILTERS, field)?;
    for value in values {
        bounds::text(value, bounds::MAX_UUID_BYTES, field)?;
    }
    Ok(())
}

fn version(value: &str, field: &'static str) -> Result<(), WireError> {
    bounds::text(value, 64, field)?;
    if value.contains('+') {
        return Err(WireError::Schema(field));
    }
    let core = value.split_once('-').map_or(value, |(core, _)| core);
    let mut parts = core.split('.');
    let valid = (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    }) && parts.next().is_none();
    if !valid {
        return Err(WireError::Schema(field));
    }
    Ok(())
}
