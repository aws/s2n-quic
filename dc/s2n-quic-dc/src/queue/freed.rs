// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Server-side freed-queue notification.
//!
//! When a server-side receiver is dropped the stream's slot index must be
//! recycled back to the client so it can reuse that queue slot.  This module
//! implements the per-peer accumulator and the batch submission channel.
//!
//! ## Flow
//!
//! 1. Stream completes → `StreamReceiver` / `ControlReceiver` dropped.
//! 2. `FreedSender::record(queue_id)` is called.
//! 3. The `queue_id` is inserted into the pending set under the inner lock.
//!    If no batch token is in-flight, a new `FreedBatch` token is submitted to
//!    the global endpoint channel and `in_flight` is set.
//! 4. The emission task calls `FreedBatch::take_snapshot` at serialisation
//!    time to drain the pending set.  Any IDs that accumulated between step 3
//!    and step 4 are included naturally — this is the "snapshot at transmission
//!    time" property that yields free batching.
//! 5. After the frame is sent the emission task calls
//!    `FreedBatch::check_and_resubmit`.  If more IDs accumulated the token
//!    requeues itself; otherwise `in_flight` is cleared and the next `record`
//!    call will submit a fresh token.

use crate::path::secret::map::Entry as PathSecretEntry;
use s2n_quic_core::{interval_set::IntervalSet, varint::VarInt};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// A token submitted to the endpoint emission task.
///
/// The actual freed ranges are NOT captured at submission time.  Instead,
/// `take` drains the shared `FreedInner` state at serialisation time,
/// picking up any IDs that arrived after the token was enqueued.
///
/// # Contract
///
/// The emission task MUST call `check_and_resubmit` for every token it
/// receives, even if transmission fails.  Dropping a `FreedBatch` without
/// calling `check_and_resubmit` will leave `in_flight` permanently set,
/// preventing future freed-ID emission for this peer.
pub struct FreedBatch {
    inner: Arc<FreedInner>,
    pub path_entry: Arc<PathSecretEntry>,
}

impl FreedBatch {
    /// Swap the pending freed set with the caller's buffer, returning a request_id.
    ///
    /// After this call, `dest` contains the IDs to encode.  The caller should
    /// clear `dest` after encoding to preserve its allocation for the next swap.
    /// Returns `None` if there is nothing to send.
    pub fn take(&self, dest: &mut IntervalSet<VarInt>) -> Option<VarInt> {
        let mut state = self.inner.state.lock().unwrap();
        if state.freed.is_empty() {
            return None;
        }
        core::mem::swap(&mut state.freed, dest);
        Some(state.take_next_request_id())
    }

    /// Merge unsent ranges back into the pending set.
    ///
    /// Used when encoding hit the MTU budget and there are leftover ranges
    /// that need to be sent in a subsequent batch.
    pub fn put_back(&self, remainder: &mut IntervalSet<VarInt>) {
        let mut state = self.inner.state.lock().unwrap();
        let _ = state.freed.union(remainder);
        remainder.clear();
    }

    /// Called after transmission (or on failure).  If more IDs accumulated,
    /// resubmit the token; otherwise clear `in_flight`.
    ///
    /// # Contract
    ///
    /// This MUST be called exactly once per token received from the channel.
    /// Failure to call this permanently blocks freed-ID emission for this peer.
    pub fn check_and_resubmit(self, tx: &FreedBatchTx) {
        let mut state = self.inner.state.lock().unwrap();
        if state.freed.is_empty() {
            state.in_flight = false;
        } else {
            drop(state);
            let _ = tx.send(self);
        }
    }
}

/// Channel handle for submitting `FreedBatch` tokens to the global emission task.
pub type FreedBatchTx = mpsc::UnboundedSender<FreedBatch>;
pub type FreedBatchRx = mpsc::UnboundedReceiver<FreedBatch>;

/// Create the global channel for freed-batch emission.
pub fn freed_batch_channel() -> (FreedBatchTx, FreedBatchRx) {
    mpsc::unbounded_channel()
}

// ── Per-peer accumulator ──────────────────────────────────────────────────────

