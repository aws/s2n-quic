// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    buffer::{
        reader::{storage::Chunk, Reader, Storage},
        writer,
    },
    varint::VarInt,
};

/// Returns an empty buffer for the current offset of an inner reader
#[derive(Debug)]
pub struct Empty<'a, R: Reader + ?Sized>(&'a R);

impl<'a, R: Reader + ?Sized> Empty<'a, R> {
    #[inline]
    pub fn new(reader: &'a R) -> Self {
        Self(reader)
    }
}

impl<'a, R: Reader + ?Sized> Storage for Empty<'a, R> {
    type Error = core::convert::Infallible;

    #[inline(always)]
    fn buffered_len(&self) -> usize {
        0
    }

    #[inline(always)]
    fn read_chunk(&mut self, _watermark: usize) -> Result<Chunk, Self::Error> {
        Ok(Chunk::empty())
    }

    #[inline(always)]
    fn partial_copy_into<Dest>(&mut self, _dest: &mut Dest) -> Result<Chunk, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        Ok(Chunk::empty())
    }
}

impl<'a, R: Reader + ?Sized> Reader for Empty<'a, R> {
    #[inline]
    fn current_offset(&self) -> VarInt {
        self.0.current_offset()
    }

    #[inline]
    fn final_offset(&self) -> Option<VarInt> {
        self.0.final_offset()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::testing::Data;

    #[test]
    fn empty_test() {
        let mut reader = Data::new(1000);
        assert_eq!(reader.buffered_len(), 1000);

        {
            assert_eq!(reader.with_empty_buffer().buffered_len(), 0);
        }

        let mut dest = &mut [0u8; 16][..];
        let chunk = reader.partial_copy_into(&mut dest).unwrap();
        assert_eq!(chunk.len(), 16);

        let mut reader = reader.with_empty_buffer();

        assert_eq!(reader.buffered_len(), 0);
        assert!(reader.buffer_is_empty());

        let chunk = reader.partial_copy_into(&mut dest).unwrap();
        assert_eq!(chunk.len(), 0);
    }
}
