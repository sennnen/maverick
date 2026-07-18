//! Historical-control decode: COMMAND_RESPONSE (0x24) and METADATA (0x31) into the typed facts
//! the M5 controller consumes. The inner header — packet type, sequence, command/kind — is [XVAL]
//! across generations; only documented offsets are read, and no parser scans for a cursor
//! (docs/protocol/whoop.md). Data packets are not this module's job and return `None`.

use mav_model::error::{codes, MavError, Result};
use mav_model::historical::{ControlResult, HistoricalControl};

const COMMAND_RESPONSE: u8 = 0x24;
const METADATA: u8 = 0x31;

const METADATA_START: u8 = 1;
const METADATA_END: u8 = 2;
const METADATA_COMPLETE: u8 = 3;

/// Decode one validated frame payload into a control fact, `None` when the packet is not a
/// control packet, or `DECODE_FIELD_UNREADABLE` when a control packet is shorter than its
/// documented fixed header.
pub fn decode_control(payload: &[u8]) -> Result<Option<HistoricalControl>> {
    let Some(&packet_type) = payload.first() else {
        return Err(unreadable("packet type", payload.len()));
    };
    match packet_type {
        COMMAND_RESPONSE => decode_response(payload).map(Some),
        METADATA => decode_metadata(payload).map(Some),
        _ => Ok(None),
    }
}

fn decode_response(payload: &[u8]) -> Result<HistoricalControl> {
    let [_, origin_seq, to_opcode, result, ..] = payload else {
        return Err(unreadable("command response header", payload.len()));
    };
    Ok(HistoricalControl::Response {
        to_opcode: *to_opcode,
        origin_seq: *origin_seq,
        result: ControlResult::from_wire(*result),
    })
}

/// Where HISTORY_END's 8-byte end_data (trim cursor `u32` + next `u32`) sits in the metadata
/// body: inner 13..21, i.e. body 10..18. The acknowledgement must echo exactly these eight bytes
/// — the strap re-serves the same chunk forever otherwise — and never the whole body, whose
/// leading bytes are the record unix and counters, not cursor material. Pinned by a real 5.0/MG
/// capture (fixtures/control/gen5_history_end_v2.json, [WRS]).
const END_DATA_RANGE: std::ops::Range<usize> = 10..18;

fn decode_metadata(payload: &[u8]) -> Result<HistoricalControl> {
    let [_, seq, kind, body @ ..] = payload else {
        return Err(unreadable("metadata header", payload.len()));
    };
    Ok(match *kind {
        METADATA_START => HistoricalControl::MetadataStart { seq: *seq },
        METADATA_END => {
            let Some(end_data) = body.get(END_DATA_RANGE) else {
                return Err(unreadable("history-end end_data", payload.len()));
            };
            HistoricalControl::MetadataEnd {
                seq: *seq,
                ack_payload: end_data.to_vec(),
                record_count: None,
            }
        }
        METADATA_COMPLETE => HistoricalControl::MetadataComplete { seq: *seq },
        other => HistoricalControl::MetadataUnknown {
            kind: other,
            seq: *seq,
        },
    })
}

fn unreadable(what: &str, len: usize) -> MavError {
    MavError::new(
        codes::DECODE_FIELD_UNREADABLE,
        "control packet too short for its documented header",
    )
    .context(format!("{what}, payload length {len}"))
}
