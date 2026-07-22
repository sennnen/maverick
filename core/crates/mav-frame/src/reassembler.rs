//! Streaming reassembly: BLE notifications arrive as arbitrary fragments, and this turns them
//! back into validated frames. Nothing is dropped silently; garbage and CRC failures come back as
//! events so the caller can log them with their error codes.

use crate::frame::RawFrame;
use crate::spec::FrameSpec;
use mav_model::error::{codes, MavError};

const DEFAULT_MAX_FRAME_BYTES: usize = 8192;

#[derive(Clone, PartialEq, Debug)]
pub enum ReassemblyEvent {
    /// A frame whose header and payload CRCs both checked out.
    Frame(RawFrame),
    /// Bytes discarded while scanning for a start-of-frame marker.
    SkippedGarbage { bytes: usize },
    /// A start-of-frame that failed validation. One byte was consumed and scanning resumed, so a
    /// real frame that happened to contain the start byte in its body is still recovered.
    InvalidFrame(MavError),
}

pub struct Reassembler {
    /// `None` is passthrough: the wire carries no framing at all (a standard GATT characteristic
    /// value), so every pushed chunk is one complete frame.
    spec: Option<FrameSpec>,
    max_frame_bytes: usize,
    buf: Vec<u8>,
    pos: usize,
}

impl Reassembler {
    /// Reassemble any frame format described by a `FrameSpec`. Every device format arrives this
    /// way; the core names none of them (ADR-012).
    pub fn with_spec(spec: FrameSpec) -> Self {
        Self::with_spec_and_max(spec, DEFAULT_MAX_FRAME_BYTES)
    }

    pub fn with_spec_and_max(spec: FrameSpec, max_frame_bytes: usize) -> Self {
        Self {
            spec: Some(spec),
            max_frame_bytes,
            buf: Vec::new(),
            pos: 0,
        }
    }

    /// No framing: each pushed chunk is one complete frame. This is how standard GATT profiles
    /// arrive — a notification value has no start byte, no length, and no CRC (PL-P8).
    pub fn passthrough() -> Self {
        Self::passthrough_with_max(DEFAULT_MAX_FRAME_BYTES)
    }

    pub fn passthrough_with_max(max_frame_bytes: usize) -> Self {
        Self {
            spec: None,
            max_frame_bytes,
            buf: Vec::new(),
            pos: 0,
        }
    }

