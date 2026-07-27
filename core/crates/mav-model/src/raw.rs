//! Decode output: samples as the wire produced them, before signal quality has judged them and
//! before any provenance row exists. `seq` is the per-second occurrence index the decoder assigns
//! so that equal values landing in the same second stay distinguishable all the way to the dedup
//! key (the RR lesson in docs/pipeline.md).

use crate::ids::DeviceId;
use crate::stream::StreamKind;
use crate::time::DeviceTime;
use serde::{Deserialize, Serialize};

/// A raw field value in its wire-native width. Kept typed rather than widened to f64 so that a
/// fixture can assert exactly what was read, and so a lossy conversion is visible where it
/// happens. `Converted` is the one exception: a manifest unit conversion (scale and offset)
/// produces a physical value that no longer has a wire width.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RawValue {
    U8(u8),
    U16(u16),
    U32(u32),
    I16(i16),
    I32(i32),
    F32(f32),
    Converted(f64),
}

impl RawValue {
    pub fn as_f64(self) -> f64 {
        match self {
            RawValue::U8(v) => f64::from(v),
            RawValue::U16(v) => f64::from(v),
            RawValue::U32(v) => f64::from(v),
            RawValue::I16(v) => f64::from(v),
            RawValue::I32(v) => f64::from(v),
            RawValue::F32(v) => f64::from(v),
            RawValue::Converted(v) => v,
        }
    }

    /// A stable bit pattern for dedup keys, so two samples with equal values key equally and a
    /// float value never trips over NaN-inequality. Paired with [`RawValue::tag`] it is also the
    /// store's lossless encoding: two integers instead of a JSON string in a primary key.
    pub const fn key_bits(self) -> u64 {
        match self {
            RawValue::U8(v) => v as u64,
            RawValue::U16(v) => v as u64,
            RawValue::U32(v) => v as u64,
            RawValue::I16(v) => v as u16 as u64,
            RawValue::I32(v) => v as u32 as u64,
            RawValue::F32(v) => v.to_bits() as u64,
            RawValue::Converted(v) => v.to_bits(),
        }
    }

    /// Which width the bits are to be read back as. Frozen: the pair is on disk.
    pub const fn tag(self) -> u8 {
        match self {
            RawValue::U8(_) => 0,
            RawValue::U16(_) => 1,
            RawValue::U32(_) => 2,
            RawValue::I16(_) => 3,
            RawValue::I32(_) => 4,
            RawValue::F32(_) => 5,
            RawValue::Converted(_) => 6,
        }
    }

    pub const fn from_parts(tag: u8, bits: u64) -> Option<Self> {
        Some(match tag {
            0 => RawValue::U8(bits as u8),
            1 => RawValue::U16(bits as u16),
            2 => RawValue::U32(bits as u32),
            3 => RawValue::I16(bits as u16 as i16),
            4 => RawValue::I32(bits as u32 as i32),
            5 => RawValue::F32(f32::from_bits(bits as u32)),
            6 => RawValue::Converted(f64::from_bits(bits)),
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct RawSample {
    pub kind: StreamKind,
    pub device_time: DeviceTime,
    pub seq: u16,
    pub value: RawValue,
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct RawSampleBatch {
    pub device: DeviceId,
    pub samples: Vec<RawSample>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_f64_widens_every_variant() {
        assert_eq!(RawValue::U8(62).as_f64(), 62.0);
        assert_eq!(RawValue::I16(-2).as_f64(), -2.0);
        assert_eq!(RawValue::Converted(36.5).as_f64(), 36.5);
    }

    #[test]
    fn key_bits_distinguishes_type_but_equal_values_key_equal() {
        assert_eq!(RawValue::U16(830).key_bits(), RawValue::U16(830).key_bits());
        assert_ne!(RawValue::F32(1.0).key_bits(), RawValue::F32(1.5).key_bits());
    }

    #[test]
    fn key_bits_handles_nan_stably() {
        let nan = RawValue::F32(f32::NAN);
        assert_eq!(nan.key_bits(), nan.key_bits());
    }

    /// The store keeps values as `(tag, bits)` so the sample table's key is all integers. The pair
    /// has to be lossless for every width, including the sign-extending and float ones.
    #[test]
    fn every_width_survives_the_two_integer_encoding() {
        for value in [
            RawValue::U8(u8::MAX),
            RawValue::U16(812),
            RawValue::U32(u32::MAX),
            RawValue::I16(i16::MIN),
            RawValue::I32(-1),
            RawValue::F32(-1.5),
            RawValue::Converted(36.5),
        ] {
            assert_eq!(
                RawValue::from_parts(value.tag(), value.key_bits()),
                Some(value)
            );
        }
        assert_eq!(RawValue::from_parts(7, 0), None);
    }

    #[test]
    fn batch_roundtrips_through_json() {
        let batch = RawSampleBatch {
            device: DeviceId::new(1),
            samples: vec![RawSample {
                kind: StreamKind::HeartRate,
                device_time: DeviceTime::from_nanos(5_000_000_000),
                seq: 0,
                value: RawValue::U8(71),
            }],
        };
        let json = serde_json::to_string(&batch).unwrap();
        assert_eq!(
            serde_json::from_str::<RawSampleBatch>(&json).unwrap(),
            batch
        );
    }
}
