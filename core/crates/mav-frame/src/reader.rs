//! A bounds-checked little-endian reader over a frame payload. Decoders use this instead of
//! indexing so that a short or malformed payload becomes a typed error with the offset in it,
//! never a panic. Absolute reads exist because the record layouts are documented as fixed offsets.

use mav_model::error::{codes, MavError, Result};

pub struct TypedReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> TypedReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn out_of_bounds(&self, offset: usize, wanted: usize) -> MavError {
        MavError::new(
            codes::FRAME_READER_OUT_OF_BOUNDS,
            "read past end of payload",
        )
        .context(format!(
            "offset {offset}, wanted {wanted}, len {}",
            self.data.len()
        ))
    }

    fn take_at<const N: usize>(&self, offset: usize) -> Result<[u8; N]> {
        let end = offset
            .checked_add(N)
            .ok_or_else(|| self.out_of_bounds(offset, N))?;
        let slice = self
            .data
            .get(offset..end)
            .ok_or_else(|| self.out_of_bounds(offset, N))?;
        let mut out = [0u8; N];
        out.copy_from_slice(slice);
        Ok(out)
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        let bytes = self.take_at::<N>(self.pos)?;
        self.pos += N;
        Ok(bytes)
    }

    pub fn seek(&mut self, offset: usize) -> Result<()> {
        if offset > self.data.len() {
            return Err(self.out_of_bounds(offset, 0));
        }
        self.pos = offset;
        Ok(())
    }

    pub fn skip(&mut self, count: usize) -> Result<()> {
        let target = self
            .pos
            .checked_add(count)
            .ok_or_else(|| self.out_of_bounds(self.pos, count))?;
        self.seek(target)
    }

    pub fn bytes(&mut self, count: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(count)
            .ok_or_else(|| self.out_of_bounds(self.pos, count))?;
        let slice = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| self.out_of_bounds(self.pos, count))?;
        self.pos = end;
        Ok(slice)
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take::<1>()?[0])
    }

    pub fn u16_le(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take::<2>()?))
    }

    pub fn u32_le(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take::<4>()?))
    }

    pub fn i16_le(&mut self) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take::<2>()?))
    }

    pub fn i32_le(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.take::<4>()?))
    }

    pub fn f32_le(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take::<4>()?))
    }

    pub fn u8_at(&self, offset: usize) -> Result<u8> {
        Ok(self.take_at::<1>(offset)?[0])
    }

    pub fn u16_le_at(&self, offset: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take_at::<2>(offset)?))
    }

    pub fn u32_le_at(&self, offset: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take_at::<4>(offset)?))
    }

    pub fn i16_le_at(&self, offset: usize) -> Result<i16> {
        Ok(i16::from_le_bytes(self.take_at::<2>(offset)?))
    }

    pub fn f32_le_at(&self, offset: usize) -> Result<f32> {
        Ok(f32::from_le_bytes(self.take_at::<4>(offset)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sequential_reads_advance_the_cursor() {
        let data = [0x28, 0x01, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12];
        let mut r = TypedReader::new(&data);
        assert_eq!(r.u8().unwrap(), 0x28);
        assert_eq!(r.u8().unwrap(), 0x01);
        assert_eq!(r.u16_le().unwrap(), 0x1234);
        assert_eq!(r.u32_le().unwrap(), 0x1234_5678);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn absolute_reads_do_not_move_the_cursor() {
        let data = [0x00, 0x00, 0xE8, 0x03];
        let r = TypedReader::new(&data);
        assert_eq!(r.u16_le_at(2).unwrap(), 1000);
    }

    #[test]
    fn signed_and_float_reads() {
        let mut bytes = (-2i16).to_le_bytes().to_vec();
        bytes.extend(1.5f32.to_le_bytes());
        let mut r = TypedReader::new(&bytes);
        assert_eq!(r.i16_le().unwrap(), -2);
        assert_eq!(r.f32_le().unwrap(), 1.5);
    }

    #[test]
    fn out_of_bounds_is_a_typed_error_with_the_offset() {
        let data = [0x01, 0x02];
        let mut r = TypedReader::new(&data);
        r.skip(1).unwrap();
        let err = r.u32_le().unwrap_err();
        assert_eq!(err.code, codes::FRAME_READER_OUT_OF_BOUNDS);
        assert!(err.context.iter().any(|c| c.contains("offset 1")), "{err}");
        assert_eq!(r.position(), 1);
    }

    #[test]
    fn seek_past_end_is_refused_but_seek_to_end_is_fine() {
        let data = [0x01, 0x02, 0x03];
        let mut r = TypedReader::new(&data);
        assert!(r.seek(3).is_ok());
        assert!(r.seek(4).is_err());
    }

    #[test]
    fn bytes_returns_the_exact_slice() {
        let data = [0x0A, 0x0B, 0x0C, 0x0D];
        let mut r = TypedReader::new(&data);
        r.skip(1).unwrap();
        assert_eq!(r.bytes(2).unwrap(), &[0x0B, 0x0C]);
        assert_eq!(r.position(), 3);
        assert!(r.bytes(2).is_err());
    }
}
