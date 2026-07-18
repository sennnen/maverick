//! The wire level: CRC primitives, frame building and validation, streaming reassembly, and a
//! bounds-checked byte reader. Everything here is pure and deterministic; bytes in, frames or
//! typed errors out. The frame layouts themselves are documented in docs/protocol/whoop.md, and
//! the gen5 layout is pinned by a real captured hello frame in this crate's tests.
#![forbid(unsafe_code)]

pub mod crc;
pub mod frame;
pub mod reader;
pub mod reassembler;
pub mod spec;

pub use crc::{crc16_modbus, crc32, crc8};
pub use frame::{build_frame, build_with_spec, RawFrame, WireFormat};
pub use reader::TypedReader;
pub use reassembler::{Reassembler, ReassemblyEvent};
pub use spec::{CrcKind, Endian, FrameSpec, HeaderCrc, LengthField, Trailer};