/// Shareable per-peer freed-queue state.
///
/// Cloned into each `StreamReceiver` / `ControlReceiver` that belongs to the
/// same peer so that the receiver Drop path can cheaply record a freed ID.
#[derive(Clone)]
pub struct FreedSender {
    inner: Arc<FreedInner>,
    path_entry: Arc<PathSecretEntry>,
    endpoint_tx: FreedBatchTx,
}

impl core::fmt::Debug for FreedSender {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FreedSender")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

pub(crate) struct FreedInner {
    state: Mutex<FreedState>,
}

impl core::fmt::Debug for FreedInner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.state.try_lock() {
            Ok(s) => f
                .debug_struct("FreedInner")
                .field("freed_count", &s.freed.count())
                .field("in_flight", &s.in_flight)
                .finish(),
            Err(_) => write!(f, "FreedInner(<locked>)"),
        }
    }
}

struct FreedState {
    /// IDs that have been freed but not yet included in a transmitted batch.
    freed: IntervalSet<VarInt>,
    /// Monotonically increasing request ID for the next batch.
    ///
    /// Saturates at `VarInt::MAX` rather than wrapping to preserve monotonicity
    /// and per-batch dedup on the client side.
    next_request_id: VarInt,
    /// True while a batch token has been submitted and not yet re-submitted or
    /// cleared by `check_and_resubmit`.
    in_flight: bool,
}

impl FreedState {
    fn take_next_request_id(&mut self) -> VarInt {
        let id = self.next_request_id;
        // Saturating increment within VarInt range preserves monotonicity.
        if let Ok(next) = VarInt::new(id.as_u64().saturating_add(1)) {
            self.next_request_id = next;
        }
        // If already at VarInt::MAX we keep re-using it; the client dedup
        // mechanism handles this edge case by never rejecting VarInt::MAX.
        id
    }
}

impl FreedSender {
    pub fn new(path_entry: Arc<PathSecretEntry>, endpoint_tx: FreedBatchTx) -> Self {
        Self {
            inner: Arc::new(FreedInner {
                state: Mutex::new(FreedState {
                    freed: IntervalSet::new(),
                    next_request_id: VarInt::from_u8(0),
                    in_flight: false,
                }),
            }),
            path_entry,
            endpoint_tx,
        }
    }

