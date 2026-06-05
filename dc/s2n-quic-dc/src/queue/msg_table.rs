// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Message table: tracks in-flight messages and gates delivery on fence satisfaction.
//!
//! The table maps `msg_id` (a monotonic, contiguous u64 per stream) to `MsgEntry`.
//! When a message completes and all prior messages are also complete, the table
//! drains contiguous complete messages into the output queue.

use super::msg_entry::{self, CheckoutResult, CompleteResult, MsgEntry};
use bytes::{Bytes, BytesMut};
use std::collections::VecDeque;

/// Maximum number of in-flight messages per stream before rejecting new msg_ids.
pub(crate) const MAX_PENDING_MESSAGES: usize = 64;

/// The message reassembly table for a single stream slot.
pub(crate) struct MsgTable {
    base_id: u64,
    entries: VecDeque<Option<MsgEntry>>,
    fin_msg_id: Option<u64>,
}

/// Successful checkout from `insert`.
#[derive(Debug)]
pub(crate) struct Checkout {
    pub ptr: *mut u8,
    pub expected_len: u32,
    pub chunk_index: u32,
    pub keep_alive: Bytes,
}

/// Error from `insert`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InsertError {
    /// Frame was a duplicate (already received).
    Duplicate,
    /// Contention: another thread is writing this chunk.
    Contention,
    /// The msg_id is stale (already delivered).
    Stale,
    /// The msg_id gap exceeds MAX_PENDING_MESSAGES.
    GapExceeded,
    /// The message_size exceeds the maximum supported.
    MessageTooLarge,
    /// The entry is poisoned (stream was reset).
    Poisoned,
    /// message_size on this frame doesn't match the existing entry.
    SizeMismatch,
    /// is_fin flag doesn't match the existing entry.
    FinMismatch,
    /// chunk_index exceeds the message's chunk count.
    OffsetOverflow,
    /// chunk_size on this frame doesn't match the existing entry.
    ChunkSizeMismatch,
    /// payload_len doesn't match the expected chunk length.
    PayloadLenMismatch,
}

/// Outcome of completing a chunk write.
pub(crate) enum CompleteOutcome {
    /// More chunks pending; no messages ready for delivery.
    Pending,
    /// One or more messages are ready. Drain them with `drain_complete`.
    Ready,
    /// The entry was poisoned during the write.
    Poisoned,
}

/// A delivered message ready to be pushed into the intrusive queue.
pub(crate) struct DeliveredMsg {
    pub payload: BytesMut,
    pub stream_offset: u64,
    pub is_fin: bool,
}

impl MsgTable {
    pub(crate) fn new() -> Self {
        Self {
            base_id: 0,
            entries: VecDeque::new(),
            fin_msg_id: None,
        }
    }

    /// Attempt to insert a frame for the given msg_id.
    ///
    /// On success, returns a `Checkout` with pointer, expected length, and chunk index.
    /// The caller writes data at the pointer (outside the lock), then calls `complete`
    /// with `msg_id` and `chunk_index`.
    pub(crate) fn insert(
        &mut self,
        msg_id: u64,
        stream_offset: u64,
        message_size: u32,
        chunk_size: u16,
        chunk_index: u32,
        payload_len: u32,
        is_fin: bool,
    ) -> Result<Checkout, InsertError> {
        if msg_id < self.base_id {
            return Err(InsertError::Stale);
        }

        if self.is_fin_delivered() {
            return Err(InsertError::Stale);
        }

        let index = (msg_id - self.base_id) as usize;

        if index >= MAX_PENDING_MESSAGES {
            return Err(InsertError::GapExceeded);
        }

        if chunk_size == 0 {
            return Err(InsertError::MessageTooLarge);
        }

        // Validate using u32 arithmetic to avoid u16 truncation in chunks_for_size
        let true_chunk_count = (message_size as u64).div_ceil(chunk_size as u64);
        if true_chunk_count > msg_entry::MAX_CHUNKS as u64 {
            return Err(InsertError::MessageTooLarge);
        }

        let chunk_count = true_chunk_count as u16;

        if chunk_index >= chunk_count as u32 {
            return Err(InsertError::OffsetOverflow);
        }

        // Extend the deque if needed
        if index >= self.entries.len() {
            self.entries.resize_with(index + 1, || None);
        }

        let entry = self.entries[index]
            .get_or_insert_with(|| MsgEntry::new(message_size, chunk_size, stream_offset, is_fin));

        if entry.message_size() != message_size {
            return Err(InsertError::SizeMismatch);
        }

        if entry.stream_offset() != stream_offset {
            return Err(InsertError::SizeMismatch);
        }

        if entry.is_fin() != is_fin {
            return Err(InsertError::FinMismatch);
        }

        if entry.chunk_size() != chunk_size {
            return Err(InsertError::ChunkSizeMismatch);
        }

        if is_fin {
            self.fin_msg_id = Some(msg_id);
        }

        match entry.checkout(chunk_index) {
            CheckoutResult::Ok {
                ptr,
                len,
                keep_alive,
            } => {
                if payload_len != len {
                    entry.cancel_checkout(chunk_index);
                    return Err(InsertError::PayloadLenMismatch);
                }
                Ok(Checkout {
                    ptr,
                    expected_len: len,
                    chunk_index,
                    keep_alive,
                })
            }
            CheckoutResult::Duplicate => Err(InsertError::Duplicate),
            CheckoutResult::Contention => Err(InsertError::Contention),
            CheckoutResult::Poisoned => Err(InsertError::Poisoned),
        }
    }

