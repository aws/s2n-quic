// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::{reader::storage::Chunk, writer::Storage};
use bytes::{buf::UninitSlice, Bytes, BytesMut};

/// Tracks the number of bytes written to the underlying storage
pub struct Tracked<'a, S: Storage + ?Sized> {
    storage: &'a mut S,
    written: usize,
}

impl<'a, S: Storage + ?Sized> Tracked<'a, S> {
    #[inline]
    pub fn new(storage: &'a mut S) -> Self {
        Self {
            storage,
            written: 0,
        }
    }

    /// Returns the number of bytes written to the underlying storage
    #[inline]
    pub fn written_len(&self) -> usize {
        self.written
    }
}

impl<'a, S: Storage + ?Sized> Storage for Tracked<'a, S> {
    const SPECIALIZES_BYTES: bool = S::SPECIALIZES_BYTES;
    const SPECIALIZES_BYTES_MUT: bool = S::SPECIALIZES_BYTES_MUT;

    #[inline]
    fn put_slice(&mut self, bytes: &[u8]) {
        self.storage.put_slice(bytes);
        self.written += bytes.len();
    }

    #[inline(always)]
    fn put_uninit_slice<F, Error>(&mut self, payload_len: usize, f: F) -> Result<bool, Error>
    where
        F: FnOnce(&mut UninitSlice) -> Result<(), Error>,
    {
        let did_write = self.storage.put_uninit_slice(payload_len, f)?;
        if did_write {
            self.written += payload_len;
        }
        Ok(did_write)
    }

    #[inline]
    fn remaining_capacity(&self) -> usize {
        self.storage.remaining_capacity()
    }

    #[inline]
    fn has_remaining_capacity(&self) -> bool {
        self.storage.has_remaining_capacity()
    }

    #[inline]
    fn put_bytes(&mut self, bytes: Bytes) {
        let len = bytes.len();
        self.storage.put_bytes(bytes);
        self.written += len;
    }

    #[inline]
    fn put_bytes_mut(&mut self, bytes: BytesMut) {
        let len = bytes.len();
        self.storage.put_bytes_mut(bytes);
        self.written += len;
    }

    #[inline]
    fn put_chunk(&mut self, chunk: Chunk) {
        let len = chunk.len();
        self.storage.put_chunk(chunk);
        self.written += len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracked_test() {
        let mut writer: Vec<u8> = vec![];

        {
            let mut writer = writer.tracked();
            assert_eq!(writer.written_len(), 0);
            writer.put_slice(b"hello");
            assert_eq!(writer.written_len(), 5);
        }

        {
            let mut writer = writer.tracked();
            assert_eq!(writer.written_len(), 0);
            writer.put_bytes(Bytes::from_static(b"hello"));
            assert_eq!(writer.written_len(), 5);
        }

        {
            let mut writer = writer.tracked();
            assert_eq!(writer.written_len(), 0);
            writer.put_bytes_mut(BytesMut::from(&b"hello"[..]));
            assert_eq!(writer.written_len(), 5);
        }
    }
}
