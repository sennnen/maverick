//! `mav-replay <connector.mavconn> <publisher-public-key-hex>`.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: mav-replay <connector.mavconn> <publisher-public-key-hex>");
        return ExitCode::from(2);
    }
    let key = match mav_replay::decode_public_key(&args[2]) {
        Ok(key) => key,
        Err(error) => {
            eprintln!("error: {error}");
            return ExitCode::FAILURE;
        }
    };
    match mav_replay::replay_file(Path::new(&args[1]), key) {
        Ok(replay) => {
            println!(
                "connector={} version={} fixtures={}",
                replay.connector_id,
                replay.connector_version,
                replay.fixtures.len()
            );
            for fixture in replay.fixtures {
                println!(
                    "fixture={} events={} fuel={} memory={}",
                    fixture.name,
                    fixture.events_run,
                    fixture.max_fuel_consumed,
                    fixture.peak_memory_bytes
                );
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
