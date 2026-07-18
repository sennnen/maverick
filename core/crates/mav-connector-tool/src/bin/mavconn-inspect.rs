use mav_connector_tool::{encode_hex, inspect};
use std::env;
use std::error::Error;
use std::fs;

fn main() -> Result<(), Box<dyn Error>> {
    let path = env::args()
        .nth(1)
        .ok_or("usage: mavconn-inspect ARTIFACT")?;
    let artifact = inspect(fs::read(path)?)?;
    let report = artifact.report();
    println!("connector_id={}", report.manifest.connector_id.as_str());
    println!("version={}", report.manifest.version);
    println!("publisher_key_id={}", report.signature.publisher_key_id);
    println!("artifact_sha256={}", encode_hex(&report.artifact_digest));
    println!("signed_sha256={}", encode_hex(&report.signed_digest));
    println!("fixtures={}", report.fixtures.cases.len());
    Ok(())
}
