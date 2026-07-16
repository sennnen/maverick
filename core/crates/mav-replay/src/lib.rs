//! Capture replay: read a capture file and a manifest, run them through the full pipeline, and
//! return the snapshot plus a dump of every stage boundary. It is the primary hardware-free
//! development and fixture-generation tool, and because it drives the same `run_realtime` the FFI
//! does, a capture replayed here runs the identical code a live device would. See docs/pipeline.md.
#![forbid(unsafe_code)]

use mav_engine::{run_realtime_output, AnalyticsSnapshot, Capture, Manifest, Snapshot, Store};
use mav_model::error::{codes, MavError, Result};
use mav_obs::ring::{RingEntry, RingLog, RingLogTap};
use std::path::Path;
use std::sync::Arc;

/// The result of a replay: the snapshot, its canonical hash, and the ordered stage-boundary dump.
pub struct Replay {
    pub snapshot: Snapshot,
    pub hash: String,
    pub analytics: AnalyticsSnapshot,
    pub analytics_hash: String,
    pub boundary: Vec<RingEntry>,
}

/// Replay a capture file against a manifest file, into a fresh in-memory store.
pub fn replay_files(manifest_path: &Path, capture_path: &Path) -> Result<Replay> {
    let manifest = Manifest::from_json(&read(manifest_path)?)?;
    let capture = load_capture(capture_path)?;
    replay(&manifest, &capture)
}

/// Replay an already-parsed capture against a manifest. The tap is the ring log, so the boundary
/// dump is whatever the pipeline emitted, rejections included.
pub fn replay(manifest: &Manifest, capture: &Capture) -> Result<Replay> {
    let log = Arc::new(RingLog::new(4096));
    let tap = RingLogTap(log.clone());
    let store = Store::open_in_memory()?;

    let output = run_realtime_output(manifest, capture, &store, &tap)?;
    let hash = output.snapshot.canonical_hash()?;
    let analytics_hash = output.analytics.canonical_hash()?;
    let boundary = log.recent(4096);
    Ok(Replay {
        snapshot: output.snapshot,
        hash,
        analytics: output.analytics,
        analytics_hash,
        boundary,
    })
}

pub fn load_capture(path: &Path) -> Result<Capture> {
    Capture::from_json(&read(path)?)
}

fn read(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| {
        MavError::new(codes::STORAGE_OPEN, "could not read a file")
            .context(path.display().to_string())
            .context(e.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mav_obs::ring::RingEntryKind;
    use mav_obs::stage::Stage;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../fixtures/replay")
            .join(name)
    }

    fn run() -> Replay {
        replay_files(
            &fixture("realtime_hr_v1.manifest.json"),
            &fixture("realtime_hr_v1.capture.json"),
        )
        .unwrap()
    }

    #[test]
    fn replay_produces_the_expected_snapshot() {
        let replay = run();
        assert_eq!(replay.snapshot.current_bpm, Some(63));
        assert_eq!(replay.snapshot.in_range_samples, 3);
        assert_eq!(replay.snapshot.excluded_samples, 0);
    }

    #[test]
    fn replay_matches_the_frozen_expected_fixture() {
        let replay = run();
        let expected: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(fixture("realtime_hr_v1.expected.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(replay.hash, expected["hash"].as_str().unwrap());
        assert_eq!(
            serde_json::to_value(&replay.snapshot).unwrap(),
            expected["snapshot"]
        );
    }

    #[test]
    fn replay_hash_is_reproducible() {
        assert_eq!(run().hash, run().hash);
    }

    #[test]
    fn boundary_dump_covers_the_stages_in_order() {
        let replay = run();
        let stages: Vec<Stage> = replay
            .boundary
            .iter()
            .filter(|e| matches!(e.kind, RingEntryKind::Produced { .. }))
            .map(|e| e.stage)
            .collect();
        // Decode and SQI fire per frame, then store, features, and snapshots once at the end.
        assert!(stages.contains(&Stage::Decode));
        assert!(stages.contains(&Stage::Sqi));
        assert_eq!(stages.last(), Some(&Stage::Snapshots));
    }

    #[test]
    fn rr_fixture_reproduces_the_frozen_prv_snapshot() {
        let replay = replay_files(
            &fixture("realtime_rr_prv_v1.manifest.json"),
            &fixture("realtime_rr_prv_v1.capture.json"),
        )
        .unwrap();
        let expected: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(fixture("realtime_rr_prv_v1.expected.json")).unwrap(),
        )
        .unwrap();

        assert_eq!(
            serde_json::to_value(&replay.analytics).unwrap(),
            expected["analytics"]
        );
        assert_eq!(
            replay.analytics_hash,
            expected["analytics_hash"].as_str().unwrap()
        );
    }
}
