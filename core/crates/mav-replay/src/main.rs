//! `mav-replay <manifest.json> <capture.json>` — run a capture through the full pipeline and print
//! the snapshot, its canonical hash, and every stage boundary. The debugging and fixture tool that
//! stands in for hardware. See docs/pipeline.md.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: mav-replay <manifest.json> <capture.json>");
        return ExitCode::from(2);
    }

    match mav_replay::replay_files(Path::new(&args[1]), Path::new(&args[2])) {
        Ok(replay) => {
            match replay.snapshot.canonical_json() {
                Ok(json) => println!("snapshot: {json}"),
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::FAILURE;
                }
            }
            println!("hash: {}", replay.hash);
            println!("boundary:");
            for entry in &replay.boundary {
                match serde_json::to_string(entry) {
                    Ok(line) => println!("  {line}"),
                    Err(e) => eprintln!("  <unserialisable entry: {e}>"),
                }
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
