//! Outbound frame building, driven by a `FrameSpec` exactly as inbound validation is: the spec
//! carries the header template, length field, CRCs, and padding rule, and one builder serves
//! every format. gen4 and gen5 are named presets over that data; the gen5 preset is pinned
//! against a real captured hello frame below, which is the strongest evidence we can hold
//! without hardware.

use crate::spec::{FrameSpec, HEADER_TEMPLATE_MAX};
use mav_model::error::{codes, MavError, Result};

pub const START_OF_FRAME: u8 = 0xAA;

/// gen4 is a 4-byte-header wire with a CRC-8 header check; gen5 is an 8-byte-header wire with a
/// CRC-16 header check and a payload padded to a 4-byte boundary. The pair are named presets over
/// the general [`FrameSpec`]; a connector can supply a third format as data.
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

/// Build a complete frame around `payload` for a named preset. Used to encode commands and to
/// construct test fixtures; the reassembler is its inverse.
pub fn build_frame(format: WireFormat, payload: &[u8]) -> Result<Vec<u8>> {
    build_with_spec(&format.spec(), payload)
}

/// Build a complete frame around `payload` from any frame description: header template, then SOF,
/// declared length, and header CRC written over it, then the padded payload, then the trailer CRC.
/// The reassembler validating with the same spec is its inverse (the delivered payload keeps the
/// declared padding, which the trailer CRC covers).
pub fn build_with_spec(spec: &FrameSpec, payload: &[u8]) -> Result<Vec<u8>> {
    if spec.header_len > HEADER_TEMPLATE_MAX {
        return Err(MavError::new(
            codes::FRAME_OVERSIZED,
            "header longer than the builder's template",
        )
        .context(format!(
            "header_len {} > {HEADER_TEMPLATE_MAX}",
            spec.header_len
        )));
    }
    let fields_fit = spec.length.offset + spec.length.width <= spec.header_len
        && spec.header_crc.is_none_or(|c| {
            c.over.0 <= c.over.1
                && c.over.1 <= spec.header_len
                && c.at + c.kind.width() <= spec.header_len
        });
    if !fields_fit {
        return Err(MavError::new(
            codes::FRAME_OVERSIZED,
            "header fields fall outside the declared header",
        ));
    }
    let pad = spec.pad_payload_to.max(1);
    let padded_len = payload.len().div_ceil(pad) * pad;
    let declared = if spec.length_includes_trailer {
        padded_len + spec.trailer.kind.width()
    } else {
        padded_len
    };
    let width = spec.length.width.min(8);
    let max_declared = if width >= 8 {
        u64::MAX
    } else {
        (1u64 << (8 * width)) - 1
    };
    if declared as u64 > max_declared {
        return Err(
            MavError::new(codes::FRAME_OVERSIZED, "payload too large to frame")
                .context(format!("declared {declared} > max {max_declared}")),
        );
    }

    let mut frame = spec.header_template[..spec.header_len].to_vec();
    frame[0] = spec.sof;
    let at = spec.length.offset;
    spec.length
        .endian
        .write(declared as u64, &mut frame[at..at + spec.length.width]);
    if let Some(crc) = spec.header_crc {
        let computed = crc.kind.compute(&frame[crc.over.0..crc.over.1]);
        crc.endian
            .write(computed, &mut frame[crc.at..crc.at + crc.kind.width()]);
    }

    frame.extend_from_slice(payload);
    frame.resize(spec.header_len + padded_len, 0);
    let trailer = spec
        .trailer
        .kind
        .compute(&frame[spec.header_len..spec.header_len + padded_len]);
    let mut trailer_bytes = vec![0u8; spec.trailer.kind.width()];
    spec.trailer.endian.write(trailer, &mut trailer_bytes);
    frame.extend_from_slice(&trailer_bytes);
    Ok(frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc::{crc32, crc8};
    use crate::reassembler::Reassembler;

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

    /// A frame format unlike either preset (BE length that excludes the trailer, no header CRC,
    /// filler byte the template must carry): the builder and the reassembler are inverses.
    fn custom_spec() -> FrameSpec {
        use crate::spec::{CrcKind, Endian, LengthField, Trailer, HEADER_TEMPLATE_MAX};
        FrameSpec {
            sof: 0x7E,
            header_len: 4,
            length: LengthField {
                offset: 1,
                width: 2,
                endian: Endian::Be,
            },
            length_includes_trailer: false,
            header_crc: None,
            trailer: Trailer {
                kind: CrcKind::Crc8,
                endian: Endian::Le,
            },
            pad_payload_to: 1,
            header_template: {
                let mut t = [0; HEADER_TEMPLATE_MAX];
                t[3] = 0x5A;
                t
            },
        }
    }

    #[test]
    fn spec_built_frames_round_trip_through_the_reassembler() {
        for (spec, payload) in [
            (FrameSpec::gen4(), vec![0x28u8, 0x01, 0x00, 0x42]),
            (FrameSpec::gen5(), vec![0x23u8, 0x01, 0x91]),
            (custom_spec(), vec![0x11u8, 0x22, 0x33, 0x44, 0x55]),
        ] {
            let wire = build_with_spec(&spec, &payload).unwrap();
            let mut r = Reassembler::with_spec(spec);
            let frames: Vec<RawFrame> = r
                .push(&wire)
                .into_iter()
                .filter_map(|e| match e {
                    crate::reassembler::ReassemblyEvent::Frame(f) => Some(f),
                    _ => None,
                })
                .collect();
            assert_eq!(frames.len(), 1);
            // The delivered payload is the padded payload; padding is zeros past the input.
            assert_eq!(&frames[0].payload[..payload.len()], &payload[..]);
            assert!(frames[0].payload[payload.len()..].iter().all(|&b| b == 0));
            assert_eq!(frames[0].payload.len() % spec.pad_payload_to.max(1), 0);
        }
    }

    #[test]
    fn custom_template_filler_bytes_reach_the_wire() {
        let wire = build_with_spec(&custom_spec(), &[0xAB]).unwrap();
        assert_eq!(wire[0], 0x7E);
        assert_eq!(&wire[1..3], &[0x00, 0x01], "BE length excludes trailer");
        assert_eq!(wire[3], 0x5A, "template filler survives");
        assert_eq!(wire[4], 0xAB);
        assert_eq!(wire[5], crc8(&[0xAB]));
    }

    #[test]
    fn a_header_field_outside_the_header_is_refused() {
        let mut spec = custom_spec();
        spec.length.offset = 3;
        let err = build_with_spec(&spec, &[0x01]).unwrap_err();
        assert_eq!(err.code, codes::FRAME_OVERSIZED);
    }
}