    /// Bytes buffered but not yet resolved into an event.
    pub fn pending(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Drop any partial frame, e.g. on disconnect. Returns how many bytes were discarded so the
    /// caller can log the loss rather than swallow it.
    pub fn reset(&mut self) -> usize {
        let discarded = self.pending();
        self.buf.clear();
        self.pos = 0;
        discarded
    }

    /// Feed one fragment and collect everything that resolves. Incomplete trailing frames stay
    /// buffered for the next call.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<ReassemblyEvent> {
        let Some(spec) = self.spec else {
            if chunk.is_empty() {
                return Vec::new();
            }
            if chunk.len() > self.max_frame_bytes {
                return vec![ReassemblyEvent::InvalidFrame(
                    MavError::new(codes::FRAME_OVERSIZED, "frame exceeds maximum size").context(
                        format!("total {}, max {}", chunk.len(), self.max_frame_bytes),
                    ),
                )];
            }
            return vec![ReassemblyEvent::Frame(RawFrame {
                payload: chunk.to_vec(),
            })];
        };

        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();

        loop {
            let skipped = self.buf[self.pos..]
                .iter()
                .take_while(|&&b| b != spec.sof)
                .count();
            if skipped > 0 {
                self.pos += skipped;
                events.push(ReassemblyEvent::SkippedGarbage { bytes: skipped });
            }

            let avail = self.buf.len() - self.pos;
            if avail < spec.header_len {
                break;
            }

            let head = &self.buf[self.pos..];
            let declared = spec.read_declared(head);

            if !spec.header_crc_ok(head) {
                events.push(ReassemblyEvent::InvalidFrame(
                    MavError::new(codes::FRAME_HEADER_CRC_MISMATCH, "header crc mismatch")
                        .context(format!("declared_len {declared}")),
                ));
                self.pos += 1;
                continue;
            }

            if declared < spec.min_declared() {
                events.push(ReassemblyEvent::InvalidFrame(
                    MavError::new(
                        codes::FRAME_TRUNCATED,
                        "declared length leaves no room for payload",
                    )
                    .context(format!("declared_len {declared}")),
                ));
                self.pos += 1;
                continue;
            }

            let total = spec.total_len(declared);
            if total > self.max_frame_bytes {
                events.push(ReassemblyEvent::InvalidFrame(
                    MavError::new(codes::FRAME_OVERSIZED, "frame exceeds maximum size")
                        .context(format!("total {total}, max {}", self.max_frame_bytes)),
                ));
                self.pos += 1;
                continue;
            }

            if avail < total {
                break;
            }

            let frame = &self.buf[self.pos..self.pos + total];
            if !spec.trailer_ok(frame, total) {
                let (start, end) = spec.payload_range(total);
                events.push(ReassemblyEvent::InvalidFrame(
                    MavError::new(codes::FRAME_PAYLOAD_CRC_MISMATCH, "payload crc mismatch")
                        .context(format!("payload_len {}", end - start)),
                ));
                self.pos += 1;
                continue;
            }

            let (start, end) = spec.payload_range(total);
            events.push(ReassemblyEvent::Frame(RawFrame {
                payload: frame[start..end].to_vec(),
            }));
            self.pos += total;
        }

        if self.pos > 0 {
            self.buf.drain(..self.pos);
            self.pos = 0;
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc::crc8;
    use crate::frame::build_with_spec;
    use crate::spec::{CrcKind, Endian, HeaderCrc, LengthField, Trailer, HEADER_TEMPLATE_MAX};

    /// 4-byte header, LE length counting the trailer, CRC-8 header check, CRC-32 trailer.
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

    /// 8-byte header with template markers, CRC-16 header check, payload padded to four.
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

    fn frames_of(events: &[ReassemblyEvent]) -> Vec<Vec<u8>> {
        events
            .iter()
            .filter_map(|e| match e {
                ReassemblyEvent::Frame(f) => Some(f.payload.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn whole_frame_in_one_push() {
        for spec in [compact_spec(), routed_spec()] {
            let wire = build_with_spec(&spec, &[0x28, 0x01, 0x00, 0x42]).unwrap();
            let mut r = Reassembler::with_spec(spec);
            let events = r.push(&wire);
            assert_eq!(frames_of(&events), vec![vec![0x28, 0x01, 0x00, 0x42]]);
            assert_eq!(r.pending(), 0);
        }
    }

    #[test]
    fn frame_split_across_single_byte_pushes() {
        let spec = routed_spec();
        let wire = build_with_spec(&spec, &[0x23, 0x01, 0x91, 0x01]).unwrap();
        let mut r = Reassembler::with_spec(spec);
        let mut collected = Vec::new();
        for &b in &wire {
            collected.extend(r.push(&[b]));
        }
        assert_eq!(frames_of(&collected), vec![vec![0x23, 0x01, 0x91, 0x01]]);
    }

    #[test]
    fn garbage_before_frame_is_reported_and_skipped() {
        let spec = compact_spec();
        let mut wire = vec![0x00, 0x13, 0x37];
        wire.extend(build_with_spec(&spec, &[0x30, 0x02, 0x05]).unwrap());
        let mut r = Reassembler::with_spec(spec);
        let events = r.push(&wire);
        assert_eq!(events[0], ReassemblyEvent::SkippedGarbage { bytes: 3 });
        assert_eq!(frames_of(&events), vec![vec![0x30, 0x02, 0x05]]);
    }

    #[test]
    fn corrupted_payload_is_rejected_and_next_frame_recovered() {
        let spec = compact_spec();
        let mut bad = build_with_spec(&spec, &[0x28, 0x01, 0x00, 0x42]).unwrap();
        let last = bad.len() - 5;
        bad[last] ^= 0x01;
        let good = build_with_spec(&spec, &[0x28, 0x02, 0x00, 0x43]).unwrap();
        let mut wire = bad;
        wire.extend(&good);

        let mut r = Reassembler::with_spec(spec);
        let events = r.push(&wire);
        let invalid: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ReassemblyEvent::InvalidFrame(err) => Some(err.code),
                _ => None,
            })
            .collect();
        assert!(
            invalid.contains(&codes::FRAME_PAYLOAD_CRC_MISMATCH),
            "{events:?}"
        );
        assert_eq!(frames_of(&events), vec![vec![0x28, 0x02, 0x00, 0x43]]);
    }

    #[test]
    fn corrupted_header_is_rejected() {
        let spec = routed_spec();
        let mut wire = build_with_spec(&spec, &[0x23, 0x01, 0x91, 0x01]).unwrap();
        wire[6] ^= 0xFF;
        let mut r = Reassembler::with_spec(spec);
        let events = r.push(&wire);
        assert!(matches!(
            events.first(),
            Some(ReassemblyEvent::InvalidFrame(err)) if err.code == codes::FRAME_HEADER_CRC_MISMATCH
        ));
        assert!(frames_of(&events).is_empty());
    }

    #[test]
    fn oversized_declared_length_is_rejected_not_awaited() {
        let mut r = Reassembler::with_spec_and_max(compact_spec(), 64);
        let mut header = vec![crate::frame::START_OF_FRAME];
        let declared = 1000u16.to_le_bytes();
        header.extend_from_slice(&declared);
        header.push(crc8(&declared));
        let events = r.push(&header);
        assert!(matches!(
            events.first(),
            Some(ReassemblyEvent::InvalidFrame(err)) if err.code == codes::FRAME_OVERSIZED
        ));
    }

    #[test]
    fn a_custom_spec_reassembles_an_unrelated_frame() {
        // 0x5A SOF, big-endian payload length, a single CRC-8 trailer, no header CRC, and the
        // length counts the payload only.
        let spec = FrameSpec {
            sof: 0x5A,
            header_len: 3,
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
            header_template: [0; HEADER_TEMPLATE_MAX],
        };
        let payload = [0xDEu8, 0xAD, 0xBE, 0xEF];
        let mut wire = vec![0x5A];
        wire.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        wire.extend_from_slice(&payload);
        wire.push(crc8(&payload));

        let mut r = Reassembler::with_spec(spec);
        let events = r.push(&wire);
        assert_eq!(frames_of(&events), vec![payload.to_vec()]);
    }

    #[test]
    fn passthrough_delivers_each_chunk_whole() {
        let mut r = Reassembler::passthrough();
        let events = r.push(&[0x10, 0x40, 0x33, 0x03]);
        assert_eq!(frames_of(&events), vec![vec![0x10, 0x40, 0x33, 0x03]]);
    }
}
