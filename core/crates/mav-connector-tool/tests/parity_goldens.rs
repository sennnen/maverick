#![allow(clippy::expect_used)]

use mav_connector_tool::{decode_hex, parity_report};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

const MAX_FUEL_PER_CALL: u64 = 5_000_000;
const MAX_LINEAR_MEMORY_BYTES: u64 = 4 * 1024 * 1024;

#[test]
fn signed_whoop_artifacts_reproduce_frozen_parity_reports_within_mobile_budgets() {
    for family in ["whoop4", "whoop5"] {
        let public_key = "dfef1d92a685c9df623b8a321740b0a59de0de538bbfea9ddb703394a1e0f5bd";
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let artifact = fs::read(root.join(format!("fixtures/connectors/{family}_v1.mavconn")))
            .expect("signed connector fixture");
        let expected = fs::read_to_string(root.join(format!(
            "fixtures/connectors/{family}_parity_v1.expected.json"
        )))
        .expect("parity report fixture");
        let actual = parity_report(artifact, decode_hex(public_key).expect("public key"))
            .expect("artifact parity execution");
        assert_eq!(actual, expected, "{family} parity report drifted");

        let report: Value = serde_json::from_str(&actual).expect("parity JSON");
        let fixtures = report["fixtures"].as_array().expect("fixture list");
        for required in ["history-cursor-retry", "state-restart", "malformed-frame"] {
            assert!(
                fixtures.iter().any(|fixture| fixture["name"] == required),
                "{family} lacks {required} parity coverage"
            );
        }
        for fixture in fixtures {
            let fuel = fixture["max_fuel_consumed"].as_u64().expect("fuel");
            let memory = fixture["peak_memory_bytes"].as_u64().expect("memory");
            assert!(fuel <= MAX_FUEL_PER_CALL, "{family} fixture fuel exceeded");
            assert!(
                memory <= MAX_LINEAR_MEMORY_BYTES,
                "{family} fixture memory exceeded"
            );
        }
    }
}
