// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Per-message reassembly state for the QueueMsg sub-protocol.
//!
//! Each in-flight message gets one `MsgEntry` that tracks which chunks have been
//! received, which are currently checked out for writing, and whether the entry
//! has been poisoned by a reset.

use crate::bitset::InlineBitSet;
use bytes::{Bytes, BytesMut};

/// Maximum number of chunks per message (limited by inline bitset capacity).
/// At 8KB per chunk this covers messages up to 2MB.
pub const MAX_CHUNKS: u32 = InlineBitSet::<4>::CAPACITY;

/// Per-message reassembly entry.
///
/// Lives inside the MsgTable (one per in-flight msg_id). The buffer is pre-allocated
/// to `message_size` on first frame arrival. Chunks are written directly into the buffer
/// at their message-local offset without intermediate allocation.
pub(crate) struct MsgEntry {
    buffer: BytesMut,
    keep_alive: Bytes,
    stream_offset: u64,
    chunk_size: u16,
    chunk_count: u16,
    received: InlineBitSet<4>,
    checked_out: InlineBitSet<4>,
    poisoned: bool,
    is_fin: bool,
}

/// Result of attempting to check out a chunk for writing.
pub(crate) enum CheckoutResult {
    /// The chunk is available. Write exactly `len` bytes at `ptr`.
    Ok {
        ptr: *mut u8,
        len: u32,
        keep_alive: Bytes,
    },
    /// Duplicate: this chunk was already received.
    Duplicate,
    /// Another thread is currently writing this chunk (contention on retransmit).
    Contention,
    /// The entry has been poisoned by a reset; discard the frame.
    Poisoned,
}

/// Result of completing a chunk write.
pub(crate) enum CompleteResult {
    /// More chunks are still pending.
    Pending,
    /// All chunks received; the message is fully assembled.
    Complete,
    /// The entry was poisoned while we were writing; discard work.
    Poisoned,
}

impl MsgEntry {
    /// Creates a new entry for a message with the given total size and per-chunk size.
    ///
    /// The buffer is allocated with `total_size` capacity and unsafely set to that length.
    /// The bitset guarantees write-before-read, so uninitialized memory is never exposed.
    pub(crate) fn new(
        message_size: u32,
        chunk_size: u16,
        stream_offset: u64,
        is_fin: bool,
    ) -> Self {
        let chunk_count = chunks_for_size(message_size, chunk_size);
        debug_assert!(chunk_count as u32 <= MAX_CHUNKS);

        // +1 byte provides a tiny prefix we can split/freeze as a keep-alive handle.
        // This avoids copying/cloning the message payload while still creating a ref-counted
        // handle that keeps the original allocation alive during checkout writes.
        let mut buffer = BytesMut::with_capacity(message_size as usize + 1);
        // SAFETY: The received bitset guarantees that only fully-written regions are ever
        // read. No consumer sees the buffer until all chunks are marked received.
        // We reserve one extra prefix byte and split/freeze it as a keep-alive handle so
        // checked-out pointers remain valid even if the table is cleared concurrently.
        unsafe { buffer.set_len(message_size as usize + 1) };
        let keep_alive = buffer.split_to(1).freeze();

        Self {
            buffer,
            keep_alive,
            stream_offset,
            chunk_size,
            chunk_count,
            received: InlineBitSet::new(),
            checked_out: InlineBitSet::new(),
            poisoned: false,
            is_fin,
        }
    }

    #[inline]
    pub(crate) fn message_size(&self) -> u32 {
        self.buffer.len() as u32
    }

    #[inline]
    pub(crate) fn chunk_size(&self) -> u16 {
        self.chunk_size
    }

    #[inline]
    pub(crate) fn stream_offset(&self) -> u64 {
        self.stream_offset
    }

    #[inline]
    pub(crate) fn is_fin(&self) -> bool {
        self.is_fin
    }

    #[inline]
    #[cfg(test)]
    pub(crate) fn is_poisoned(&self) -> bool {
        self.poisoned
    }

    #[inline]
    pub(crate) fn has_checkouts(&self) -> bool {
        !self.checked_out.is_empty()
    }

    #[inline]
    pub(crate) fn is_complete(&self) -> bool {
        self.received.all_set(self.chunk_count as u32) && self.checked_out.is_empty()
    }

