//! The three checksums the WHOOP wire uses: CRC-8 (poly 0x07) over gen4 header length bytes,
//! CRC-16/Modbus over the gen5 header, and zlib CRC-32 over every frame payload. Bitwise
//! implementations, pinned by the standard check value for "123456789" and by a real captured
//! gen5 hello frame in frame.rs.

pub fn crc8(data: &[u8]) -> u8 {
    let mut crc = 0u8;
    for &byte in data {
        crc ^= byte;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 {
                (crc << 1) ^ 0x07
            } else {
                crc << 1
            };
        }
    }
    crc
}

pub fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc = 0xFFFFu16;
    for &byte in data {
        crc ^= u16::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHECK: &[u8] = b"123456789";

    #[test]
    fn crc8_matches_standard_check_value() {
        assert_eq!(crc8(CHECK), 0xF4);
    }

    #[test]
    fn crc16_modbus_matches_standard_check_value() {
        assert_eq!(crc16_modbus(CHECK), 0x4B37);
    }

    #[test]
    fn crc32_matches_standard_check_value() {
        assert_eq!(crc32(CHECK), 0xCBF4_3926);
    }

    #[test]
    fn empty_input_yields_initial_state() {
        assert_eq!(crc8(&[]), 0x00);
        assert_eq!(crc16_modbus(&[]), 0xFFFF);
        assert_eq!(crc32(&[]), 0x0000_0000);
    }
}
