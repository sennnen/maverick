use mav_model::error::{codes, MavError, Result};
use std::ops::Range;
use wasmi::{Memory, Store};

pub(crate) fn write(
    memory: Memory,
    store: &mut Store<crate::instance::HostState>,
    pointer: u32,
    bytes: &[u8],
) -> Result<Range<usize>> {
    let length = u32::try_from(bytes.len()).map_err(|_| {
        error(
            codes::CONNECTOR_RUNTIME_INPUT_OVERSIZED,
            "connector input cannot fit the ABI length carrier",
        )
    })?;
    let range = checked_range(memory, store, pointer, length)?;
    memory.write(store, range.start, bytes).map_err(|source| {
        error(
            codes::CONNECTOR_RUNTIME_MEMORY_ACCESS,
            format!("guest memory write failed: {source}"),
        )
    })?;
    Ok(range)
}

pub(crate) fn read(
    memory: Memory,
    store: &Store<crate::instance::HostState>,
    pointer: u32,
    length: u32,
) -> Result<(Vec<u8>, Range<usize>)> {
    let range = checked_range(memory, store, pointer, length)?;
    let mut bytes = vec![0_u8; range.len()];
    memory
        .read(store, range.start, &mut bytes)
        .map_err(|source| {
            error(
                codes::CONNECTOR_RUNTIME_MEMORY_ACCESS,
                format!("guest memory read failed: {source}"),
            )
        })?;
    Ok((bytes, range))
}

fn checked_range(
    memory: Memory,
    store: &Store<crate::instance::HostState>,
    pointer: u32,
    length: u32,
) -> Result<Range<usize>> {
    let start = pointer as usize;
    let end = start.checked_add(length as usize).ok_or_else(|| {
        error(
            codes::CONNECTOR_RUNTIME_MEMORY_ACCESS,
            "guest pointer and length overflow host size",
        )
    })?;
    if start == 0 || end > memory.data(store).len() {
        return Err(error(
            codes::CONNECTOR_RUNTIME_MEMORY_ACCESS,
            "guest pointer and length are outside linear memory",
        ));
    }
    Ok(start..end)
}

pub(crate) fn overlaps(first: &Range<usize>, second: &Range<usize>) -> bool {
    first.start < second.end && second.start < first.end
}

fn error(code: u16, message: impl Into<String>) -> MavError {
    MavError::new(code, message)
}
