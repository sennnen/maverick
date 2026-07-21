//! Deterministic `.mavconn` developer tooling.

use ed25519_dalek::{Signature, VerifyingKey};
use mav_connector_abi::{
    decode_canonical, encode_canonical, AbiDescriptor, FixtureSet, Manifest, SignatureAlgorithm,
    SignatureRecord, Validate, SIGNATURE_SCHEMA,
};
use mav_connector_runtime::{
    signature_digest, Artifact, FixtureResult, KeyScope, KeyStatus, LimitProfile, PublisherKey,
    RevocationSet, TrustPolicy,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fmt;
use wasmparser::{Encoding, ExternalKind, FuncType, Parser, Payload, ValType, Validator};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolError {
    InvalidMetadata(&'static str),
    InvalidWasm(String),
    ExistingMavSection(String),
    UnexpectedImport,
    MissingExport(String),
    InvalidExport(String),
    InvalidPublicKey,
    InvalidSignature,
    Artifact(String),
}

impl fmt::Display for ToolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMetadata(section) => write!(formatter, "invalid {section} metadata"),
            Self::InvalidWasm(message) => write!(formatter, "invalid WebAssembly: {message}"),
            Self::ExistingMavSection(name) => {
                write!(formatter, "input module already contains {name}")
            }
            Self::UnexpectedImport => formatter.write_str("ABI v1 modules cannot import symbols"),
            Self::MissingExport(name) => write!(formatter, "required export {name} is missing"),
            Self::InvalidExport(name) => write!(formatter, "required export {name} has wrong type"),
            Self::InvalidPublicKey => formatter.write_str("invalid Ed25519 public key"),
            Self::InvalidSignature => formatter.write_str("invalid Ed25519 signature"),
            Self::Artifact(message) => write!(formatter, "artifact rejected: {message}"),
        }
    }
}

impl std::error::Error for ToolError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedArtifact {
    pub bytes: Vec<u8>,
    pub digest: [u8; 32],
    pub publisher_key_id: String,
}

pub fn prepare(
    module: &[u8],
    manifest: &Manifest,
    abi: &AbiDescriptor,
    fixtures: &FixtureSet,
) -> Result<PreparedArtifact, ToolError> {
    manifest
        .validate()
        .map_err(|_| ToolError::InvalidMetadata("manifest"))?;
    abi.validate()
        .map_err(|_| ToolError::InvalidMetadata("ABI"))?;
    fixtures
        .validate()
        .map_err(|_| ToolError::InvalidMetadata("fixtures"))?;
    let manifest_bytes =
        encode_canonical(manifest).map_err(|_| ToolError::InvalidMetadata("manifest"))?;
    let abi_bytes = encode_canonical(abi).map_err(|_| ToolError::InvalidMetadata("ABI"))?;
    let fixture_bytes =
        encode_canonical(fixtures).map_err(|_| ToolError::InvalidMetadata("fixtures"))?;
    let fixture_digest: [u8; 32] = Sha256::digest(&fixture_bytes).into();
    if fixture_digest != manifest.fixture_set_hash {
        return Err(ToolError::InvalidMetadata("manifest fixture hash"));
    }
    validate_module(module, abi, false)?;
    let mut bytes = module.to_vec();
    append_custom(&mut bytes, "mav:manifest", &manifest_bytes);
    append_custom(&mut bytes, "mav:abi", &abi_bytes);
    append_custom(&mut bytes, "mav:fixtures", &fixture_bytes);
    let digest = signature_digest([bytes.as_slice()]);
    Ok(PreparedArtifact {
        bytes,
        digest,
        publisher_key_id: manifest.publisher_key_id.clone(),
    })
}

pub fn prepare_encoded(
    module: &[u8],
    manifest: &[u8],
    abi: &[u8],
    fixtures: &[u8],
) -> Result<PreparedArtifact, ToolError> {
    let manifest =
        decode_canonical(manifest).map_err(|_| ToolError::InvalidMetadata("manifest"))?;
    let abi = decode_canonical(abi).map_err(|_| ToolError::InvalidMetadata("ABI"))?;
    let fixtures =
        decode_canonical(fixtures).map_err(|_| ToolError::InvalidMetadata("fixtures"))?;
    prepare(module, &manifest, &abi, &fixtures)
}

pub fn prepared_unsigned(
    bytes: Vec<u8>,
    publisher_key_id: String,
) -> Result<PreparedArtifact, ToolError> {
    let digest = signature_digest([bytes.as_slice()]);
    Ok(PreparedArtifact {
        bytes,
        digest,
        publisher_key_id,
    })
}

