//! Frame layouts and builders for the two WHOOP wire formats. The layouts come from two
//! independent reverse-engineered codebases that agree exactly; docs/protocol/whoop.md is the
//! narrative reference. The gen5 builder is pinned against a real captured hello frame below,
//! which is the strongest evidence we can hold without hardware.

use crate::crc::{crc16_modbus, crc32, crc8};
use crate::spec::FrameSpec;
use mav_model::error::{codes, MavError, Result};

pub const START_OF_FRAME: u8 = 0xAA;

/// gen4 is the WHOOP 4.0 wire (4-byte header, CRC-8 header check); gen5 is the WHOOP 5.0 and MG
/// wire (8-byte header, CRC-16 header check, payload padded to a 4-byte boundary). The pair are
/// named presets over the general [`FrameSpec`]; a connector can supply a third format as data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WireFormat {
    Gen4,
    Gen5,
}

impl WireFormat {
    pub const fn header_len(self) -> usize {
        match self {
            WireFormat::Gen4 => 4,
            WireFormat::Gen5 => 8,
        }
    }

    /// The data-driven description of this format, used by the reassembler.
    pub const fn spec(self) -> FrameSpec {
        match self {
            WireFormat::Gen4 => FrameSpec::gen4(),
            WireFormat::Gen5 => FrameSpec::gen5(),
        }
    }
}

/// A validated frame: both CRCs checked. For gen5 the payload keeps its zero padding, because the
/// inner layout is offset-addressed from the payload start and the padding is part of what the
/// CRC-32 covers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RawFrame {
    pub payload: Vec<u8>,
}

/// Build a complete frame around `payload`. Used to encode commands and to construct test
/// fixtures; the reassembler is its inverse.
pub fn build_frame(format: WireFormat, payload: &[u8]) -> Result<Vec<u8>> {
    match format {
        WireFormat::Gen4 => {
            let declared = payload
                .len()
                .checked_add(4)
                .filter(|&n| n <= usize::from(u16::MAX))
                .ok_or_else(|| {
                    MavError::new(codes::FRAME_OVERSIZED, "gen4 payload too large to frame")
                })?;
            let declared = declared as u16;
            let len_bytes = declared.to_le_bytes();
            let mut frame = Vec::with_capacity(4 + payload.len() + 4);
            frame.push(START_OF_FRAME);
            frame.extend_from_slice(&len_bytes);
            frame.push(crc8(&len_bytes));
            frame.extend_from_slice(payload);
            frame.extend_from_slice(&crc32(payload).to_le_bytes());
            Ok(frame)
        }
        WireFormat::Gen5 => {
            let padding = (4 - payload.len() % 4) % 4;
            let padded_len = payload.len() + padding;
            let declared = padded_len
                .checked_add(4)
                .filter(|&n| n <= usize::from(u16::MAX))
                .ok_or_else(|| {
                    MavError::new(codes::FRAME_OVERSIZED, "gen5 payload too large to frame")
                })?;
            let declared = declared as u16;
            let mut frame = Vec::with_capacity(8 + padded_len + 4);
            frame.push(START_OF_FRAME);
            frame.push(0x01);
            frame.extend_from_slice(&declared.to_le_bytes());
            frame.extend_from_slice(&[0x00, 0x01]);
            let header_crc = crc16_modbus(&frame[0..6]);
            frame.extend_from_slice(&header_crc.to_le_bytes());
            frame.extend_from_slice(payload);
            frame.resize(8 + padded_len, 0);
            let payload_crc = crc32(&frame[8..8 + padded_len]);
            frame.extend_from_slice(&payload_crc.to_le_bytes());
            Ok(frame)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The static gen5 client hello, byte for byte as captured from real straps by both source
    /// codebases: GET_HELLO (cmd 0x91 = 145), seq 1, data [0x01].
    const GEN5_HELLO: [u8; 16] = [
        0xAA, 0x01, 0x08, 0x00, 0x00, 0x01, 0xE6, 0x71, 0x23, 0x01, 0x91, 0x01, 0x36, 0x3E, 0x5C,
        0x8D,
    ];

    #[test]
    fn gen5_builder_reproduces_the_captured_hello() {
        let frame = build_frame(WireFormat::Gen5, &[0x23, 0x01, 0x91, 0x01]).unwrap();
        assert_eq!(frame, GEN5_HELLO);
    }

    #[test]
    fn gen5_pads_payload_to_four_byte_boundary() {
        let frame = build_frame(WireFormat::Gen5, &[0x23, 0x01, 0x91]).unwrap();
        // Declared length covers padded payload (4) plus CRC-32 (4).
        assert_eq!(u16::from_le_bytes([frame[2], frame[3]]), 8);
        assert_eq!(frame.len(), 16);
        assert_eq!(frame[11], 0x00);
    }

    #[test]
    fn gen4_layout_matches_spec() {
        let payload = [0x23u8, 0x07, 0x0A, 0x01];
        let frame = build_frame(WireFormat::Gen4, &payload).unwrap();
        assert_eq!(frame[0], START_OF_FRAME);
        assert_eq!(u16::from_le_bytes([frame[1], frame[2]]), 8);
        assert_eq!(frame[3], crc8(&frame[1..3]));
        assert_eq!(&frame[4..8], &payload);
        assert_eq!(
            u32::from_le_bytes([frame[8], frame[9], frame[10], frame[11]]),
            crc32(&payload)
        );
        assert_eq!(frame.len(), 12);
    }

    #[test]
    fn oversized_payload_is_refused() {
        let big = vec![0u8; usize::from(u16::MAX)];
        let err = build_frame(WireFormat::Gen4, &big).unwrap_err();
        assert_eq!(err.code, codes::FRAME_OVERSIZED);
        let err = build_frame(WireFormat::Gen5, &big).unwrap_err();
        assert_eq!(err.code, codes::FRAME_OVERSIZED);
    }
}
