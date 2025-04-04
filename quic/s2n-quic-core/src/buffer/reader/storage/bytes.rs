// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::{
    reader::{
        storage::{Chunk, Infallible as _},
        Storage,
    },
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

        ensure!(len > 0, Ok(Self::new().into()));

        // if this is reading the entire thing then swap `self` for an empty value
        if self.capacity() == len {
            Ok(core::mem::replace(self, Self::new()).into())
        } else {
            Ok(self.split_to(len).into())
        }
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

        let consumes_self = watermark == self.len();

        if Dest::SPECIALIZES_BYTES_MUT || consumes_self {
            let Chunk::BytesMut(chunk) = self.infallible_read_chunk(watermark) else {
                unsafe { assume!(false) }
            };
            dest.put_bytes_mut(chunk);
        } else if Dest::SPECIALIZES_BYTES {
            let Chunk::BytesMut(chunk) = self.infallible_read_chunk(watermark) else {
                unsafe { assume!(false) }
            };
            dest.put_bytes(chunk.freeze());
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

        let consumes_self = watermark == self.len();

        if Dest::SPECIALIZES_BYTES || consumes_self {
            let Chunk::Bytes(chunk) = self.infallible_read_chunk(watermark) else {
                unsafe { assume!(false) }
            };
            dest.put_bytes(chunk);
        } else {
            // copy bytes into the destination buf
            dest.put_slice(&self[..watermark]);
            // advance the chunk rather than splitting to avoid refcount churn
            bytes::Buf::advance(self, watermark)
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use writer::Storage as _;

    #[test]
    fn bytes_into_queue_test() {
        let mut reader = Bytes::from_static(b"hello world");

        let mut writer: Vec<Bytes> = vec![];
        {
            let mut writer = writer.with_write_limit(5);
            let chunk = reader.partial_copy_into(&mut writer).unwrap();
            assert_eq!(&chunk[..], b"hello");
        }

        assert!(writer.is_empty());
        assert_eq!(&reader[..], b" world");

        reader.copy_into(&mut writer).unwrap();

        assert_eq!(writer.len(), 1);
        assert_eq!(&writer.pop().unwrap()[..], b" world");
    }
}
