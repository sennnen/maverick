//! The provenance id and the running trace hash over one session's events and actions.

use super::*;

pub(super) fn metadata_id(session_id: u64, batch_id: u64, index: u64) -> u64 {
    let mut value = 0xcbf2_9ce4_8422_2325_u64;
    for part in [session_id, batch_id, index] {
        for byte in part.to_le_bytes() {
            value ^= u64::from(byte);
            value = value.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    value | (1_u64 << 63)
}

pub(super) fn trace_event(hash: u64, value: &ConnectorEvent) -> Result<u64> {
    let bytes = mav_connector_abi::encode_canonical(value).map_err(|source| {
        error(
            codes::CONNECTOR_HOST_ACTION_INVALID,
            format!("connector trace value is not canonical: {source}"),
        )
    })?;
    Ok(trace_bytes(hash, bytes))
}

pub(super) fn trace_action(hash: u64, value: &ConnectorAction) -> Result<u64> {
    let bytes = mav_connector_abi::encode_canonical(value).map_err(|source| {
        error(
            codes::CONNECTOR_HOST_ACTION_INVALID,
            format!("connector trace value is not canonical: {source}"),
        )
    })?;
    Ok(trace_bytes(hash, bytes))
}

pub(super) fn trace_bytes(hash: u64, bytes: Vec<u8>) -> u64 {
    bytes.into_iter().fold(hash, |current, byte| {
        (current ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}
