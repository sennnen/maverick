//! Outbound frame building, driven by a `FrameSpec` exactly as inbound validation is: the spec
//! carries the header template, length field, CRCs, and padding rule, and one builder serves
//! every format. Device wire constants are not named here; a connector supplies its own spec as
//! data (ADR-012).

use crate::spec::{FrameSpec, HEADER_TEMPLATE_MAX};
use mav_model::error::{codes, MavError, Result};

pub const START_OF_FRAME: u8 = 0xAA;

/// A validated frame: both CRCs checked. A padded payload keeps its zero padding, because inner
/// layouts are offset-addressed from the payload start and the padding is part of what the
/// trailer CRC covers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RawFrame {
    pub payload: Vec<u8>,
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
    use crate::spec::{CrcKind, Endian, HeaderCrc, LengthField, Trailer};

    /// Compact format: 4-byte header, little-endian length that counts the trailer, CRC-8 header
    /// check, CRC-32 trailer, no payload padding.
    fn compact_spec() -> FrameSpec {
        FrameSpec {
            sof: 0xAA,
            header_len: 4,
            length: LengthField {
                offset: 1,
                width: 2,
                endian: Endian::Le,
            },
            length_includes_trailer: true,
            header_crc: Some(HeaderCrc {
                kind: CrcKind::Crc8,
                over: (1, 3),
                at: 3,
                endian: Endian::Le,
            }),
            trailer: Trailer {
                kind: CrcKind::Crc32,
                endian: Endian::Le,
            },
            pad_payload_to: 1,
            header_template: [0; HEADER_TEMPLATE_MAX],
        }
    }

    /// Routed format: 8-byte header carrying template markers the header CRC covers, CRC-16 header
    /// check, and a payload padded to four bytes.
    fn routed_spec() -> FrameSpec {
        FrameSpec {
            sof: 0xAA,
            header_len: 8,
            length: LengthField {
                offset: 2,
                width: 2,
                endian: Endian::Le,
            },
            length_includes_trailer: true,
            header_crc: Some(HeaderCrc {
                kind: CrcKind::Crc16Modbus,
                over: (0, 6),
                at: 6,
                endian: Endian::Le,
            }),
            trailer: Trailer {
                kind: CrcKind::Crc32,
                endian: Endian::Le,
            },
            pad_payload_to: 4,
            header_template: {
                let mut t = [0; HEADER_TEMPLATE_MAX];
                t[1] = 0x01;
                t[5] = 0x01;
                t
            },
        }
    }

    /// A format unlike the two above (BE length that excludes the trailer, no header CRC, filler
    /// byte the template must carry): the builder and the reassembler stay inverses.
    fn custom_spec() -> FrameSpec {
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
    fn padding_rounds_the_payload_and_the_declared_length_covers_it() {
        let frame = build_with_spec(&routed_spec(), &[0x23, 0x01, 0x91]).unwrap();
        // Declared length covers the padded payload (4) plus the CRC-32 (4).
        assert_eq!(u16::from_le_bytes([frame[2], frame[3]]), 8);
        assert_eq!(frame.len(), 16);
        assert_eq!(frame[11], 0x00);
    }

    #[test]
    fn header_template_markers_survive_and_the_header_crc_covers_them() {
        let frame = build_with_spec(&routed_spec(), &[0x23, 0x01, 0x91, 0x01]).unwrap();
        assert_eq!(frame[1], 0x01);
        assert_eq!(frame[5], 0x01);
        assert_eq!(
            u16::from_le_bytes([frame[6], frame[7]]),
            crate::crc::crc16_modbus(&frame[..6])
        );
    }

    #[test]
    fn compact_layout_matches_spec() {
        let payload = [0x23u8, 0x07, 0x0A, 0x01];
        let frame = build_with_spec(&compact_spec(), &payload).unwrap();
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
        for spec in [compact_spec(), routed_spec()] {
            let err = build_with_spec(&spec, &big).unwrap_err();
            assert_eq!(err.code, codes::FRAME_OVERSIZED);
        }
    }

    #[test]
    fn spec_built_frames_round_trip_through_the_reassembler() {
        for (spec, payload) in [
            (compact_spec(), vec![0x28u8, 0x01, 0x00, 0x42]),
            (routed_spec(), vec![0x23u8, 0x01, 0x91]),
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
            let padded = payload.len().div_ceil(spec.pad_payload_to) * spec.pad_payload_to;
            let mut expected = payload.clone();
            expected.resize(padded, 0);
            assert_eq!(frames[0].payload, expected);
        }
    }

    #[test]
    fn a_header_longer_than_the_template_is_refused() {
        let mut spec = compact_spec();
        spec.header_len = HEADER_TEMPLATE_MAX + 1;
        let err = build_with_spec(&spec, &[0x01]).unwrap_err();
        assert_eq!(err.code, codes::FRAME_OVERSIZED);
    }
}
