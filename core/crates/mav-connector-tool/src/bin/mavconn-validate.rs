use mav_connector_tool::{decode_hex, validate};
use std::env;
use std::error::Error;
use std::fs;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        return Err("usage: mavconn-validate ARTIFACT PUBLIC_KEY_HEX".into());
    }
    validate(&fs::read(&args[1])?, decode_hex::<32>(&args[2])?)?;
    println!("validate: ok");
    Ok(())
}
