//! Validate a directory of connector manifests against the real `mav-codec` schema.
//!
//!     cargo run -p mav-codec --example validate_manifests -- <dir>
//!
//! It loads every `*/manifest.json` under the directory through `Manifest::from_json`, which is the
//! authoritative check the connectors repository's structural validator cannot do on its own. Point
//! it at a checkout of `sennnen/maverick-connectors`. This is a dev tool, not a test, so it adds no
//! dependency from the core on the connectors repository (ADR-011).

use mav_codec::Manifest;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(dir) = args.get(1) else {
        eprintln!("usage: validate_manifests <dir>");
        return ExitCode::from(2);
    };

    let mut manifests: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path().join("manifest.json"))
            .filter(|p| p.is_file())
            .collect(),
        Err(e) => {
            eprintln!("cannot read {dir}: {e}");
            return ExitCode::FAILURE;
        }
    };
    manifests.sort();

    if manifests.is_empty() {
        eprintln!("no */manifest.json found under {dir}");
        return ExitCode::FAILURE;
    }

    let mut failed = false;
    for path in &manifests {
        let display = path.display();
        match std::fs::read_to_string(path) {
            Err(e) => {
                eprintln!("FAIL {display}: {e}");
                failed = true;
            }
            Ok(json) => match Manifest::from_json(&json) {
                Ok(manifest) => println!(
                    "ok   {display}: family {}, {} command(s), {} capability(ies)",
                    manifest.identity.family,
                    manifest.commands.len(),
                    manifest.capabilities.len(),
                ),
                Err(e) => {
                    eprintln!("FAIL {display}: {e}");
                    failed = true;
                }
            },
        }
    }

    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
