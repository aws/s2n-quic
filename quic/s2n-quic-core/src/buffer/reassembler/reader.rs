// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Reassembler;
use crate::{
    buffer::{
        reader::{
            storage::{Chunk, Infallible},
            Reader, Storage,
        },
        writer,
    },
    varint::VarInt,
};
use bytes::BytesMut;

impl Storage for Reassembler {
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.len()
    }

    #[inline]
    fn buffer_is_empty(&self) -> bool {
        self.is_empty()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk, Self::Error> {
        if let Some(chunk) = self.pop_watermarked(watermark) {
            return Ok(chunk.into());
        }

        Ok(Default::default())
    }

    #[inline]
    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        // ensure we have enough capacity in the destination buf
        ensure!(dest.has_remaining_capacity(), Ok(Default::default()));

        let mut prev = BytesMut::new();

        loop {
            let remaining = dest.remaining_capacity();
            unsafe {
                assume!(prev.len() <= remaining);
            }
            let watermark = remaining - prev.len();

            debug_assert!(remaining > 0);

            match self.pop_watermarked(watermark) {
                Some(chunk) => {
                    debug_assert!(!chunk.is_empty(), "pop should never return an empty chunk");
                    debug_assert!(
                        chunk.len() <= watermark,
                        "chunk should never exceed watermark"
                    );

                    // flush the previous chunk if needed
                    if !prev.is_empty() {
                        dest.put_bytes_mut(prev);
                    }

                    // if the chunk is exactly the same size as the watermark, then return it
                    if chunk.len() == watermark {
                        return Ok(chunk.into());
                    }

                    // store the chunk for another iteration, in case we can pull more
                    prev = chunk;
                }
                None if prev.is_empty() => {
                    return Ok(Default::default());
                }
                None => {
                    return Ok(prev.into());
                }
            }
        }
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        loop {
            // ensure we have enough capacity in the destination buf
            ensure!(dest.has_remaining_capacity(), Ok(()));

            let transform = |buffer: &mut BytesMut, _is_final_offset| {
                let mut dest = dest.track_write();
                buffer.infallible_copy_into(&mut dest);
                ((), dest.written_len())
            };

            if self.pop_transform(transform).is_none() {
                return Ok(());
            }
        }
    }
}

impl Reader for Reassembler {
    #[inline]
    fn current_offset(&self) -> VarInt {
        unsafe {
            // SAFETY: offset will always fit into a VarInt
            VarInt::new_unchecked(self.cursors.start_offset)
        }
    }

    #[inline]
    fn final_offset(&self) -> Option<VarInt> {
        self.final_size().map(|v| unsafe {
            // SAFETY: offset will always fit into a VarInt
            VarInt::new_unchecked(v)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undersized_dest_partial_copy_into_test() {
        let mut reassembler = Reassembler::default();

        reassembler.write_at(VarInt::ZERO, b"hello").unwrap();

        let mut dest = &mut [0u8; 1][..];
        let chunk = reassembler.infallible_partial_copy_into(&mut dest);
        assert_eq!(dest.len(), 1, "the destination should not be written into");
        assert_eq!(&chunk[..], b"h");

        assert_eq!(reassembler.current_offset().as_u64(), 1);
    }

    #[test]
    fn oversized_dest_partial_copy_into_test() {
        let mut reassembler = Reassembler::default();

        reassembler.write_at(VarInt::ZERO, b"hello").unwrap();

        let mut reader = reassembler.with_checks();

        let mut dest = &mut [0u8; 10][..];
        let chunk = reader.infallible_partial_copy_into(&mut dest);
        assert_eq!(dest.len(), 10, "the destination should not be written into");
        assert_eq!(&chunk[..], b"hello");

        assert_eq!(reader.current_offset().as_u64(), 5);
    }

    #[test]
    fn multiple_chunk_dest_partial_copy_into_test() {
        let mut reassembler = Reassembler::default();

        // align the cursor to just before a slot boundary
        let offset: VarInt = (super::super::MIN_BUFFER_ALLOCATION_SIZE - 1)
            .try_into()
            .unwrap();
        reassembler.skip(offset).unwrap();
        reassembler.write_at(offset, b"hello").unwrap();

        let mut reader = reassembler.with_checks();
        let mut dest = [0u8; 10];

        let chunk = {
            let mut dest = &mut dest[..];
            let chunk = reader.infallible_partial_copy_into(&mut dest);
            assert_eq!(
                dest.len(),
                9,
                "the destination should have a single byte written to it"
            );
            chunk
        };

        assert_eq!(&dest[..1], b"h");
        assert_eq!(&chunk[..], b"ello");
    }
}