    /// Complete a chunk write. Call after copying data into the pointer from `insert`.
    pub(crate) fn complete(&mut self, msg_id: u64, chunk_index: u32) -> CompleteOutcome {
        debug_assert!(msg_id >= self.base_id);
        let index = (msg_id - self.base_id) as usize;
        debug_assert!(index < self.entries.len());

        let entry = self.entries[index]
            .as_mut()
            .expect("entry must exist after checkout");

        match entry.complete_chunk(chunk_index) {
            CompleteResult::Pending => CompleteOutcome::Pending,
            CompleteResult::Complete => {
                if self.can_deliver() {
                    CompleteOutcome::Ready
                } else {
                    CompleteOutcome::Pending
                }
            }
            CompleteResult::Poisoned => CompleteOutcome::Poisoned,
        }
    }

    /// Drain all contiguous complete messages from the front of the table.
    pub(crate) fn drain_complete(&mut self) -> DrainIter<'_> {
        DrainIter { table: self }
    }

    /// Returns true if the stream has reached FIN (all messages through fin_msg_id delivered).
    pub(crate) fn is_fin_delivered(&self) -> bool {
        match self.fin_msg_id {
            Some(fin_id) => fin_id < self.base_id,
            None => false,
        }
    }

    /// Cancel a checkout without marking received (write failed, e.g. decrypt error).
    /// The chunk can be retried on a future retransmission.
    pub(crate) fn cancel_checkout(&mut self, msg_id: u64, chunk_index: u32) {
        debug_assert!(msg_id >= self.base_id);
        let index = (msg_id - self.base_id) as usize;
        if let Some(Some(entry)) = self.entries.get_mut(index) {
            entry.cancel_checkout(chunk_index);
        }
    }

    /// Poison all entries (called during reset).
    pub(crate) fn poison(&mut self) {
        for entry in self.entries.iter_mut().flatten() {
            entry.poison();
        }
        for entry in self.entries.iter_mut() {
            if let Some(e) = entry {
                if !e.has_checkouts() {
                    *entry = None;
                }
            }
        }
    }

    fn can_deliver(&self) -> bool {
        matches!(self.entries.front(), Some(Some(entry)) if entry.is_complete())
    }
}

pub(crate) struct DrainIter<'a> {
    table: &'a mut MsgTable,
}

