use mav_connector_tool::{decode_hex, inspect, validate};
use std::env;
use std::error::Error;
use std::fs;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        return Err("usage: mavconn-test ARTIFACT PUBLIC_KEY_HEX".into());
    }
    let bytes = fs::read(&args[1])?;
    validate(&bytes, decode_hex::<32>(&args[2])?)?;
    let artifact = inspect(bytes)?;
    for fixture in &artifact.report().fixtures.cases {
        println!("fixture={} structural=ok", fixture.name);
    }
    println!(
        "mavconn-test: {} structural fixture(s) ok; execution begins in runtime packet WC-P4",
        artifact.report().fixtures.cases.len()
    );
    Ok(())
}
