//! Typed historical-control facts: what a control packet says, divorced from how any generation
//! lays the bytes out. The M5 controller consumes exactly these values; cursor bytes stay opaque
//! end to end (docs/protocol/whoop.md, the safe-cursor invariant).

use serde::{Deserialize, Serialize};

/// The result byte of a historical control command response. On the wire `1` is ok and `2` is
/// pending [XVAL]; anything else is preserved exactly rather than coerced to a guess.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum ControlResult {
    Ok,
    Pending,
    Unknown(u8),
}

impl ControlResult {
    pub fn from_wire(byte: u8) -> Self {
        match byte {
            1 => Self::Ok,
            2 => Self::Pending,
            other => Self::Unknown(other),
        }
    }
}

/// One decoded control fact from a validated frame. `Response` answers a command; the metadata
/// variants delimit a historical burst. `MetadataUnknown` stays typed and is logged by the caller —
/// it is never mistaken for completion.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "control")]
pub enum HistoricalControl {
    Response {
        to_opcode: u8,
        origin_seq: u8,
        result: ControlResult,
    },
    MetadataStart {
        seq: u8,
    },
    MetadataEnd {
        seq: u8,
        /// Every acknowledgement byte the strap sent, byte for byte. Never parsed, never trimmed.
        ack_payload: Vec<u8>,
        /// No admitted source pins a record count inside `END`; the slot exists for the day a
        /// capture proves one, and stays `None` until then.
        record_count: Option<u32>,
    },
    MetadataComplete {
        seq: u8,
    },
    MetadataUnknown {
        kind: u8,
        seq: u8,
    },
}
