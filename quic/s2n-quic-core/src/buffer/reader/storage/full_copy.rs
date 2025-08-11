// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::{
    reader::{storage::Chunk, Storage},
    writer,
};

/// Forces a full copy, even when `partial_copy_into` is called
#[derive(Debug)]
pub struct FullCopy<'a, S: Storage + ?Sized>(&'a mut S);

impl<'a, S: Storage + ?Sized> FullCopy<'a, S> {
    #[inline]
    pub fn new(storage: &'a mut S) -> Self {
        Self(storage)
    }
}

impl<S: Storage + ?Sized> Storage for FullCopy<'_, S> {
    type Error = S::Error;

    #[inline]
    fn buffered_len(&self) -> usize {
        self.0.buffered_len()
    }

    #[inline]
    fn buffer_is_empty(&self) -> bool {
        self.0.buffer_is_empty()
    }

    #[inline]
    fn read_chunk(&mut self, watermark: usize) -> Result<Chunk<'_>, Self::Error> {
        self.0.read_chunk(watermark)
    }

    #[inline]
    fn partial_copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<Chunk<'_>, Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        // force the full copy
        self.0.copy_into(dest)?;
        Ok(Chunk::empty())
    }

    #[inline]
    fn copy_into<Dest>(&mut self, dest: &mut Dest) -> Result<(), Self::Error>
    where
        Dest: writer::Storage + ?Sized,
    {
        self.0.copy_into(dest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_copy_test() {
        let mut reader = &b"hello world"[..];
        let len = reader.len();
        let mut reader = reader.full_copy();
        let mut writer: Vec<u8> = vec![];

        let chunk = reader.partial_copy_into(&mut writer).unwrap();
        assert_eq!(chunk.len(), 0);
        assert_eq!(writer.len(), len);
    }
}
