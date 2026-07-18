use crate::{Validate, WireError};

pub const MAX_CONNECTOR_ID_BYTES: usize = 128;
pub const MAX_TEXT_BYTES: usize = 1_024;
pub const MAX_LABEL_BYTES: usize = 128;
pub const MAX_UUID_BYTES: usize = 64;
pub const MAX_LOGICAL_ID_BYTES: usize = 64;
pub const MAX_EVENT_BYTES: usize = 65_536;
pub const MAX_STATE_BYTES: usize = 65_536;
pub const MAX_DIAGNOSTIC_BYTES: usize = 1_024;
pub const MAX_STATE_KEY_BYTES: usize = 128;
pub const MAX_ACTIONS: usize = 32;
pub const MAX_SAMPLES_PER_ACTION: usize = 512;
pub const MAX_FIXTURES: usize = 64;
pub const MAX_FIXTURE_EVENTS: usize = 512;
pub const MAX_FIXTURE_ACTIONS: usize = 512;
pub const MAX_DEVICE_FAMILIES: usize = 32;
pub const MAX_SERVICES: usize = 32;
pub const MAX_CHARACTERISTICS: usize = 128;
pub const MAX_CAPABILITIES: usize = 64;
pub const MAX_SCAN_FILTERS: usize = 32;
pub const MAX_TIMER_DELAY_MS: u64 = 86_400_000;

pub(crate) fn text(value: &str, max: usize, field: &'static str) -> Result<(), WireError> {
    if value.is_empty() || value.len() > max {
        return Err(WireError::Bounds(field));
    }
    Ok(())
}

pub(crate) fn identifier(value: &str, max: usize, field: &'static str) -> Result<(), WireError> {
    text(value, max, field)?;
    let bytes = value.as_bytes();
    let edge_is_valid = bytes
        .first()
        .zip(bytes.last())
        .is_some_and(|(first, last)| first.is_ascii_alphanumeric() && last.is_ascii_alphanumeric());
    if !edge_is_valid
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
    {
        return Err(WireError::Schema(field));
    }
    Ok(())
}

pub(crate) fn bytes(value: &[u8], max: usize, field: &'static str) -> Result<(), WireError> {
    if value.len() > max {
        return Err(WireError::Bounds(field));
    }
    Ok(())
}

pub(crate) fn count(len: usize, max: usize, field: &'static str) -> Result<(), WireError> {
    if len > max {
        return Err(WireError::Bounds(field));
    }
    Ok(())
}

pub(crate) fn all<T: Validate>(values: &[T]) -> Result<(), WireError> {
    for value in values {
        value.validate()?;
    }
    Ok(())
}
