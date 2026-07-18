use mav_connector_abi::{decode_canonical, AbiDescriptor, FixtureSet, Manifest, SignatureRecord};
use mav_model::error::{codes, MavError, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::ops::Range;
use subtle::ConstantTimeEq;
use wasmparser::{Encoding, Parser, Payload, Validator};

const MAX_ARTIFACT_BYTES: usize = 4 * 1024 * 1024;
const MAX_CUSTOM_SECTION_BYTES: usize = 1024 * 1024;
const MAX_SIGNATURE_SECTION_BYTES: usize = 4 * 1024;
const MAX_SECTIONS: u32 = 128;
const SIGNATURE_DOMAIN: &[u8] = b"mavconn-signature-v1\0";
const REQUIRED: [&str; 4] = ["mav:manifest", "mav:abi", "mav:fixtures", "mav:signature"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InspectionReport {
    pub artifact_digest: [u8; 32],
    pub signed_digest: [u8; 32],
    pub manifest: Manifest,
    pub abi: AbiDescriptor,
    pub fixtures: FixtureSet,
    pub signature: SignatureRecord,
    pub section_count: u32,
}

#[derive(Clone, Debug)]
pub struct Artifact {
    bytes: Vec<u8>,
    signature_range: Range<usize>,
    report: InspectionReport,
}

#[derive(Debug)]
struct Sections<'a> {
    manifest: &'a [u8],
    abi: &'a [u8],
    fixtures: &'a [u8],
    signature: &'a [u8],
    signature_range: Range<usize>,
    count: u32,
}

impl Artifact {
    pub fn inspect(bytes: Vec<u8>) -> Result<Self> {
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(error(
                codes::CONNECTOR_ARTIFACT_OVERSIZED,
                "connector artifact exceeds four MiB",
            ));
        }
        Validator::new().validate_all(&bytes).map_err(|source| {
            error(
                codes::CONNECTOR_ARTIFACT_MALFORMED_WASM,
                format!("WebAssembly validation failed: {source}"),
            )
        })?;
        let sections = scan_sections(&bytes)?;
        let manifest: Manifest = decode(sections.manifest, "manifest")?;
        let abi: AbiDescriptor = decode(sections.abi, "ABI")?;
        let fixtures: FixtureSet = decode(sections.fixtures, "fixtures")?;
        let signature: SignatureRecord = decode(sections.signature, "signature")?;

        if manifest.publisher_key_id != signature.publisher_key_id {
            return Err(error(
                codes::CONNECTOR_ARTIFACT_NONCANONICAL_CBOR,
                "manifest and signature publisher ids differ",
            ));
        }
        let fixture_digest: [u8; 32] = Sha256::digest(sections.fixtures).into();
        if !bool::from(fixture_digest.ct_eq(&manifest.fixture_set_hash)) {
            return Err(error(
                codes::CONNECTOR_ARTIFACT_DIGEST_MISMATCH,
                "manifest fixture-set hash does not match mav:fixtures",
            ));
        }

        let signature_range = sections.signature_range.clone();
        let section_count = sections.count;
        let signed_digest = signature_digest([
            &bytes[..signature_range.start],
            &bytes[signature_range.end..],
        ]);
        if !bool::from(signed_digest.ct_eq(&signature.digest)) {
            return Err(error(
                codes::CONNECTOR_ARTIFACT_DIGEST_MISMATCH,
                "signature digest does not match canonical unsigned module",
            ));
        }
        let artifact_digest = Sha256::digest(&bytes).into();
        Ok(Self {
            bytes,
            signature_range,
            report: InspectionReport {
                artifact_digest,
                signed_digest,
                manifest,
                abi,
                fixtures,
                signature,
                section_count,
            },
        })
    }

    pub fn report(&self) -> &InspectionReport {
        &self.report
    }

    pub fn canonical_unsigned_chunks(&self) -> CanonicalUnsigned<'_> {
        CanonicalUnsigned {
            before: Some(&self.bytes[..self.signature_range.start]),
            after: Some(&self.bytes[self.signature_range.end..]),
        }
    }
}

pub struct CanonicalUnsigned<'a> {
    before: Option<&'a [u8]>,
    after: Option<&'a [u8]>,
}

impl<'a> Iterator for CanonicalUnsigned<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<Self::Item> {
        self.before.take().or_else(|| self.after.take())
    }
}

pub fn signature_digest<'a>(chunks: impl IntoIterator<Item = &'a [u8]>) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(SIGNATURE_DOMAIN);
    for chunk in chunks {
        hasher.update(chunk);
    }
    hasher.finalize().into()
}

