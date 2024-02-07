// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::buffer::reader::storage::Chunk;
use bytes::{Bytes, BytesMut};

mod buf;
mod byte_queue;
mod discard;
mod empty;
mod limit;
mod tracked;
mod uninit_slice;
mod write_once;

pub use buf::BufMut;
pub use bytes::buf::UninitSlice;
pub use discard::Discard;
pub use empty::Empty;
pub use limit::Limit;
pub use tracked::Tracked;
pub use write_once::WriteOnce;

/// An implementation that accepts concrete types of chunked writes
pub trait Storage {
    const SPECIALIZES_BYTES: bool = false;
    const SPECIALIZES_BYTES_MUT: bool = false;

    /// Writes a slice of bytes into the storage
    ///
    /// The bytes MUST always be less than `remaining_capacity`.
    fn put_slice(&mut self, bytes: &[u8]);

    /// Tries to write into a uninit slice for the current storage
    ///
    /// If `false` is returned, the storage wasn't capable of this operation and a regular `put_*`
    /// call should be used instead.
    #[inline(always)]
    fn put_uninit_slice<F, Error>(&mut self, payload_len: usize, f: F) -> Result<bool, Error>
    where
        F: FnOnce(&mut UninitSlice) -> Result<(), Error>,
    {
        // we can specialize on an empty payload
        ensure!(payload_len == 0, Ok(false));

        f(UninitSlice::new(&mut []))?;

        Ok(true)
    }

    /// Returns the additional number of bytes that can be written to the storage
    fn remaining_capacity(&self) -> usize;

    /// Returns `true` if the storage will accept any additional bytes
    #[inline]
    fn has_remaining_capacity(&self) -> bool {
        self.remaining_capacity() > 0
    }

    /// Writes [`Bytes`] into the storage
    ///
    /// Callers should check `SPECIALIZES_BYTES` before deciding to use this method. Otherwise, it
    /// might be cheaper to copy a slice into the storage and then increment the offset.
    #[inline]
    fn put_bytes(&mut self, bytes: Bytes) {
        self.put_slice(&bytes);
    }

    /// Writes [`BytesMut`] into the storage
    ///
    /// Callers should check `SPECIALIZES_BYTES_MUT` before deciding to use this method. Otherwise, it
    /// might be cheaper to copy a slice into the storage and then increment the offset.
    #[inline]
    fn put_bytes_mut(&mut self, bytes: BytesMut) {
        self.put_slice(&bytes);
    }

    /// Writes a reader [`Chunk`] into the storage
    #[inline]
    fn put_chunk(&mut self, chunk: Chunk) {
        match chunk {
            Chunk::Slice(v) => self.put_slice(v),
            Chunk::Bytes(v) => self.put_bytes(v),
            Chunk::BytesMut(v) => self.put_bytes_mut(v),
        }
    }

    /// Limits the number of bytes that can be written to the storage
    #[inline]
    fn with_write_limit(&mut self, max_len: usize) -> Limit<Self> {
        Limit::new(self, max_len)
    }

    /// Tracks the number of bytes written to the storage
    #[inline]
    fn track_write(&mut self) -> Tracked<Self> {
        Tracked::new(self)
    }

    /// Only allows a single write into the storage. After that, no more writes are allowed.
    ///
    /// This can be used for very low latency scenarios where processing the single read is more
    /// important than filling the entire storage with as much data as possible.
    #[inline]
    fn write_once(&mut self) -> WriteOnce<Self> {
        WriteOnce::new(self)
    }
}
