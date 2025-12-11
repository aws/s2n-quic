// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::{
    reader::{storage::Chunk, Storage},
    writer,
};
use core::cmp::Ordering;

/// Implementation of [`Storage`] that delegates to a [`bytes::Buf`] implementation.
pub struct Buf<'a, B: bytes::Buf> {
    buf: &'a mut B,
    /// tracks the number of bytes that need to be advanced in the Buf
    pending: usize,
}

impl<'a, B> Buf<'a, B>
where
    B: bytes::Buf,
{
    #[inline]
    pub fn new(buf: &'a mut B) -> Self {
        Self { buf, pending: 0 }
    }

    /// Advances any pending bytes that has been read in the underlying Buf
    #[inline]
    fn commit_pending(&mut self) {
        ensure!(self.pending > 0);
        unsafe {
            assume!(self.buf.remaining() >= self.pending);
            assume!(self.buf.chunk().len() >= self.pending);
        }
        self.buf.advance(self.pending);
        self.pending = 0;
    }
}

impl<B> Storage for Buf<'_, B>
where
    B: bytes::Buf,
{
    type Error = core::convert::Infallible;

    #[inline]
    fn buffered_len(&self) -> usize {
        unsafe {
            assume!(self.buf.remaining() >= self.pending);
            assume!(self.buf.chunk().len() >= self.pending);
        }
        self.buf.remaining() - self.pending
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk<'_>, Self::Error> {
        self.commit_pending();
        let chunk = self.buf.chunk();
        let len = chunk.len().min(watermark);
        self.pending = len;
        Ok(chunk[..len].into())
    }

    #[inline]
    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk<'_>, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        self.commit_pending();

        ensure!(dest.has_remaining_capacity(), Ok(Chunk::empty()));

        loop {
            let chunk_len = self.buf.chunk().len();

            if chunk_len == 0 {
                debug_assert_eq!(
                    self.buf.remaining(),
                    0,
                    "buf returned empty chunk with remaining bytes"
                );
                return Ok(Chunk::empty());
            }

            match chunk_len.cmp(&dest.remaining_capacity()) {
                // if there's more chunks left, then copy this one out and keep going
                Ordering::Less if self.buf.remaining() > chunk_len => {
                    if Dest::SPECIALIZES_BYTES {
                        let chunk = self.buf.copy_to_bytes(chunk_len);
                        dest.put_bytes(chunk);
                    } else {
                        dest.put_slice(self.buf.chunk());
                        self.buf.advance(chunk_len);
                    }
                    continue;
                }
                Ordering::Less | Ordering::Equal => {
                    let chunk = self.buf.chunk();
                    self.pending = chunk.len();
                    return Ok(chunk.into());
                }
                Ordering::Greater => {
                    let len = dest.remaining_capacity();
                    let chunk = &self.buf.chunk()[..len];
                    self.pending = len;
                    return Ok(chunk.into());
                }
            }
        }
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        self.commit_pending();

        loop {
            let chunk = self.buf.chunk();
            let len = chunk.len().min(dest.remaining_capacity());

            ensure!(len > 0, Ok(()));

            if Dest::SPECIALIZES_BYTES {
                let chunk = self.buf.copy_to_bytes(len);
                dest.put_bytes(chunk);
            } else {
                dest.put_slice(&chunk[..len]);
                self.buf.advance(len);
            }
        }
    }
}

impl<B> Drop for Buf<'_, B>
where
    B: bytes::Buf,
{
    #[inline]
    fn drop(&mut self) {
        // make sure we advance the consumed bytes on drop
        self.commit_pending();
    }
}
