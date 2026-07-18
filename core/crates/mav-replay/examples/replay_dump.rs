//! Replay a capture against a manifest and print the computed snapshot, analytics, and hashes as
//! JSON — the raw material for a `fixtures/replay/*.expected.json` file. The evidence block is
//! written by hand after eyeballing these values, per skills/golden-fixtures.
//!
//!     cargo run -p mav-replay --example replay_dump -- <manifest.json> <capture.json>

use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, manifest, capture] = args.as_slice() else {
        eprintln!("usage: replay_dump <manifest.json> <capture.json>");
        std::process::exit(2);
    };
    let replay = match mav_replay::replay_files(Path::new(manifest), Path::new(capture)) {
        Ok(replay) => replay,
        Err(error) => {
            eprintln!("replay failed: {error}");
            std::process::exit(1);
        }
    };
    let out = serde_json::json!({
        "hash": replay.hash,
        "snapshot": replay.snapshot,
        "analytics_hash": replay.analytics_hash,
        "analytics": replay.analytics,
    });
    match serde_json::to_string_pretty(&out) {
        Ok(text) => println!("{text}"),
        Err(error) => {
            eprintln!("serialise failed: {error}");
            std::process::exit(1);
        }
    }
}
