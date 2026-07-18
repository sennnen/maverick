use minicbor::{Decode, Decoder, Encode};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireError {
    Encode(String),
    Decode(String),
    TrailingBytes,
    NonCanonical,
    Bounds(&'static str),
    Schema(&'static str),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Encode(message) => write!(f, "CBOR encode failed: {message}"),
            Self::Decode(message) => write!(f, "CBOR decode failed: {message}"),
            Self::TrailingBytes => f.write_str("CBOR input has trailing bytes"),
            Self::NonCanonical => f.write_str("CBOR input is not the canonical schema encoding"),
            Self::Bounds(field) => write!(f, "wire bound exceeded: {field}"),
            Self::Schema(field) => write!(f, "wire schema rejected: {field}"),
        }
    }
}

impl std::error::Error for WireError {}

pub trait Validate {
    fn validate(&self) -> Result<(), WireError>;
}

pub fn encode_canonical<T>(value: &T) -> Result<Vec<u8>, WireError>
where
    T: Encode<()> + Validate,
{
    value.validate()?;
    minicbor::to_vec(value).map_err(|error| WireError::Encode(error.to_string()))
}

pub fn decode_canonical<'bytes, T>(bytes: &'bytes [u8]) -> Result<T, WireError>
where
    T: Decode<'bytes, ()> + Encode<()> + Validate,
{
    let mut decoder = Decoder::new(bytes);
    let value =
        T::decode(&mut decoder, &mut ()).map_err(|error| WireError::Decode(error.to_string()))?;
    if decoder.position() != bytes.len() {
        return Err(WireError::TrailingBytes);
    }
    value.validate()?;
    let canonical = encode_canonical(&value)?;
    if canonical != bytes {
        return Err(WireError::NonCanonical);
    }
    Ok(value)
}