impl Iterator for DrainIter<'_> {
    type Item = DeliveredMsg;

    fn next(&mut self) -> Option<Self::Item> {
        let entry = self.table.entries.front()?.as_ref()?;
        if !entry.is_complete() {
            return None;
        }

        let entry = self.table.entries.pop_front().unwrap().unwrap();
        self.table.base_id += 1;

        let stream_offset = entry.stream_offset();
        let is_fin = entry.is_fin();
        let payload = entry.into_buffer();

        Some(DeliveredMsg {
            payload,
            stream_offset,
            is_fin,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK_SIZE: u16 = 8192;

    fn write_chunk(
        table: &mut MsgTable,
        msg_id: u64,
        stream_offset: u64,
        message_size: u32,
        chunk_index: u32,
        payload_len: u32,
    ) -> CompleteOutcome {
        let checkout = table
            .insert(
                msg_id,
                stream_offset,
                message_size,
                CHUNK_SIZE,
                chunk_index,
                payload_len,
                false,
            )
            .expect("insert should succeed");
        assert_eq!(checkout.expected_len, payload_len);
        unsafe { core::ptr::write_bytes(checkout.ptr, 0xAB, payload_len as usize) };
        table.complete(msg_id, checkout.chunk_index)
    }

    // insert args: msg_id, stream_offset, message_size, chunk_size, chunk_index, payload_len, is_fin

    #[test]
    fn single_message_single_chunk() {
        let mut table = MsgTable::new();
        // 4096-byte message, one chunk
        let outcome = write_chunk(&mut table, 0, 0, 4096, 0, 4096);
        assert!(matches!(outcome, CompleteOutcome::Ready));

        let delivered: Vec<_> = table.drain_complete().collect();
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].payload.len(), 4096);
        assert_eq!(delivered[0].stream_offset, 0);
        assert!(!delivered[0].is_fin);
    }

    #[test]
    fn single_message_multi_chunk() {
        let mut table = MsgTable::new();
        let msg_size = CHUNK_SIZE as u32 * 4;

        for i in 0..3u32 {
            let outcome = write_chunk(&mut table, 0, 0, msg_size, i, CHUNK_SIZE as u32);
            assert!(matches!(outcome, CompleteOutcome::Pending));
        }

        let outcome = write_chunk(&mut table, 0, 0, msg_size, 3, CHUNK_SIZE as u32);
        assert!(matches!(outcome, CompleteOutcome::Ready));

        let delivered: Vec<_> = table.drain_complete().collect();
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].payload.len(), msg_size as usize);
    }

    #[test]
    fn out_of_order_chunks() {
        let mut table = MsgTable::new();
        let msg_size = CHUNK_SIZE as u32 * 3;

        assert!(matches!(
            write_chunk(&mut table, 0, 0, msg_size, 2, CHUNK_SIZE as u32),
            CompleteOutcome::Pending
        ));
        assert!(matches!(
            write_chunk(&mut table, 0, 0, msg_size, 0, CHUNK_SIZE as u32),
            CompleteOutcome::Pending
        ));
        assert!(matches!(
            write_chunk(&mut table, 0, 0, msg_size, 1, CHUNK_SIZE as u32),
            CompleteOutcome::Ready
        ));
    }

    #[test]
    fn multiple_messages_stream_offset() {
        let mut table = MsgTable::new();

        write_chunk(&mut table, 0, 0, 4096, 0, 4096);
        write_chunk(&mut table, 1, 4096, 4096, 0, 4096);

        let delivered: Vec<_> = table.drain_complete().collect();
        assert_eq!(delivered.len(), 2);
        assert_eq!(delivered[0].stream_offset, 0);
        assert_eq!(delivered[1].stream_offset, 4096);
    }

    #[test]
    fn fence_blocks_delivery_until_prior_complete() {
        let mut table = MsgTable::new();

        // msg_id=1 completes first
        assert!(matches!(
            write_chunk(&mut table, 1, 4096, 4096, 0, 4096),
            CompleteOutcome::Pending
        ));
        assert_eq!(table.drain_complete().count(), 0);

        // msg_id=0 completes — both deliver
        assert!(matches!(
            write_chunk(&mut table, 0, 0, 4096, 0, 4096),
            CompleteOutcome::Ready
        ));

        let delivered: Vec<_> = table.drain_complete().collect();
        assert_eq!(delivered.len(), 2);
        assert_eq!(delivered[0].stream_offset, 0);
        assert_eq!(delivered[1].stream_offset, 4096);
    }

    #[test]
    fn stale_msg_id() {
        let mut table = MsgTable::new();

        write_chunk(&mut table, 0, 0, 4096, 0, 4096);
        table.drain_complete().count();

        let result = table.insert(0, 0, 4096, CHUNK_SIZE, 0, 4096, false);
        assert_eq!(result.unwrap_err(), InsertError::Stale);
    }

    #[test]
    fn gap_exceeded() {
        let mut table = MsgTable::new();
        let result = table.insert(
            MAX_PENDING_MESSAGES as u64,
            0,
            4096,
            CHUNK_SIZE,
            0,
            4096,
            false,
        );
        assert_eq!(result.unwrap_err(), InsertError::GapExceeded);
    }

    #[test]
    fn size_mismatch() {
        let mut table = MsgTable::new();
        // Insert chunk 0 of an 8192-byte message (1 chunk at CHUNK_SIZE=8192)
        write_chunk(&mut table, 0, 0, CHUNK_SIZE as u32, 0, CHUNK_SIZE as u32);

        // Try again with different message_size
        let result = table.insert(0, 0, 9999, CHUNK_SIZE, 0, CHUNK_SIZE as u32, false);
        assert_eq!(result.unwrap_err(), InsertError::SizeMismatch);
    }

    #[test]
    fn fin_mismatch() {
        let mut table = MsgTable::new();
        let msg_size = CHUNK_SIZE as u32 * 2;

        // First frame says no fin
        table
            .insert(0, 0, msg_size, CHUNK_SIZE, 0, CHUNK_SIZE as u32, false)
            .unwrap();

        // Second frame for same msg says fin — mismatch
        let result = table.insert(0, 0, msg_size, CHUNK_SIZE, 1, CHUNK_SIZE as u32, true);
        assert_eq!(result.unwrap_err(), InsertError::FinMismatch);
    }

    #[test]
    fn chunk_index_overflow() {
        let mut table = MsgTable::new();
        // 4096-byte message with 8192 chunk_size = 1 chunk. chunk_index=1 is out of bounds.
        let result = table.insert(0, 0, 4096, CHUNK_SIZE, 1, 4096, false);
        assert_eq!(result.unwrap_err(), InsertError::OffsetOverflow);
    }

    #[test]
    fn duplicate_chunk() {
        let mut table = MsgTable::new();
        let msg_size = CHUNK_SIZE as u32 * 2;
        write_chunk(&mut table, 0, 0, msg_size, 0, CHUNK_SIZE as u32);

        let result = table.insert(0, 0, msg_size, CHUNK_SIZE, 0, CHUNK_SIZE as u32, false);
        assert_eq!(result.unwrap_err(), InsertError::Duplicate);
    }

    #[test]
    fn poison_frees_complete_entries() {
        let mut table = MsgTable::new();

        // msg_id=0: checkout outstanding (don't complete)
        table
            .insert(0, 0, 4096, CHUNK_SIZE, 0, 4096, false)
            .unwrap();

        // msg_id=1: fully complete
        write_chunk(&mut table, 1, 4096, 4096, 0, 4096);

        table.poison();

        // msg_id=0 still has checkout, entry stays
        assert!(table.entries[0].is_some());
        // msg_id=1 had no checkouts, freed
        assert!(table.entries[1].is_none());
    }

    #[test]
    fn fin_delivery() {
        let mut table = MsgTable::new();

        let checkout = table.insert(0, 0, 4096, CHUNK_SIZE, 0, 4096, true).unwrap();
        unsafe { core::ptr::write_bytes(checkout.ptr, 0, checkout.expected_len as usize) };
        table.complete(0, checkout.chunk_index);

        assert!(!table.is_fin_delivered());

        let delivered: Vec<_> = table.drain_complete().collect();
        assert_eq!(delivered.len(), 1);
        assert!(delivered[0].is_fin);
        assert!(table.is_fin_delivered());
    }

    #[test]
    fn starts_empty_no_allocation() {
        let table = MsgTable::new();
        assert_eq!(table.entries.capacity(), 0);
    }

    #[test]
    fn insert_rejected_after_fin_delivered() {
        let mut table = MsgTable::new();

        // Deliver a FIN message
        let checkout = table.insert(0, 0, 4096, CHUNK_SIZE, 0, 4096, true).unwrap();
        unsafe { core::ptr::write_bytes(checkout.ptr, 0, checkout.expected_len as usize) };
        table.complete(0, checkout.chunk_index);
        table.drain_complete().count();

        assert!(table.is_fin_delivered());

        // New insert must be rejected as stale
        let result = table.insert(1, 4096, 4096, CHUNK_SIZE, 0, 4096, false);
        assert_eq!(result.unwrap_err(), InsertError::Stale);
    }

    #[test]
    fn insert_allowed_for_fin_message_chunks_before_delivery() {
        let mut table = MsgTable::new();
        let msg_size = CHUNK_SIZE as u32 * 2;

        // Insert first chunk of FIN message
        let checkout = table
            .insert(0, 0, msg_size, CHUNK_SIZE, 0, CHUNK_SIZE as u32, true)
            .unwrap();
        unsafe { core::ptr::write_bytes(checkout.ptr, 0, checkout.expected_len as usize) };
        table.complete(0, checkout.chunk_index);

        // FIN not yet delivered (second chunk missing)
        assert!(!table.is_fin_delivered());

        // Second chunk should still work
        let checkout = table
            .insert(0, 0, msg_size, CHUNK_SIZE, 1, CHUNK_SIZE as u32, true)
            .unwrap();
        unsafe { core::ptr::write_bytes(checkout.ptr, 0, checkout.expected_len as usize) };
        table.complete(0, checkout.chunk_index);

        let delivered: Vec<_> = table.drain_complete().collect();
        assert_eq!(delivered.len(), 1);
        assert!(delivered[0].is_fin);
        assert!(table.is_fin_delivered());
    }

    #[test]
    fn chunk_size_zero_rejected() {
        let mut table = MsgTable::new();
        let result = table.insert(0, 0, 4096, 0, 0, 4096, false);
        assert_eq!(result.unwrap_err(), InsertError::MessageTooLarge);
    }

    #[test]
    fn chunk_size_zero_via_truncation_rejected() {
        let mut table = MsgTable::new();
        let chunk_size_on_wire: u64 = 65536;
        let chunk_size_truncated = chunk_size_on_wire as u16;
        assert_eq!(chunk_size_truncated, 0);
        let result = table.insert(0, 0, 4096, chunk_size_truncated, 0, 4096, false);
        assert_eq!(result.unwrap_err(), InsertError::MessageTooLarge);
    }

    #[test]
    fn chunk_count_overflow_rejected() {
        let mut table = MsgTable::new();
        // message_size=65792 / chunk_size=1 → true chunk_count=65792 > MAX_CHUNKS(256)
        let result = table.insert(0, 0, 65792, 1, 0, 1, false);
        assert_eq!(result.unwrap_err(), InsertError::MessageTooLarge);
    }

    #[test]
    fn max_valid_chunk_count_accepted() {
        let mut table = MsgTable::new();
        // 256 chunks of 1 byte each = message_size 256, exactly at MAX_CHUNKS
        let result = table.insert(0, 0, 256, 1, 0, 1, false);
        assert!(result.is_ok());
    }

    #[test]
    fn one_over_max_chunks_rejected() {
        let mut table = MsgTable::new();
        // 257 chunks needed → rejected
        let result = table.insert(0, 0, 257, 1, 0, 1, false);
        assert_eq!(result.unwrap_err(), InsertError::MessageTooLarge);
    }

    /// Regression: a frame with a smaller chunk_size than the first frame can
    /// produce a higher chunk_count, allowing a chunk_index that exceeds the
    /// entry's actual chunk_count. The entry's checkout() then computes an
    /// offset using the stored (larger) chunk_size, writing beyond the buffer.
    ///
    /// Example: message_size=16384, first frame chunk_size=8192 → entry has 2 chunks.
    /// Attacker frame chunk_size=4096 → validation computes 4 chunks, chunk_index=3
    /// passes OffsetOverflow. But entry.checkout(3) computes offset=3*8192=24576,
    /// which is beyond the 16384-byte buffer. In release builds (no debug_assert),
    /// this is a heap buffer overflow.
    #[test]
    fn chunk_size_mismatch_rejects_overflow() {
        let mut table = MsgTable::new();

        // First frame establishes the entry with chunk_size=8192.
        // message_size=16384 → chunk_count = 2 (chunks 0 and 1).
        let result = table.insert(0, 0, 16384, 8192, 0, 8192, false);
        assert!(result.is_ok());

        // Attacker sends a frame claiming chunk_size=4096 for the same msg_id.
        // With chunk_size=4096, chunk_count = 4, so chunk_index=3 passes
        // the OffsetOverflow check (3 < 4). But the entry was created with
        // chunk_size=8192, so checkout() would compute offset = 3 * 8192 = 24576,
        // far beyond the 16384-byte buffer.
        //
        // This MUST be rejected before reaching checkout().
        let result = table.insert(0, 0, 16384, 4096, 3, 4096, false);
        assert_eq!(result.unwrap_err(), InsertError::ChunkSizeMismatch);
    }
}