fn decode<'bytes, T>(bytes: &'bytes [u8], section: &str) -> Result<T>
where
    T: minicbor::Decode<'bytes, ()> + minicbor::Encode<()> + mav_connector_abi::Validate,
{
    decode_canonical(bytes).map_err(|source| {
        error(
            codes::CONNECTOR_ARTIFACT_NONCANONICAL_CBOR,
            format!("mav:{section} CBOR rejected: {source}"),
        )
    })
}

fn scan_sections(bytes: &[u8]) -> Result<Sections<'_>> {
    let mut cursor = 8;
    let mut count = 0_u32;
    let mut seen = BTreeSet::new();
    let mut found: [Option<&[u8]>; 4] = [None; 4];
    let mut signature_range = None;
    let mut next_required = 0_usize;
    let mut required_started = false;

    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(|source| {
            error(
                codes::CONNECTOR_ARTIFACT_MALFORMED_WASM,
                format!("WebAssembly parse failed: {source}"),
            )
        })?;
        match payload {
            Payload::Version {
                encoding: Encoding::Module,
                range,
                ..
            } if range == (0..8) => {
                cursor = range.end;
            }
            Payload::Version { .. } => {
                return Err(error(
                    codes::CONNECTOR_ARTIFACT_MALFORMED_WASM,
                    "artifact is not a core WebAssembly module",
                ));
            }
            Payload::CustomSection(reader) => {
                let range = reader.range();
                let full_range = cursor..range.end;
                cursor = range.end;
                bump_section_count(&mut count)?;
                let name = reader.name();
                let data = reader.data();
                if range.len() > MAX_CUSTOM_SECTION_BYTES
                    || (name == "mav:signature" && range.len() > MAX_SIGNATURE_SECTION_BYTES)
                {
                    return Err(error(
                        codes::CONNECTOR_ARTIFACT_SECTION_OVERSIZED,
                        format!("custom section {name} exceeds its byte limit"),
                    ));
                }
                if name.starts_with("mav:critical:") && !REQUIRED.contains(&name) {
                    return Err(error(
                        codes::CONNECTOR_ARTIFACT_UNKNOWN_CRITICAL_SECTION,
                        format!("unsupported critical section {name}"),
                    ));
                }
                if name.starts_with("mav:") && !seen.insert(name) {
                    return Err(error(
                        codes::CONNECTOR_ARTIFACT_SECTION_DUPLICATE,
                        format!("custom section {name} appears more than once"),
                    ));
                }
                if let Some(index) = REQUIRED.iter().position(|required| *required == name) {
                    if index != next_required {
                        return Err(error(
                            codes::CONNECTOR_ARTIFACT_SECTION_ORDER,
                            format!("custom section {name} is out of order"),
                        ));
                    }
                    required_started = true;
                    next_required += 1;
                    found[index] = Some(data);
                    if name == "mav:signature" {
                        signature_range = Some(full_range);
                    }
                } else if required_started {
                    return Err(error(
                        codes::CONNECTOR_ARTIFACT_SECTION_ORDER,
                        "required mav sections must be the final section sequence",
                    ));
                }
            }
            Payload::End(_) => {}
            other => {
                if let Some((_, range)) = other.as_section() {
                    if required_started {
                        return Err(error(
                            codes::CONNECTOR_ARTIFACT_SECTION_ORDER,
                            "standard sections cannot follow mav metadata",
                        ));
                    }
                    cursor = range.end;
                    bump_section_count(&mut count)?;
                }
            }
        }
    }

    if next_required != REQUIRED.len() {
        return Err(error(
            codes::CONNECTOR_ARTIFACT_SECTION_MISSING,
            format!("required section {} is missing", REQUIRED[next_required]),
        ));
    }
    let [Some(manifest), Some(abi), Some(fixtures), Some(signature)] = found else {
        return Err(error(
            codes::CONNECTOR_ARTIFACT_SECTION_MISSING,
            "one or more required sections are missing",
        ));
    };
    let signature_range = signature_range.ok_or_else(|| {
        error(
            codes::CONNECTOR_ARTIFACT_SECTION_MISSING,
            "mav:signature byte range is missing",
        )
    })?;
    Ok(Sections {
        manifest,
        abi,
        fixtures,
        signature,
        signature_range,
        count,
    })
}

fn error(code: u16, message: impl Into<String>) -> MavError {
    MavError::new(code, message)
}

fn bump_section_count(count: &mut u32) -> Result<()> {
    *count += 1;
    if *count > MAX_SECTIONS {
        return Err(error(
            codes::CONNECTOR_ARTIFACT_SECTION_OVERSIZED,
            "artifact contains more than 128 sections",
        ));
    }
    Ok(())
}
