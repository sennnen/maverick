//! The UniFFI facade: the single surface iOS and Android call into (ADR-010). It exposes only what
//! an app needs and nothing of the pipeline's internals, so the types behind it can keep moving
//! while the boundary stays small. The two functions here are the Milestone 1 surface: the core
//! version, and running a capture through the pipeline to a snapshot plus its parity hash.
//!
//! Generating the Swift and Kotlin bindings and linking them on each platform is documented in
//! apps/ios/README.md and apps/android/README.md; the Rust side and the bindgen step are verified
//! in CI, and the simulator link is a documented local step until the app milestone.
#![forbid(unsafe_code)]

use mav_model::error::MavError;
use mav_obs::stage::Stage;
use mav_obs::tap::{Tap, TapEvent};

uniffi::setup_scaffolding!();

/// The error the FFI hands back. Flattened to its message across the boundary, because a host app
/// wants a readable reason and the stable numeric code is already inside it (`MAV-<code>`).
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum FfiError {
    #[error("{0}")]
    Core(String),
}

impl From<MavError> for FfiError {
    fn from(error: MavError) -> Self {
        FfiError::Core(error.to_string())
    }
}

/// The result of running a capture: the canonical snapshot JSON and its parity hash.
#[derive(Debug, uniffi::Record)]
pub struct RunResult {
    pub snapshot_json: String,
    pub hash: String,
}

/// A tap that keeps nothing. The FFI entry runs a one-shot capture and returns the snapshot; a host
/// that wants the boundary dump uses `mav-replay` or a future streaming surface.
struct DiscardTap;
impl Tap for DiscardTap {
    fn on_stage(&self, _stage: Stage, _event: TapEvent) {}
}

/// The core version, so a host and a bug report can pin exactly which build produced a result.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Run one `capture/v1` capture against a device manifest and return the snapshot as canonical JSON
/// with its hash. Both inputs are JSON strings the host already holds, so the boundary carries no
/// pipeline types. This is the function the parity harness drives on each platform: the same inputs
/// must return the same hash, and any difference is a binding bug.
#[uniffi::export]
pub fn run_capture(manifest_json: String, capture_json: String) -> Result<RunResult, FfiError> {
    let snapshot = mav_engine::run_realtime_json(&manifest_json, &capture_json, &DiscardTap)?;
    Ok(RunResult {
        snapshot_json: snapshot.canonical_json()?,
        hash: snapshot.canonical_hash()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/replay")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
    }

    #[test]
    fn core_version_matches_the_crate() {
        assert_eq!(core_version(), env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn run_capture_reproduces_the_golden_hash() {
        // The FFI must return exactly what mav-replay froze for the same fixture.
        let result = run_capture(
            fixture("realtime_hr_v1.manifest.json"),
            fixture("realtime_hr_v1.capture.json"),
        )
        .unwrap();
        assert_eq!(result.hash, "33143ef069a85a38");
        assert!(result.snapshot_json.contains("\"current_bpm\":63"));
    }

    #[test]
    fn a_broken_capture_is_a_readable_error() {
        let err = run_capture("{}".to_owned(), "{}".to_owned()).unwrap_err();
        let FfiError::Core(message) = err;
        assert!(message.starts_with("MAV-"), "{message}");
    }
}
