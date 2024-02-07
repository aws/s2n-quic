// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::{
        reader::{storage::Chunk, Reader, Storage},
        writer::{self, Storage as _},
    },
    varint::VarInt,
};

/// Wraps a reader and limits the amount of data that can be read from it
///
/// This can be used for applying back pressure to the reader with flow control.
pub struct Limit<'a, R: Reader + ?Sized> {
    len: usize,
    reader: &'a mut R,
}

impl<'a, R: Reader + ?Sized> Limit<'a, R> {
    #[inline]
    pub fn new(reader: &'a mut R, max_buffered_len: usize) -> Self {
        let len = max_buffered_len.min(reader.buffered_len());

        Self { len, reader }
    }
}

impl<'a, R: Reader + ?Sized> Storage for Limit<'a, R> {
    type Error = R::Error;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.len
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk, Self::Error> {
        let watermark = self.len.min(watermark);
        let chunk = self.reader.read_chunk(watermark)?;
        unsafe {
            assume!(chunk.len() <= self.len);
        }
        self.len -= chunk.len();
        Ok(chunk)
    }

    #[inline]
    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let mut dest = dest.with_write_limit(self.len);
        let mut dest = dest.track_write();
        let chunk = self.reader.partial_copy_into(&mut dest)?;
        let len = dest.written_len() + chunk.len();
        unsafe {
            assume!(len <= self.len);
        }
        self.len -= len;
        Ok(chunk)
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let mut dest = dest.with_write_limit(self.len);
        let mut dest = dest.track_write();
        self.reader.copy_into(&mut dest)?;
        let len = dest.written_len();
        unsafe {
            assume!(len <= self.len);
        }
        self.len -= len;
        Ok(())
    }
}

impl<'a, R: Reader + ?Sized> Reader for Limit<'a, R> {
    #[inline]
    fn current_offset(&self) -> VarInt {
        self.reader.current_offset()
    }

    #[inline]
    fn final_offset(&self) -> Option<VarInt> {
        self.reader.final_offset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::testing::Data;

    #[test]
    fn max_data_test() {
        let mut reader = Data::new(1000);
        assert_eq!(reader.buffered_len(), 1000);
        let mut reader = reader.with_checks();

        let max_data = 32usize;

        let mut reader = reader.with_max_data(VarInt::from_u8(max_data as _));
        assert_eq!(reader.buffered_len(), max_data);

        let mut dest = &mut [0u8; 16][..];
        let chunk = reader.partial_copy_into(&mut dest).unwrap();
        assert_eq!(chunk.len(), 16);

        assert_eq!(reader.buffered_len(), max_data - 16);

        let mut dest = &mut [0u8; 16][..];
        let chunk = reader.partial_copy_into(&mut dest).unwrap();
        assert_eq!(chunk.len(), 16);
        assert!(reader.buffer_is_empty());
    }
}
