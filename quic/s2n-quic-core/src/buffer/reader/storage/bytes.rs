// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::{
    reader::{storage::Chunk, Storage},
    writer,
};
use bytes::{Bytes, BytesMut};

impl Storage for BytesMut {
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.len()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk, Self::Error> {
        let len = self.len().min(watermark);
        Ok(self.split_to(len).into())
    }

    #[inline]
    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        self.read_chunk(dest.remaining_capacity())
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let watermark = self.len().min(dest.remaining_capacity());

        if Dest::SPECIALIZES_BYTES_MUT {
            let buffer = self.split_to(watermark);
            dest.put_bytes_mut(buffer);
        } else if Dest::SPECIALIZES_BYTES {
            let buffer = self.split_to(watermark);
            dest.put_bytes(buffer.freeze());
        } else {
            // copy bytes into the destination buf
            dest.put_slice(&self[..watermark]);
            // advance the chunk rather than splitting to avoid refcount churn
            bytes::Buf::advance(self, watermark)
        }

        Ok(())
    }
}

impl Storage for Bytes {
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.len()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk, Self::Error> {
        let len = self.len().min(watermark);
        Ok(self.split_to(len).into())
    }

    #[inline]
    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        self.read_chunk(dest.remaining_capacity())
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        let watermark = self.len().min(dest.remaining_capacity());

        if Dest::SPECIALIZES_BYTES {
            let buffer = self.split_to(watermark);
            dest.put_bytes(buffer);
        } else {
            // copy bytes into the destination buf
            dest.put_slice(&self[..watermark]);
            // advance the chunk rather than splitting to avoid refcount churn
            bytes::Buf::advance(self, watermark)
        }

        Ok(())
    }
}