    /// Record that `queue_id` has been freed and, if no token is in-flight,
    /// submit a fresh one.
    ///
    /// Called on the application / receiver-drop path; designed to be cheap
    /// (a single lock + an optional channel send).
    pub fn record(&self, queue_id: VarInt) {
        let mut state = self.inner.state.lock().unwrap();

        let _ = state.freed.insert_value(queue_id);

        if state.in_flight {
            // A token is already outstanding; our ID will be included when
            // `take_snapshot` is called at serialisation time.
            return;
        }

        // No token in flight — submit one now.
        state.in_flight = true;
        drop(state);

        let token = FreedBatch {
            inner: self.inner.clone(),
            path_entry: self.path_entry.clone(),
        };
        if self.endpoint_tx.send(token).is_err() {
            // Channel closed — clear in_flight so we don't permanently block.
            self.inner.state.lock().unwrap().in_flight = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::varint::VarInt;

    fn test_sender() -> (FreedSender, FreedBatchRx) {
        let (tx, rx) = freed_batch_channel();
        let path_entry = PathSecretEntry::builder("127.0.0.1:4433".parse().unwrap()).build();
        (FreedSender::new(path_entry, tx), rx)
    }

    #[test]
    fn record_first_submits_token() {
        let (sender, mut rx) = test_sender();
        sender.record(VarInt::from_u8(5));
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn record_second_no_resubmit() {
        let (sender, mut rx) = test_sender();
        sender.record(VarInt::from_u8(5));
        sender.record(VarInt::from_u8(6));
        // Only one token in the channel
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn take_drains_accumulated() {
        let (sender, mut rx) = test_sender();
        sender.record(VarInt::from_u8(5));
        sender.record(VarInt::from_u8(6));
        sender.record(VarInt::from_u8(7));
        let batch = rx.try_recv().unwrap();
        let mut dest = IntervalSet::new();
        let request_id = batch.take(&mut dest);
        assert!(request_id.is_some());
        assert!(dest.contains(&VarInt::from_u8(5)));
        assert!(dest.contains(&VarInt::from_u8(6)));
        assert!(dest.contains(&VarInt::from_u8(7)));
    }

    #[test]
    fn take_empty_returns_none() {
        let (sender, mut rx) = test_sender();
        sender.record(VarInt::from_u8(1));
        let batch = rx.try_recv().unwrap();
        // First take drains
        let mut dest = IntervalSet::new();
        batch.take(&mut dest);
        // Second take is empty
        let result = batch.take(&mut dest);
        assert!(result.is_none());
    }

    #[test]
    fn take_request_id_increments() {
        let (sender, mut rx) = test_sender();
        let (tx, _) = freed_batch_channel();

        sender.record(VarInt::from_u8(1));
        let batch = rx.try_recv().unwrap();
        let mut dest = IntervalSet::new();
        let id0 = batch.take(&mut dest).unwrap();
        assert_eq!(id0, VarInt::from_u8(0));
        batch.check_and_resubmit(&tx);

        sender.record(VarInt::from_u8(2));
        let batch = rx.try_recv().unwrap();
        dest.clear();
        let id1 = batch.take(&mut dest).unwrap();
        assert_eq!(id1, VarInt::from_u8(1));
    }

    #[test]
    fn put_back_merges() {
        let (sender, mut rx) = test_sender();
        sender.record(VarInt::from_u8(10));
        sender.record(VarInt::from_u8(20));
        let batch = rx.try_recv().unwrap();

        let mut dest = IntervalSet::new();
        batch.take(&mut dest);
        assert_eq!(dest.count(), 2);

        // Simulate partial send: put back id=20
        let mut remainder = IntervalSet::new();
        let _ = remainder.insert_value(VarInt::from_u8(20));
        batch.put_back(&mut remainder);
        assert!(remainder.is_empty());

        // Record more while batch is still in flight
        sender.record(VarInt::from_u8(30));

        // Take again should include put-back + new
        dest.clear();
        let id = batch.take(&mut dest);
        assert!(id.is_some());
        assert!(dest.contains(&VarInt::from_u8(20)));
        assert!(dest.contains(&VarInt::from_u8(30)));
    }

    #[test]
    fn check_and_resubmit_clears_when_empty() {
        let (sender, mut rx) = test_sender();
        let (tx, _rx2) = freed_batch_channel();

        sender.record(VarInt::from_u8(1));
        let batch = rx.try_recv().unwrap();
        let mut dest = IntervalSet::new();
        batch.take(&mut dest);
        batch.check_and_resubmit(&tx);

        // in_flight cleared → next record submits fresh token
        sender.record(VarInt::from_u8(2));
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn check_and_resubmit_resubmits_when_more() {
        let (sender, mut rx) = test_sender();
        let (tx, mut rx2) = freed_batch_channel();

        sender.record(VarInt::from_u8(1));
        let batch = rx.try_recv().unwrap();
        let mut dest = IntervalSet::new();
        batch.take(&mut dest);

        // Record more during serialization
        sender.record(VarInt::from_u8(2));

        // check_and_resubmit sees non-empty → resubmits to tx
        batch.check_and_resubmit(&tx);
        assert!(rx2.try_recv().is_ok());
    }

    #[test]
    fn record_after_channel_closed() {
        let (sender, rx) = test_sender();
        drop(rx);
        // Should not panic
        sender.record(VarInt::from_u8(1));
        // in_flight should be cleared
        let state = sender.inner.state.lock().unwrap();
        assert!(!state.in_flight);
    }

    #[test]
    fn request_id_saturates() {
        let (sender, mut rx) = test_sender();
        let (tx, _) = freed_batch_channel();

        // Set next_request_id near max
        {
            let mut state = sender.inner.state.lock().unwrap();
            state.next_request_id = VarInt::new(VarInt::MAX.as_u64() - 1).unwrap();
        }

        sender.record(VarInt::from_u8(1));
        let batch = rx.try_recv().unwrap();
        let mut dest = IntervalSet::new();
        let id = batch.take(&mut dest).unwrap();
        assert_eq!(id.as_u64(), VarInt::MAX.as_u64() - 1);
        batch.check_and_resubmit(&tx);

        sender.record(VarInt::from_u8(2));
        let batch = rx.try_recv().unwrap();
        dest.clear();
        let id = batch.take(&mut dest).unwrap();
        // Should be VarInt::MAX (saturated)
        assert_eq!(id, VarInt::MAX);
    }
}