pub fn finalize(
    prepared: PreparedArtifact,
    signature: [u8; 64],
    public_key: [u8; 32],
) -> Result<Vec<u8>, ToolError> {
    let verifying_key =
        VerifyingKey::from_bytes(&public_key).map_err(|_| ToolError::InvalidPublicKey)?;
    verifying_key
        .verify_strict(&prepared.digest, &Signature::from_bytes(&signature))
        .map_err(|_| ToolError::InvalidSignature)?;
    let record = SignatureRecord {
        schema: SIGNATURE_SCHEMA.to_owned(),
        algorithm: SignatureAlgorithm::Ed25519,
        publisher_key_id: prepared.publisher_key_id,
        digest: prepared.digest,
        signature,
    };
    let record_bytes =
        encode_canonical(&record).map_err(|_| ToolError::InvalidMetadata("signature"))?;
    let mut bytes = prepared.bytes;
    append_custom(&mut bytes, "mav:signature", &record_bytes);
    validate(&bytes, public_key)?;
    Ok(bytes)
}

pub fn inspect(bytes: Vec<u8>) -> Result<Artifact, ToolError> {
    Artifact::inspect(bytes).map_err(|error| ToolError::Artifact(error.to_string()))
}

pub fn validate(bytes: &[u8], public_key: [u8; 32]) -> Result<(), ToolError> {
    let artifact = inspect(bytes.to_vec())?;
    validate_module(bytes, &artifact.report().abi, true)?;
    let key_id = artifact.report().signature.publisher_key_id.clone();
    let policy = TrustPolicy {
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
    };
    let revocations = RevocationSet {
        revision: 1,
        generated_at_ms: 0,
        valid_until_ms: i64::MAX,
        entries: Vec::new(),
    };
    artifact
        .verify(&policy, &revocations, 0)
        .map_err(|error| ToolError::Artifact(error.to_string()))
}

pub fn test_fixtures(
    bytes: Vec<u8>,
    public_key: [u8; 32],
) -> Result<Vec<FixtureResult>, ToolError> {
    validate(&bytes, public_key)?;
    inspect(bytes)?
        .run_fixtures(LimitProfile::mobile_v1())
        .map_err(|error| ToolError::Artifact(error.to_string()))
}

#[derive(Serialize)]
struct ParityReport {
    schema: &'static str,
    connector_id: String,
    connector_version: String,
    artifact_sha256: String,
    manifest_sha256: String,
    signed_sha256: String,
    fixture_count: usize,
    fixtures: Vec<ParityFixture>,
}

#[derive(Serialize)]
struct ParityFixture {
    name: String,
    events: u32,
    input_sha256: String,
    action_trace_sha256: String,
    sample_sha256: String,
    final_state_sha256: String,
    max_fuel_consumed: u64,
    peak_memory_bytes: u64,
}

pub fn parity_report(bytes: Vec<u8>, public_key: [u8; 32]) -> Result<String, ToolError> {
    validate(&bytes, public_key)?;
    let artifact = inspect(bytes)?;
    let report = artifact.report();
    let fixtures = artifact
        .run_fixtures(LimitProfile::mobile_v1())
        .map_err(|error| ToolError::Artifact(error.to_string()))?
        .into_iter()
        .map(|fixture| ParityFixture {
            name: fixture.name,
            events: fixture.events_run,
            input_sha256: encode_hex(&fixture.input_hash),
            action_trace_sha256: encode_hex(&fixture.action_trace_hash),
            sample_sha256: encode_hex(&fixture.sample_hash),
            final_state_sha256: encode_hex(&fixture.final_state_hash),
            max_fuel_consumed: fixture.max_fuel_consumed,
            peak_memory_bytes: fixture.peak_memory_bytes,
        })
        .collect::<Vec<_>>();
    let parity = ParityReport {
        schema: "mavconn-parity/v1",
        connector_id: report.manifest.connector_id.as_str().to_owned(),
        connector_version: report.manifest.version.clone(),
        artifact_sha256: encode_hex(&report.artifact_digest),
        manifest_sha256: encode_hex(&report.manifest_digest),
        signed_sha256: encode_hex(&report.signed_digest),
        fixture_count: fixtures.len(),
        fixtures,
    };
    serde_json::to_string_pretty(&parity)
        .map(|mut encoded| {
            encoded.push('\n');
            encoded
        })
        .map_err(|_| ToolError::InvalidMetadata("parity report"))
}

