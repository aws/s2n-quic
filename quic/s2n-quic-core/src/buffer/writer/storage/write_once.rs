// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::{reader::storage::Chunk, writer::Storage};
use bytes::{buf::UninitSlice, Bytes, BytesMut};

/// Only allows a single write into the storage. After that, no more writes are allowed.
///
/// This can be used for very low latency scenarios where processing the single read is more
/// important than filling the entire storage with as much data as possible.
pub struct WriteOnce<'a, S: Storage + ?Sized> {
    storage: &'a mut S,
    did_write: bool,
}

impl<'a, S: Storage + ?Sized> WriteOnce<'a, S> {
    #[inline]
    pub fn new(storage: &'a mut S) -> Self {
        Self {
            storage,
            did_write: false,
        }
    }
}

impl<S: Storage + ?Sized> Storage for WriteOnce<'_, S> {
    const SPECIALIZES_BYTES: bool = S::SPECIALIZES_BYTES;
    const SPECIALIZES_BYTES_MUT: bool = S::SPECIALIZES_BYTES_MUT;

    #[inline]
    fn put_slice(&mut self, bytes: &[u8]) {
        let did_write = !bytes.is_empty();
        self.storage.put_slice(bytes);
        self.did_write |= did_write;
    }

    #[inline(always)]
    fn put_uninit_slice<F, Error>(&mut self, payload_len: usize, f: F) -> Result<bool, Error>
    where
        F: FnOnce(&mut UninitSlice) -> Result<(), Error>,
    {
        let did_write = self.storage.put_uninit_slice(payload_len, f)?;
        self.did_write |= did_write && payload_len > 0;
        Ok(did_write)
    }

    #[inline]
    fn remaining_capacity(&self) -> usize {
        ensure!(!self.did_write, 0);
        self.storage.remaining_capacity()
    }

    #[inline]
    fn has_remaining_capacity(&self) -> bool {
        ensure!(!self.did_write, false);
        self.storage.has_remaining_capacity()
    }

    #[inline]
    fn put_bytes(&mut self, bytes: Bytes) {
        let did_write = !bytes.is_empty();
        self.storage.put_bytes(bytes);
        self.did_write |= did_write;
    }

    #[inline]
    fn put_bytes_mut(&mut self, bytes: BytesMut) {
        let did_write = !bytes.is_empty();
        self.storage.put_bytes_mut(bytes);
        self.did_write |= did_write;
    }

    #[inline]
    fn put_chunk(&mut self, chunk: Chunk) {
        let did_write = !chunk.is_empty();
        self.storage.put_chunk(chunk);
        self.did_write |= did_write;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_once_test() {
        let mut writer: Vec<u8> = vec![];

        {
            let mut writer = writer.write_once();
            assert!(writer.has_remaining_capacity());
            writer.put_slice(b"hello");
            assert_eq!(writer.remaining_capacity(), 0);
            assert!(!writer.has_remaining_capacity());
        }

        {
            let mut writer = writer.write_once();
            assert!(writer.has_remaining_capacity());
            writer.put_chunk(b"hello"[..].into());
            assert_eq!(writer.remaining_capacity(), 0);
            assert!(!writer.has_remaining_capacity());
        }

        {
            let mut writer = writer.write_once();
            assert!(writer.has_remaining_capacity());
            let did_write = writer
                .put_uninit_slice(5, |slice| {
                    slice.copy_from_slice(b"hello");
                    <Result<(), core::convert::Infallible>>::Ok(())
                })
                .unwrap();
            assert!(did_write);
            assert_eq!(writer.remaining_capacity(), 0);
            assert!(!writer.has_remaining_capacity());
        }

        {
            let mut writer = writer.write_once();
            assert!(writer.has_remaining_capacity());
            writer.put_bytes(Bytes::from_static(b"hello"));
            assert_eq!(writer.remaining_capacity(), 0);
            assert!(!writer.has_remaining_capacity());
        }

        {
            let mut writer = writer.write_once();
            assert!(writer.has_remaining_capacity());
            writer.put_bytes_mut(BytesMut::from(&b"hello"[..]));
            assert_eq!(writer.remaining_capacity(), 0);
            assert!(!writer.has_remaining_capacity());
        }
    }

    // ensures a reader that only reads capacity at the beginning can still write multiple chunks
    #[test]
    fn copy_into_multi_chunks() {
        let mut writer: Vec<u8> = vec![];
        {
            let mut writer = writer.write_once();

            assert!(writer.has_remaining_capacity());
            writer.put_slice(b"hello");
            assert!(!writer.has_remaining_capacity());
            writer.put_slice(b"world");
            assert!(!writer.has_remaining_capacity());
        }

        assert_eq!(&writer[..], b"helloworld");
    }
}