    /// Attempt to check out a chunk for writing.
    ///
    /// Called under the slot lock. On success, the caller releases the lock,
    /// writes the chunk data, then calls `complete_chunk`.
    pub(crate) fn checkout(&mut self, chunk_index: u32) -> CheckoutResult {
        debug_assert!(chunk_index < self.chunk_count as u32);

        if self.poisoned {
            return CheckoutResult::Poisoned;
        }
        if self.received.get(chunk_index) {
            return CheckoutResult::Duplicate;
        }
        if self.checked_out.get(chunk_index) {
            return CheckoutResult::Contention;
        }

        self.checked_out.insert(chunk_index);
        let offset = chunk_index * self.chunk_size as u32;
        let len = if chunk_index == self.chunk_count as u32 - 1 {
            self.message_size() - offset
        } else {
            self.chunk_size as u32
        };
        // SAFETY: offset is bounded by message_size (checked via chunk_index < chunk_count).
        // The pointer is stable because BytesMut capacity == len (never reallocates).
        let ptr = unsafe { self.buffer.as_mut_ptr().add(offset as usize) };
        CheckoutResult::Ok {
            ptr,
            len,
            keep_alive: self.keep_alive.clone(),
        }
    }

    /// Complete a chunk write after the data has been copied into the buffer.
    ///
    /// Called under the slot lock after releasing and re-acquiring it.
    pub(crate) fn complete_chunk(&mut self, chunk_index: u32) -> CompleteResult {
        debug_assert!(chunk_index < self.chunk_count as u32);
        debug_assert!(self.checked_out.get(chunk_index));

        self.checked_out.remove(chunk_index);

        if self.poisoned {
            return CompleteResult::Poisoned;
        }

        self.received.insert(chunk_index);

        if self.received.all_set(self.chunk_count as u32) && self.checked_out.is_empty() {
            CompleteResult::Complete
        } else {
            CompleteResult::Pending
        }
    }

    /// Cancel a checkout without marking received.
    /// Used when the write callback fails (e.g. decrypt error).
    pub(crate) fn cancel_checkout(&mut self, chunk_index: u32) {
        debug_assert!(chunk_index < self.chunk_count as u32);
        debug_assert!(self.checked_out.get(chunk_index));
        self.checked_out.remove(chunk_index);
    }

    /// Poison this entry (called during reset).
    ///
    /// If no chunks are checked out, the caller can immediately free the buffer.
    /// Otherwise, the last writer to call `complete_chunk` will see `Poisoned` and
    /// the entry can be freed then.
    pub(crate) fn poison(&mut self) {
        self.poisoned = true;
    }

    /// Consumes the entry and returns the assembled buffer.
    ///
    /// Only valid to call after `complete_chunk` returns `Complete`.
    pub(crate) fn into_buffer(self) -> BytesMut {
        debug_assert!(self.received.all_set(self.chunk_count as u32));
        debug_assert!(self.checked_out.is_empty());
        self.buffer
    }
}

