//! Frozen, device-neutral `.mavconn` metadata and event/action ABI v1 wire types.
#![forbid(unsafe_code)]

mod artifact;
mod bounds;
mod ids;
mod message;
mod wire;

pub use artifact::*;
pub use bounds::*;
pub use ids::*;
pub use message::*;
pub use wire::{decode_canonical, encode_canonical, Validate, WireError};

/// Packs an unsigned WebAssembly pointer and length into the ABI's signed `i64` carrier.
pub const fn pack_ptr_len(pointer: u32, length: u32) -> i64 {
    ((pointer as u64) << 32 | length as u64) as i64
}

/// Recovers the unsigned pointer and length halves from the ABI carrier.
pub const fn unpack_ptr_len(packed: i64) -> (u32, u32) {
    let bits = packed as u64;
    ((bits >> 32) as u32, bits as u32)
}
