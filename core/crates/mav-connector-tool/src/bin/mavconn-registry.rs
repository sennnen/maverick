use ed25519_dalek::{Signature, VerifyingKey};
use mav_connector_runtime::{
    encode_signed_registry, ingest_registry, registry_signing_digest, RegistryIndex, RegistryRoot,
    TrustPolicy,
};
use std::path::Path;

fn main() {
    if let Err(message) = run(std::env::args().collect()) {
        eprintln!("mavconn-registry: {message}");
        std::process::exit(1);
    }
}

fn run(args: Vec<String>) -> Result<(), String> {
    match args.get(1).map(String::as_str) {
        Some("prepare") if args.len() == 3 => prepare(Path::new(&args[2])),
        Some("finalize") if args.len() == 7 => finalize(
            Path::new(&args[2]),
            &args[3],
            &args[4],
            &args[5],
            Path::new(&args[6]),
        ),
        Some("verify") if args.len() == 7 => verify(
            Path::new(&args[2]),
            &args[3],
            &args[4],
            &args[5],
            &args[6],
        ),
        Some("verify-artifact") if args.len() == 10 => verify_artifact(
            Path::new(&args[2]),
            &args[3],
            &args[4],
            &args[5],
            &args[6],
            &args[7],
            &args[8],
            Path::new(&args[9]),
        ),
        _ => Err(
            "usage: mavconn-registry prepare <unsigned-index.json> | finalize <unsigned-index.json> <root-key-id> <signature-hex> <public-key-hex> <output.json> | verify <signed-index.json> <registry-id> <root-key-id> <public-key-hex> <now-ms> | verify-artifact <signed-index.json> <registry-id> <root-key-id> <public-key-hex> <now-ms> <connector-id> <version> <artifact.mavconn>"
                .to_owned(),
        ),
    }
}

fn prepare(path: &Path) -> Result<(), String> {
    let index = read_index(path)?;
    let digest = registry_signing_digest(&index).map_err(|error| error.to_string())?;
    println!("{}", hex(&digest));
    Ok(())
}

fn finalize(
    index_path: &Path,
    key_id: &str,
    signature_hex: &str,
    public_key_hex: &str,
    output: &Path,
) -> Result<(), String> {
    let index = read_index(index_path)?;
    let signature = decode::<64>(signature_hex, "signature")?;
    let public_key = decode::<32>(public_key_hex, "public key")?;
    let digest = registry_signing_digest(&index).map_err(|error| error.to_string())?;
    let verifying_key = VerifyingKey::from_bytes(&public_key)
        .map_err(|_| "invalid Ed25519 public key".to_owned())?;
    verifying_key
        .verify_strict(&digest, &Signature::from_bytes(&signature))
        .map_err(|_| "signature does not verify the registry signing digest".to_owned())?;
    let bytes = encode_signed_registry(index, key_id.to_owned(), signature)
        .map_err(|error| error.to_string())?;
    std::fs::write(output, bytes).map_err(|error| format!("write {}: {error}", output.display()))
}

fn verify(
    path: &Path,
    registry_id: &str,
    key_id: &str,
    public_key_hex: &str,
    now_ms: &str,
) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let now_ms = now_ms
        .parse::<i64>()
        .map_err(|_| "now-ms must be a signed 64-bit integer".to_owned())?;
    let root = RegistryRoot {
        registry_id: registry_id.to_owned(),
        key_id: key_id.to_owned(),
        public_key: decode::<32>(public_key_hex, "public key")?,
    };
    let snapshot = ingest_registry(
        &bytes,
        &root,
        None,
        &TrustPolicy {
            revision: 0,
            allow_third_party: false,
            allow_development: false,
            keys: Vec::new(),
        },
        now_ms,
    )
    .map_err(|error| error.to_string())?;
    println!(
        "registry_id={} revision={} sha256={}",
        snapshot.index.registry_id,
        snapshot.index.revision,
        hex(&snapshot.digest)
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn verify_artifact(
    index_path: &Path,
    registry_id: &str,
    key_id: &str,
    public_key_hex: &str,
    now_ms: &str,
    connector_id: &str,
    version: &str,
    artifact_path: &Path,
) -> Result<(), String> {
    let bytes = std::fs::read(index_path)
        .map_err(|error| format!("read {}: {error}", index_path.display()))?;
    let artifact = std::fs::read(artifact_path)
        .map_err(|error| format!("read {}: {error}", artifact_path.display()))?;
    let now_ms = now_ms
        .parse::<i64>()
        .map_err(|_| "now-ms must be a signed 64-bit integer".to_owned())?;
    let snapshot = ingest_registry(
        &bytes,
        &RegistryRoot {
            registry_id: registry_id.to_owned(),
            key_id: key_id.to_owned(),
            public_key: decode::<32>(public_key_hex, "public key")?,
        },
        None,
        &TrustPolicy {
            revision: 0,
            allow_third_party: false,
            allow_development: false,
            keys: Vec::new(),
        },
        now_ms,
    )
    .map_err(|error| error.to_string())?;
    let entry = snapshot
        .index
        .entries
        .iter()
        .find(|entry| entry.connector_id == connector_id && entry.version == version)
        .ok_or_else(|| format!("registry has no entry for {connector_id} {version}"))?;
    entry
        .verify_artifact(&artifact)
        .map_err(|error| error.to_string())?;
    println!(
        "connector_id={connector_id} version={version} sha256={}",
        hex(&entry.artifact_sha256)
    );
    Ok(())
}

fn read_index(path: &Path) -> Result<RegistryIndex, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("parse {}: {error}", path.display()))
}

fn decode<const N: usize>(value: &str, field: &str) -> Result<[u8; N], String> {
    if value.len() != N * 2 {
        return Err(format!(
            "{field} must contain exactly {} hex characters",
            N * 2
        ));
    }
    let mut bytes = [0_u8; N];
    for (index, byte) in bytes.iter_mut().enumerate() {
        let offset = index * 2;
        *byte = u8::from_str_radix(&value[offset..offset + 2], 16)
            .map_err(|_| format!("{field} contains invalid hexadecimal"))?;
    }
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
