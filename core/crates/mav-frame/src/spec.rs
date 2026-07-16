//! A frame format described as data. The reassembler is driven by a `FrameSpec` rather than a
//! hardcoded pair of formats, so a device declares its framing (start-of-frame byte, length field,
//! header and trailer CRCs, padding) instead of the core knowing it. gen4 and gen5 are two presets
//! expressed in exactly this form; a connector can supply a third. The rationale is ADR-012.
//!
//! This describes how to *validate and split* incoming frames. Building outgoing command frames is
//! a separate, WHOOP-only concern that stays in `frame.rs`.

use crate::crc::{crc16_modbus, crc32, crc8};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Endian {
    Le,
    Be,
}

impl Endian {
    fn read(self, bytes: &[u8]) -> u64 {
        let mut value = 0u64;
        match self {
            Endian::Le => {
                for (i, &b) in bytes.iter().enumerate() {
                    value |= u64::from(b) << (8 * i);
                }
            }
            Endian::Be => {
                for &b in bytes {
                    value = (value << 8) | u64::from(b);
                }
            }
        }
        value
    }
}

/// The CRC algorithms the wire uses, with their byte widths.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrcKind {
    Crc8,
    Crc16Modbus,
    Crc32,
}

impl CrcKind {
    pub const fn width(self) -> usize {
        match self {
            CrcKind::Crc8 => 1,
            CrcKind::Crc16Modbus => 2,
            CrcKind::Crc32 => 4,
        }
    }

    pub fn compute(self, data: &[u8]) -> u64 {
        match self {
            CrcKind::Crc8 => u64::from(crc8(data)),
            CrcKind::Crc16Modbus => u64::from(crc16_modbus(data)),
            CrcKind::Crc32 => u64::from(crc32(data)),
        }
    }
}

/// Where and how the declared length sits in the header.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LengthField {
    pub offset: usize,
    pub width: usize,
    pub endian: Endian,
}

/// A CRC over a fixed byte range of the header, stored at a fixed offset in the header.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HeaderCrc {
    pub kind: CrcKind,
    /// Byte range the CRC covers, `[start, end)`, relative to the frame start.
    pub over: (usize, usize),
    /// Offset the stored CRC sits at, relative to the frame start.
    pub at: usize,
    pub endian: Endian,
}

/// The trailing CRC, computed over the payload and stored in the last bytes of the frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trailer {
    pub kind: CrcKind,
    pub endian: Endian,
}

/// A complete description of one frame format, enough to validate and split incoming frames.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrameSpec {
    pub sof: u8,
    pub header_len: usize,
    pub length: LengthField,
    /// True when the declared length counts the trailer (gen4/gen5, whose length includes the
    /// CRC-32); false when it counts the payload only (the mock).
    pub length_includes_trailer: bool,
    pub header_crc: Option<HeaderCrc>,
    pub trailer: Trailer,
}

impl FrameSpec {
    /// WHOOP 4.0: `[0xAA][len u16 LE][crc8(len)]` then payload and a trailing CRC-32. The declared
    /// length includes the CRC-32.
    pub const fn gen4() -> Self {
        Self {
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
        }
    }

    /// WHOOP 5.0 / MG: `[0xAA][0x01][len u16 LE][0x00 0x01][crc16 LE]` then a 4-byte-padded payload
    /// and a trailing CRC-32. The declared length includes the CRC-32.
    pub const fn gen5() -> Self {
        Self {
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
        }
    }

    /// The smallest declared length that leaves room for the payload. When the length counts the
    /// trailer, it must be at least the trailer width; when it counts the payload only, zero is fine.
    pub const fn min_declared(&self) -> usize {
        if self.length_includes_trailer {
            self.trailer.kind.width()
        } else {
            0
        }
    }

    /// Total frame length for a given declared length.
    pub const fn total_len(&self, declared: usize) -> usize {
        if self.length_includes_trailer {
            self.header_len + declared
        } else {
            self.header_len + declared + self.trailer.kind.width()
        }
    }

    /// Read the declared length out of a buffer that holds at least `header_len` bytes.
    pub fn read_declared(&self, head: &[u8]) -> usize {
        let start = self.length.offset;
        let end = start + self.length.width;
        self.length.endian.read(&head[start..end]) as usize
    }

    /// Whether the header CRC (if any) checks out for a buffer holding at least `header_len` bytes.
    pub fn header_crc_ok(&self, head: &[u8]) -> bool {
        match self.header_crc {
            None => true,
            Some(crc) => {
                let computed = crc.kind.compute(&head[crc.over.0..crc.over.1]);
                let stored = crc.endian.read(&head[crc.at..crc.at + crc.kind.width()]);
                computed == stored
            }
        }
    }

    /// The payload's byte range within a complete frame of `total` bytes.
    pub const fn payload_range(&self, total: usize) -> (usize, usize) {
        (self.header_len, total - self.trailer.kind.width())
    }

    /// Whether the trailing CRC checks out for a complete frame.
    pub fn trailer_ok(&self, frame: &[u8], total: usize) -> bool {
        let (start, end) = self.payload_range(total);
        let payload = &frame[start..end];
        let computed = self.trailer.kind.compute(payload);
        let stored = self.trailer.endian.read(&frame[end..total]);
        computed == stored
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endian_reads_both_ways() {
        assert_eq!(Endian::Le.read(&[0x34, 0x12]), 0x1234);
        assert_eq!(Endian::Be.read(&[0x12, 0x34]), 0x1234);
    }

    #[test]
    fn gen4_total_and_payload_range() {
        let spec = FrameSpec::gen4();
        // declared 8 = 4-byte payload + 4-byte crc32; total = 12; payload = [4, 8).
        assert_eq!(spec.total_len(8), 12);
        assert_eq!(spec.payload_range(12), (4, 8));
        assert_eq!(spec.min_declared(), 4);
    }

    #[test]
    fn gen5_total_and_payload_range() {
        let spec = FrameSpec::gen5();
        assert_eq!(spec.total_len(8), 16);
        assert_eq!(spec.payload_range(16), (8, 12));
    }
}
