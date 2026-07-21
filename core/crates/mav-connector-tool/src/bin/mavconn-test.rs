use mav_connector_tool::{decode_hex, parity_report, test_fixtures};
use std::env;
use std::error::Error;
use std::fs;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 && args.len() != 5 {
        return Err("usage: mavconn-test ARTIFACT PUBLIC_KEY_HEX [--report OUTPUT.json]".into());
    }
    let results = test_fixtures(fs::read(&args[1])?, decode_hex::<32>(&args[2])?)?;
    for fixture in &results {
        println!(
            "fixture={} events={} execution=ok",
            fixture.name, fixture.events_run
        );
    }
    println!("mavconn-test: {} fixture(s) ok", results.len());
    if args.len() == 5 {
        if args[3] != "--report" {
            return Err("expected --report before output path".into());
        }
        fs::write(
            &args[4],
            parity_report(fs::read(&args[1])?, decode_hex::<32>(&args[2])?)?,
        )?;
    }
    Ok(())
}