fn validate_module(
    module: &[u8],
    abi: &AbiDescriptor,
    allow_required_sections: bool,
) -> Result<(), ToolError> {
    Validator::new()
        .validate_all(module)
        .map_err(|error| ToolError::InvalidWasm(error.to_string()))?;
    let mut exports = BTreeSet::new();
    let mut types: Vec<FuncType> = Vec::new();
    let mut functions: Vec<u32> = Vec::new();
    let mut typed_exports = Vec::new();
    for payload in Parser::new(0).parse_all(module) {
        match payload.map_err(|error| ToolError::InvalidWasm(error.to_string()))? {
            Payload::Version {
                encoding: Encoding::Module,
                ..
            } => {}
            Payload::Version { .. } => {
                return Err(ToolError::InvalidWasm(
                    "component artifacts are unsupported".to_owned(),
                ));
            }
            Payload::ImportSection(reader) if reader.count() != 0 => {
                return Err(ToolError::UnexpectedImport);
            }
            Payload::TypeSection(reader) => {
                for function_type in reader.into_iter_err_on_gc_types() {
                    types.push(
                        function_type.map_err(|error| ToolError::InvalidWasm(error.to_string()))?,
                    );
                }
            }
            Payload::FunctionSection(reader) => {
                for type_index in reader {
                    functions.push(
                        type_index.map_err(|error| ToolError::InvalidWasm(error.to_string()))?,
                    );
                }
            }
            Payload::ExportSection(reader) => {
                for export in reader {
                    let export =
                        export.map_err(|error| ToolError::InvalidWasm(error.to_string()))?;
                    exports.insert(export.name.to_owned());
                    typed_exports.push((export.name.to_owned(), export.kind, export.index));
                }
            }
            Payload::CustomSection(reader)
                if reader.name().starts_with("mav:") && !allow_required_sections =>
            {
                return Err(ToolError::ExistingMavSection(reader.name().to_owned()));
            }
            _ => {}
        }
    }
    for required in &abi.required_exports {
        if !exports.contains(required) {
            return Err(ToolError::MissingExport(required.clone()));
        }
    }
    validate_export_types(&typed_exports, &functions, &types)?;
    Ok(())
}

fn validate_export_types(
    exports: &[(String, ExternalKind, u32)],
    functions: &[u32],
    types: &[FuncType],
) -> Result<(), ToolError> {
    for (name, expected_params, expected_results) in [
        ("mav_abi_version", &[][..], &[ValType::I64][..]),
        ("mav_alloc", &[ValType::I32][..], &[ValType::I32][..]),
        ("mav_dealloc", &[ValType::I32, ValType::I32][..], &[][..]),
        (
            "mav_init",
            &[ValType::I32, ValType::I32][..],
            &[ValType::I64][..],
        ),
        (
            "mav_handle",
            &[ValType::I32, ValType::I32][..],
            &[ValType::I64][..],
        ),
        ("mav_snapshot", &[][..], &[ValType::I64][..]),
    ] {
        let (_, kind, function_index) = exports
            .iter()
            .find(|(export, _, _)| export == name)
            .ok_or_else(|| ToolError::MissingExport(name.to_owned()))?;
        if *kind != ExternalKind::Func {
            return Err(ToolError::InvalidExport(name.to_owned()));
        }
        let function_type = usize::try_from(*function_index)
            .ok()
            .and_then(|index| functions.get(index))
            .and_then(|type_index| usize::try_from(*type_index).ok())
            .and_then(|type_index| types.get(type_index))
            .ok_or_else(|| ToolError::InvalidExport(name.to_owned()))?;
        if function_type.params() != expected_params || function_type.results() != expected_results
        {
            return Err(ToolError::InvalidExport(name.to_owned()));
        }
    }
    let memory = exports
        .iter()
        .find(|(name, _, _)| name == "memory")
        .ok_or_else(|| ToolError::MissingExport("memory".to_owned()))?;
    if memory.1 != ExternalKind::Memory {
        return Err(ToolError::InvalidExport("memory".to_owned()));
    }
    Ok(())
}

pub fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

pub fn decode_hex<const N: usize>(value: &str) -> Result<[u8; N], ToolError> {
    if value.len() != N * 2 {
        return Err(ToolError::InvalidMetadata("hex value"));
    }
    let mut output = [0_u8; N];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        let high = hex_digit(pair[0]).ok_or(ToolError::InvalidMetadata("hex value"))?;
        let low = hex_digit(pair[1]).ok_or(ToolError::InvalidMetadata("hex value"))?;
        output[index] = (high << 4) | low;
    }
    Ok(output)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn append_custom(module: &mut Vec<u8>, name: &str, data: &[u8]) {
    let mut payload = Vec::new();
    push_leb(&mut payload, name.len() as u32);
    payload.extend_from_slice(name.as_bytes());
    payload.extend_from_slice(data);
    module.push(0);
    push_leb(module, payload.len() as u32);
    module.extend_from_slice(&payload);
}

fn push_leb(bytes: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            break;
        }
    }
}
