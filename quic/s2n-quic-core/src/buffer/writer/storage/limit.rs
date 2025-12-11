// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::{reader::storage::Chunk, writer::Storage};
use bytes::{buf::UninitSlice, Bytes, BytesMut};

/// An implementation that limits the number of bytes that can be written to the underlying storage
pub struct Limit<'a, S: Storage + ?Sized> {
    storage: &'a mut S,
    remaining_capacity: usize,
}

impl<'a, S: Storage + ?Sized> Limit<'a, S> {
    #[inline]
    pub fn new(storage: &'a mut S, remaining_capacity: usize) -> Self {
        let remaining_capacity = storage.remaining_capacity().min(remaining_capacity);
        Self {
            storage,
            remaining_capacity,
        }
    }
}

impl<S: Storage + ?Sized> Storage for Limit<'_, S> {
    const SPECIALIZES_BYTES: bool = S::SPECIALIZES_BYTES;
    const SPECIALIZES_BYTES_MUT: bool = S::SPECIALIZES_BYTES_MUT;

    #[inline]
    fn put_slice(&mut self, bytes: &[u8]) {
        debug_assert!(bytes.len() <= self.remaining_capacity);
        self.storage.put_slice(bytes);
        unsafe {
            assume!(self.remaining_capacity >= bytes.len());
        }
        self.remaining_capacity -= bytes.len();
    }

    #[inline(always)]
    fn put_uninit_slice<F, Error>(&mut self, payload_len: usize, f: F) -> Result<bool, Error>
    where
        F: FnOnce(&mut UninitSlice) -> Result<(), Error>,
    {
        debug_assert!(payload_len <= self.remaining_capacity);
        let did_write = self.storage.put_uninit_slice(payload_len, f)?;
        if did_write {
            unsafe {
                assume!(self.remaining_capacity >= payload_len);
            }
            self.remaining_capacity -= payload_len;
        }
        Ok(did_write)
    }

    #[inline]
    fn remaining_capacity(&self) -> usize {
        self.storage
            .remaining_capacity()
            .min(self.remaining_capacity)
    }

    #[inline]
    fn has_remaining_capacity(&self) -> bool {
        self.remaining_capacity > 0 && self.storage.has_remaining_capacity()
    }

    #[inline]
    fn put_bytes(&mut self, bytes: Bytes) {
        let len = bytes.len();
        debug_assert!(len <= self.remaining_capacity);
        self.storage.put_bytes(bytes);
        unsafe {
            assume!(self.remaining_capacity >= len);
        }
        self.remaining_capacity -= len;
    }

    #[inline]
    fn put_bytes_mut(&mut self, bytes: BytesMut) {
        let len = bytes.len();
        debug_assert!(len <= self.remaining_capacity);
        self.storage.put_bytes_mut(bytes);
        unsafe {
            assume!(self.remaining_capacity >= len);
        }
        self.remaining_capacity -= len;
    }

    #[inline]
    fn put_chunk(&mut self, chunk: Chunk) {
        let len = chunk.len();
        debug_assert!(len <= self.remaining_capacity);
        self.storage.put_chunk(chunk);
        unsafe {
            assume!(self.remaining_capacity >= len);
        }
        self.remaining_capacity -= len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_test() {
        let mut writer: Vec<u8> = vec![];

        {
            let mut writer = writer.with_write_limit(5);
            assert_eq!(writer.remaining_capacity(), 5);
            writer.put_slice(b"hello");
            assert_eq!(writer.remaining_capacity(), 0);
        }

        {
            let mut writer = writer.with_write_limit(5);
            assert_eq!(writer.remaining_capacity(), 5);
            writer.put_bytes(Bytes::from_static(b"hello"));
            assert_eq!(writer.remaining_capacity(), 0);
        }

        {
            let mut writer = writer.with_write_limit(5);
            assert_eq!(writer.remaining_capacity(), 5);
            writer.put_bytes_mut(BytesMut::from(&b"hello"[..]));
            assert_eq!(writer.remaining_capacity(), 0);
        }

        {
            let writer = writer.with_write_limit(0);
            assert!(!writer.has_remaining_capacity());
        }
    }
}
