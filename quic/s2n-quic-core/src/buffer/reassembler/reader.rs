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
        let mut prev = BytesMut::new();

        loop {
            let remaining = dest.remaining_capacity();
            // ensure we have enough capacity in the destination buf
            ensure!(remaining > 0, Ok(Default::default()));

            match self.pop_watermarked(remaining) {
                Some(chunk) => {
                    let mut prev = core::mem::replace(&mut prev, chunk);
                    if !prev.is_empty() {
                        prev.infallible_copy_into(dest);
                    }
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
            let remaining = dest.remaining_capacity();
            // ensure we have enough capacity in the destination buf
            ensure!(remaining > 0, Ok(()));

            let transform = |buffer: &mut BytesMut, _is_final_offset| {
                let len = buffer.len().min(remaining);
                buffer.infallible_copy_into(dest);
                ((), len)
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
            VarInt::new_unchecked(self.start_offset)
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
