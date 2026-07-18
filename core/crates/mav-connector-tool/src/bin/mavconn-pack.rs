use mav_connector_tool::{decode_hex, encode_hex, finalize, prepare_encoded, prepared_unsigned};
use std::env;
use std::error::Error;
use std::fs;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("digest") if args.len() == 7 => digest(&args),
        Some("finalize") if args.len() == 7 => finalize_command(&args),
        _ => Err(
            "usage: mavconn-pack digest MODULE MANIFEST ABI FIXTURES UNSIGNED\n       mavconn-pack finalize UNSIGNED PUBLISHER_ID PUBLIC_KEY_HEX SIGNATURE_HEX OUTPUT"
                .into(),
        ),
    }
}

fn digest(args: &[String]) -> Result<(), Box<dyn Error>> {
    let prepared = prepare_encoded(
        &fs::read(&args[2])?,
        &fs::read(&args[3])?,
        &fs::read(&args[4])?,
        &fs::read(&args[5])?,
    )?;
    fs::write(&args[6], &prepared.bytes)?;
    println!("{}", encode_hex(&prepared.digest));
    Ok(())
}

fn finalize_command(args: &[String]) -> Result<(), Box<dyn Error>> {
    let prepared = prepared_unsigned(fs::read(&args[2])?, args[3].clone())?;
    let public_key = decode_hex::<32>(&args[4])?;
    let signature = decode_hex::<64>(&args[5])?;
    let artifact = finalize(prepared, signature, public_key)?;
    fs::write(&args[6], artifact)?;
    Ok(())
}