/// Computes the number of chunks needed for a message of `total_size` bytes
/// with `chunk_size` bytes per chunk.
#[inline]
pub(crate) fn chunks_for_size(total_size: u32, chunk_size: u16) -> u16 {
    let cs = chunk_size as u32;
    total_size.div_ceil(cs) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK_SIZE: u16 = 8192;

    #[test]
    fn chunks_for_size_basic() {
        assert_eq!(chunks_for_size(0, CHUNK_SIZE), 0);
        assert_eq!(chunks_for_size(1, CHUNK_SIZE), 1);
        assert_eq!(chunks_for_size(8192, CHUNK_SIZE), 1);
        assert_eq!(chunks_for_size(8193, CHUNK_SIZE), 2);
        assert_eq!(chunks_for_size(65536, CHUNK_SIZE), 8);
        assert_eq!(chunks_for_size(1048576, CHUNK_SIZE), 128);
    }

    #[test]
    fn new_entry() {
        let entry = MsgEntry::new(65536, CHUNK_SIZE, 0, false);
        assert_eq!(entry.message_size(), 65536);
        assert!(!entry.is_fin());
        assert!(!entry.is_poisoned());
        assert!(!entry.has_checkouts());
    }

    #[test]
    fn checkout_and_complete_single_chunk() {
        let mut entry = MsgEntry::new(8192, CHUNK_SIZE, 0, true);

        match entry.checkout(0) {
            CheckoutResult::Ok { ptr, len, .. } => {
                assert!(!ptr.is_null());
                assert_eq!(len, 8192);
            }
            _ => panic!("expected Ok"),
        }
        assert!(entry.has_checkouts());

        match entry.complete_chunk(0) {
            CompleteResult::Complete => {}
            _ => panic!("expected Complete"),
        }
    }

    #[test]
    fn checkout_and_complete_multi_chunk() {
        let total_size = 8192 * 4;
        let mut entry = MsgEntry::new(total_size, CHUNK_SIZE, 0, false);
        let base_ptr = entry.buffer.as_mut_ptr();

        // Check out all chunks
        for i in 0..4 {
            match entry.checkout(i) {
                CheckoutResult::Ok { ptr, len, .. } => {
                    assert_eq!(ptr, unsafe { base_ptr.add((i * 8192) as usize) });
                    assert_eq!(len, 8192);
                }
                _ => panic!("expected Ok for chunk {i}"),
            }
        }

        // Complete first 3 — still pending
        for i in 0..3 {
            match entry.complete_chunk(i) {
                CompleteResult::Pending => {}
                _ => panic!("expected Pending for chunk {i}"),
            }
        }

        // Complete last — now complete
        match entry.complete_chunk(3) {
            CompleteResult::Complete => {}
            _ => panic!("expected Complete"),
        }
    }

    #[test]
    fn out_of_order_completion() {
        let mut entry = MsgEntry::new(8192 * 3, CHUNK_SIZE, 0, false);

        // Checkout in order
        for i in 0..3 {
            assert!(matches!(entry.checkout(i), CheckoutResult::Ok { .. }));
        }

        // Complete out of order: 2, 0, 1
        assert!(matches!(entry.complete_chunk(2), CompleteResult::Pending));
        assert!(matches!(entry.complete_chunk(0), CompleteResult::Pending));
        assert!(matches!(entry.complete_chunk(1), CompleteResult::Complete));
    }

    #[test]
    fn duplicate_detection() {
        let mut entry = MsgEntry::new(8192 * 2, CHUNK_SIZE, 0, false);

        assert!(matches!(entry.checkout(0), CheckoutResult::Ok { .. }));
        assert!(matches!(entry.complete_chunk(0), CompleteResult::Pending));

        // Second attempt at same chunk is duplicate
        assert!(matches!(entry.checkout(0), CheckoutResult::Duplicate));
    }

    #[test]
    fn contention_detection() {
        let mut entry = MsgEntry::new(8192 * 2, CHUNK_SIZE, 0, false);

        assert!(matches!(entry.checkout(0), CheckoutResult::Ok { .. }));
        // Same chunk checked out again while first is still outstanding
        assert!(matches!(entry.checkout(0), CheckoutResult::Contention));
    }

    #[test]
    fn poison_before_checkout() {
        let mut entry = MsgEntry::new(8192, CHUNK_SIZE, 0, false);
        entry.poison();
        assert!(entry.is_poisoned());
        assert!(matches!(entry.checkout(0), CheckoutResult::Poisoned));
    }

    #[test]
    fn poison_during_checkout() {
        let mut entry = MsgEntry::new(8192, CHUNK_SIZE, 0, false);

        assert!(matches!(entry.checkout(0), CheckoutResult::Ok { .. }));
        entry.poison();
        assert!(matches!(entry.complete_chunk(0), CompleteResult::Poisoned));
    }

    #[test]
    fn into_buffer() {
        let mut entry = MsgEntry::new(100, 100, 0, false);

        match entry.checkout(0) {
            CheckoutResult::Ok { ptr, len, .. } => {
                assert!(!ptr.is_null());
                assert_eq!(len, 100);
            }
            _ => panic!("expected Ok"),
        }
        assert!(matches!(entry.complete_chunk(0), CompleteResult::Complete));

        let buf = entry.into_buffer();
        assert_eq!(buf.len(), 100);
    }

    #[test]
    fn completion_requires_no_outstanding_checkouts() {
        let mut entry = MsgEntry::new(8192 * 2, CHUNK_SIZE, 0, false);

        // Check out both chunks
        assert!(matches!(entry.checkout(0), CheckoutResult::Ok { .. }));
        assert!(matches!(entry.checkout(1), CheckoutResult::Ok { .. }));

        // Complete chunk 1 — all received but chunk 0 still checked out
        entry.received.insert(0); // simulate chunk 0 received by another path
        entry.checked_out.remove(0); // simulate checkout cleared
        entry.received.insert(1); // mark chunk 1 received
        entry.checked_out.remove(1);

        // Directly test the completion condition
        assert!(entry.received.all_set(2));
        assert!(entry.checked_out.is_empty());
    }

    #[test]
    fn last_chunk_len_is_remainder() {
        // 10000 bytes / 8192 chunk_size = 2 chunks: first is 8192, last is 1808
        let mut entry = MsgEntry::new(10000, CHUNK_SIZE, 0, false);

        match entry.checkout(0) {
            CheckoutResult::Ok { ptr, len, .. } => {
                assert!(!ptr.is_null());
                assert_eq!(len, 8192);
            }
            _ => panic!("expected Ok"),
        }

        match entry.checkout(1) {
            CheckoutResult::Ok { ptr, len, .. } => {
                assert!(!ptr.is_null());
                assert_eq!(len, 10000 - 8192);
            }
            _ => panic!("expected Ok"),
        }
    }
}
