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
    pub server_state: Arc<super::server::ServerState>,
    pub path_entry: Arc<PathSecretEntry>,
}

impl FreedBatch {
    fn freed_state(&self) -> &Mutex<FreedState> {
        &self.server_state.freed.state
    }

    /// Swap the pending freed set with the caller's buffer, returning a request_id.
    ///
    /// After this call, `dest` contains the IDs to encode.  The caller should
    /// clear `dest` after encoding to preserve its allocation for the next swap.
    /// Returns `None` if there is nothing to send.
    pub fn take(&self, dest: &mut IntervalSet<VarInt>) -> Option<VarInt> {
        let mut state = self.freed_state().lock().unwrap();
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
        let mut state = self.freed_state().lock().unwrap();
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
        let mut state = self.freed_state().lock().unwrap();
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


pub struct FreedInner {
    state: Mutex<FreedState>,
}

impl FreedInner {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(FreedState {
                freed: IntervalSet::new(),
                next_request_id: VarInt::from_u8(0),
                in_flight: false,
            }),
        }
    }

    /// Record that `queue_id` has been freed and, if no token is in-flight,
    /// submit a fresh batch token via `endpoint_tx`.
    ///
    /// `server_state` is an Arc clone of the owning ServerState (which contains
    /// this FreedInner). This is the single Arc that keeps everything alive.
    pub fn record(
        &self,
        queue_id: VarInt,
        server_state: &Arc<super::server::ServerState>,
        path_entry: &Arc<PathSecretEntry>,
        endpoint_tx: &FreedBatchTx,
    ) {
        let mut state = self.state.lock().unwrap();

        let _ = state.freed.insert_value(queue_id);

        if state.in_flight {
            return;
        }

        state.in_flight = true;
        drop(state);

        let token = FreedBatch {
            server_state: server_state.clone(),
            path_entry: path_entry.clone(),
        };
        if endpoint_tx.send(token).is_err() {
            self.state.lock().unwrap().in_flight = false;
        }
    }
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


#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::server::ServerState;
    use s2n_quic_core::varint::VarInt;

    fn test_setup() -> (Arc<ServerState>, Arc<PathSecretEntry>, FreedBatchTx, FreedBatchRx) {
        let (tx, rx) = freed_batch_channel();
        let path_entry = PathSecretEntry::builder("127.0.0.1:4433".parse().unwrap()).build();
        let state = Arc::new(ServerState::new(VarInt::from_u8(100)));
        (state, path_entry, tx, rx)
    }

