//! The UniFFI facade: the single surface iOS and Android call into (ADR-010). It exposes only what
//! an app needs and nothing of the pipeline's internals, so the types behind it can keep moving
//! while the boundary stays small. The two functions here are the Milestone 1 surface: the core
//! version, and running a capture through the pipeline to canonical read models plus parity hashes.
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

/// Canonical session and analytics read models, each paired with its parity hash.
#[derive(Debug, uniffi::Record)]
pub struct RunResult {
    pub snapshot_json: String,
    pub hash: String,
    pub analytics_json: String,
    pub analytics_hash: String,
}

/// A tap that keeps nothing. A host that wants the boundary dump uses `mav-replay` or a future
/// streaming surface.
struct DiscardTap;
impl Tap for DiscardTap {
    fn on_stage(&self, _stage: Stage, _event: TapEvent) {}
}

/// The core version, so a host and a bug report can pin exactly which build produced a result.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Run one `capture/v1` capture against a device manifest and return canonical session and analytics
/// JSON with their hashes. Both inputs are JSON strings the host already holds, so the boundary
/// carries no pipeline types. The parity harness drives this on each platform: the same inputs must
/// return the same hashes, and any difference is a binding bug.
#[uniffi::export]
pub fn run_capture(manifest_json: String, capture_json: String) -> Result<RunResult, FfiError> {
    let output = mav_engine::run_realtime_output_json(&manifest_json, &capture_json, &DiscardTap)?;
    Ok(RunResult {
        snapshot_json: output.snapshot.canonical_json()?,
        hash: output.snapshot.canonical_hash()?,
        analytics_json: output.analytics.canonical_json()?,
        analytics_hash: output.analytics.canonical_hash()?,
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
        assert!(result.analytics_json.contains("\"availability\""));
    }

    #[test]
    fn run_capture_exposes_the_frozen_prv_analytics() {
        let result = run_capture(
            fixture("realtime_rr_prv_v1.manifest.json"),
            fixture("realtime_rr_prv_v1.capture.json"),
        )
        .unwrap();
        assert_eq!(result.analytics_hash, "e77c7b04c7fceb2c");
        assert!(result
            .analytics_json
            .contains("\"variability_label\":\"pulse_rate_variability\""));
        assert!(result
            .analytics_json
            .contains("\"kind\":\"algorithm_not_admitted\""));
    }

    #[test]
    fn a_broken_capture_is_a_readable_error() {
        let err = run_capture("{}".to_owned(), "{}".to_owned()).unwrap_err();
        let FfiError::Core(message) = err;
        assert!(message.starts_with("MAV-"), "{message}");
    }
}