    #[test]
    fn record_first_submits_token() {
        let (state, path_entry, tx, mut rx) = test_setup();
        state.freed.record(VarInt::from_u8(5), &state, &path_entry, &tx);
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn record_second_no_resubmit() {
        let (state, path_entry, tx, mut rx) = test_setup();
        state.freed.record(VarInt::from_u8(5), &state, &path_entry, &tx);
        state.freed.record(VarInt::from_u8(6), &state, &path_entry, &tx);
        assert!(rx.try_recv().is_ok());
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn take_drains_accumulated() {
        let (state, path_entry, tx, mut rx) = test_setup();
        state.freed.record(VarInt::from_u8(5), &state, &path_entry, &tx);
        state.freed.record(VarInt::from_u8(6), &state, &path_entry, &tx);
        state.freed.record(VarInt::from_u8(7), &state, &path_entry, &tx);
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
        let (state, path_entry, tx, mut rx) = test_setup();
        state.freed.record(VarInt::from_u8(1), &state, &path_entry, &tx);
        let batch = rx.try_recv().unwrap();
        let mut dest = IntervalSet::new();
        batch.take(&mut dest);
        let result = batch.take(&mut dest);
        assert!(result.is_none());
    }

    #[test]
    fn take_request_id_increments() {
        let (state, path_entry, tx, mut rx) = test_setup();

        state.freed.record(VarInt::from_u8(1), &state, &path_entry, &tx);
        let batch = rx.try_recv().unwrap();
        let mut dest = IntervalSet::new();
        let id0 = batch.take(&mut dest).unwrap();
        assert_eq!(id0, VarInt::from_u8(0));
        batch.check_and_resubmit(&tx);

        state.freed.record(VarInt::from_u8(2), &state, &path_entry, &tx);
        let batch = rx.try_recv().unwrap();
        dest.clear();
        let id1 = batch.take(&mut dest).unwrap();
        assert_eq!(id1, VarInt::from_u8(1));
    }

    #[test]
    fn put_back_merges() {
        let (state, path_entry, tx, mut rx) = test_setup();
        state.freed.record(VarInt::from_u8(10), &state, &path_entry, &tx);
        state.freed.record(VarInt::from_u8(20), &state, &path_entry, &tx);
        let batch = rx.try_recv().unwrap();

        let mut dest = IntervalSet::new();
        batch.take(&mut dest);
        assert_eq!(dest.count(), 2);

        let mut remainder = IntervalSet::new();
        let _ = remainder.insert_value(VarInt::from_u8(20));
        batch.put_back(&mut remainder);
        assert!(remainder.is_empty());

        state.freed.record(VarInt::from_u8(30), &state, &path_entry, &tx);

        dest.clear();
        let id = batch.take(&mut dest);
        assert!(id.is_some());
        assert!(dest.contains(&VarInt::from_u8(20)));
        assert!(dest.contains(&VarInt::from_u8(30)));
    }

    #[test]
    fn check_and_resubmit_clears_when_empty() {
        let (state, path_entry, tx, mut rx) = test_setup();

        state.freed.record(VarInt::from_u8(1), &state, &path_entry, &tx);
        let batch = rx.try_recv().unwrap();
        let mut dest = IntervalSet::new();
        batch.take(&mut dest);
        batch.check_and_resubmit(&tx);

        state.freed.record(VarInt::from_u8(2), &state, &path_entry, &tx);
        assert!(rx.try_recv().is_ok());
    }

    #[test]
    fn check_and_resubmit_resubmits_when_more() {
        let (state, path_entry, tx, mut rx) = test_setup();
        let (tx2, mut rx2) = freed_batch_channel();

        state.freed.record(VarInt::from_u8(1), &state, &path_entry, &tx);
        let batch = rx.try_recv().unwrap();
        let mut dest = IntervalSet::new();
        batch.take(&mut dest);

        state.freed.record(VarInt::from_u8(2), &state, &path_entry, &tx);

        batch.check_and_resubmit(&tx2);
        assert!(rx2.try_recv().is_ok());
    }

    #[test]
    fn record_after_channel_closed() {
        let (state, path_entry, tx, rx) = test_setup();
        drop(rx);
        state.freed.record(VarInt::from_u8(1), &state, &path_entry, &tx);
        let inner_state = state.freed.state.lock().unwrap();
        assert!(!inner_state.in_flight);
    }

    #[test]
    fn request_id_saturates() {
        let (state, path_entry, tx, mut rx) = test_setup();

        {
            let mut inner = state.freed.state.lock().unwrap();
            inner.next_request_id = VarInt::new(VarInt::MAX.as_u64() - 1).unwrap();
        }

        state.freed.record(VarInt::from_u8(1), &state, &path_entry, &tx);
        let batch = rx.try_recv().unwrap();
        let mut dest = IntervalSet::new();
        let id = batch.take(&mut dest).unwrap();
        assert_eq!(id.as_u64(), VarInt::MAX.as_u64() - 1);
        batch.check_and_resubmit(&tx);

        state.freed.record(VarInt::from_u8(2), &state, &path_entry, &tx);
        let batch = rx.try_recv().unwrap();
        dest.clear();
        let id = batch.take(&mut dest).unwrap();
        assert_eq!(id, VarInt::MAX);
    }
}
