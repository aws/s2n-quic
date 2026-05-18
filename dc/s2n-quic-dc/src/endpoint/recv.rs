// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials::{self, Credentials},
    flow, intrusive,
    path::{self, secret::map::Entry as PathSecretEntry},
    stream::endpoint::ack::state as ack_state,
};
use rustc_hash::FxHashMap;
use s2n_quic_core::{frame::ack::EcnCounts, varint::VarInt};
use std::{cell::RefCell, collections::hash_map, rc::Rc, sync::Arc};

pub(crate) mod ack_ranges;
mod dedup;

/// ACK transmission state machine.
///
/// ```text
/// Idle → Scheduled (ack-eliciting packet received)
/// Scheduled → Flushed (submission sent to send worker)
/// Flushed → Idle (completion returned, no new data)
/// Flushed → FlushedStale (ack-eliciting packet received while completion is in flight)
/// FlushedStale → Scheduled (completion returned, needs re-flush)
/// ```
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AckState {
    /// No ACK pending and none in flight.
    #[default]
    Idle,
    /// ACK pending — new ack-eliciting packets have arrived and need acknowledgment.
    Scheduled,
    /// ACK submission is in the send pipeline. New packets update the shared state
    /// but don't produce another submission until the completion returns.
    Flushed,
    /// ACK completion is in flight and new ack-eliciting data arrived.
    FlushedStale,
}

impl AckState {
    s2n_quic_core::state::is!(is_scheduled, Scheduled);
    s2n_quic_core::state::is!(is_flushed, Flushed);
    s2n_quic_core::state::is!(is_flushed_stale, FlushedStale);

    s2n_quic_core::state::event! {
        /// An ack-eliciting packet was received.
        on_ack_eliciting(
            Idle | Scheduled => Scheduled,
            Flushed | FlushedStale => FlushedStale,
        );
        /// The ACK submission was sent to the send worker.
        on_flush(Scheduled => Flushed);
        /// ACK flush completion returned.
        ///
        /// If no new packets arrived while in flight, transition back to idle.
        /// If packets arrived (FlushedStale), transition to scheduled so the
        /// completion path can re-encode and resubmit.
        on_flush_complete(
            Flushed => Idle,
            FlushedStale => Scheduled,
        );
        /// Scheduled but nothing to encode — reset to idle.
        on_empty(Scheduled => Idle);
    }
}

pub(crate) struct AckBurstAdapter;

impl crate::intrusive::Adapter for AckBurstAdapter {
    type Value = RefCell<Context>;
    type Target = RefCell<Context>;
    type Pointer = Rc<RefCell<Context>>;

    unsafe fn links(value: *mut Self::Value) -> *mut intrusive::Links {
        core::ptr::addr_of_mut!((*(*value).as_ptr()).ack_burst)
    }

    unsafe fn target(value: *mut Self::Value) -> *mut Self::Target {
        value
    }

    fn as_ptr(ptr: &Self::Pointer) -> *const Self::Value {
        Rc::as_ptr(ptr)
    }

    fn into_raw(ptr: Self::Pointer) -> *mut Self::Value {
        Rc::into_raw(ptr) as *mut Self::Value
    }

    unsafe fn from_raw(ptr: *mut Self::Value) -> Self::Pointer {
        Rc::from_raw(ptr)
    }
}

/// Attempt deduplication using a circular bitmap.
///
/// Tracks up to `CAPACITY` recent attempt_ids without shifting memory. The bitmap
/// is indexed relative to `right_edge` using modular arithmetic.
pub(crate) struct AttemptDedup {
    bitmap: [u64; Self::WORDS],
    right_edge: Option<u64>,
}

impl AttemptDedup {
    const WORDS: usize = 32;
    const CAPACITY: u64 = (Self::WORDS as u64) * 64;

    pub fn new() -> Self {
        Self {
            bitmap: [0; Self::WORDS],
            right_edge: None,
        }
    }

    pub fn check_attempt_id(&mut self, attempt_id: VarInt) -> Result<(), AttemptDedupError> {
        let id = attempt_id.as_u64();

        let Some(edge) = self.right_edge else {
            self.right_edge = Some(id);
            return Ok(());
        };

        if id == edge {
            return Err(AttemptDedupError::Duplicate);
        }

        if id > edge {
            let advance = id - edge;
            self.clear_range(edge + 1, advance);
            // The old right_edge moves into the bitmap (if still in window)
            if advance < Self::CAPACITY {
                let (word, mask) = Self::index(edge);
                self.bitmap[word] |= mask;
            }
            self.right_edge = Some(id);
            return Ok(());
        }

        // id < edge
        let offset = edge - id;
        if offset >= Self::CAPACITY {
            return Err(AttemptDedupError::TooOld);
        }

        let (word, mask) = Self::index(id);
        if self.bitmap[word] & mask != 0 {
            return Err(AttemptDedupError::Duplicate);
        }

        self.bitmap[word] |= mask;
        Ok(())
    }

    /// Clear `count` bit positions starting at `from`.
    fn clear_range(&mut self, from: u64, count: u64) {
        if count >= Self::CAPACITY {
            self.bitmap = [0; Self::WORDS];
            return;
        }
        for i in 0..count {
            let (word, mask) = Self::index(from + i);
            self.bitmap[word] &= !mask;
        }
    }

    #[inline]
    fn index(id: u64) -> (usize, u64) {
        let bit = (id % Self::CAPACITY) as usize;
        let word = bit / 64;
        let mask = 1u64 << (bit % 64);
        (word, mask)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum AttemptDedupError {
    /// Attempt ID already seen (duplicate)
    Duplicate,
    /// Attempt ID too old (outside window) - need to check DashMap or send retry
    TooOld,
}

/// Cached crypto state and ACK tracking for a peer.
///
/// Keyed by (credentials.id, source_sender_id) because ACK spaces and
/// deduplication windows are per-sender, not per-peer.
pub(crate) struct Context {
    pub path_entry: Arc<PathSecretEntry>,
    /// The remote peer's sender_id (from the packet routing info).
    /// Used as `dest_sender_id` in outgoing ACK frames.
    pub remote_sender_id: VarInt,
    // TODO: Support key rotation by maintaining multiple openers indexed by key_id.
    // Currently we only track the latest key, which means packets with old key_ids
    // after rotation will fail to decrypt. Need to maintain a small cache of recent
    // openers (e.g., HashMap<VarInt, Opener>) to handle in-flight packets during rotation.
    pub opener: crate::crypto::awslc::open::Application,
    /// The key_id this opener corresponds to
    #[expect(dead_code)] // TODO implement key rotation
    pub current_key_id: VarInt,
    /// Sliding window for packet number deduplication.
    pub dedup_filter: dedup::StreamFilter,
    /// Lightweight ACK range tracker for the direct ACK path.
    pub ack_ranges: ack_ranges::AckRanges,
    /// Which local sender_id outgoing ACKs for this peer route through.
    pub dest_sender_id: VarInt,
    /// Accumulated ECN counts for received packets, reported back to the sender
    /// in each ACK frame so the sender can validate ECN support and detect congestion.
    pub ecn_counts: EcnCounts,
    pub idle_wheel: crate::time::wheel::WheelLinks,
    pub created_at: crate::time::precision::Timestamp,
    pub ack_state: AckState,
    pub attempt_dedup: AttemptDedup,
    /// Map from stream_id to allocated queue_id for this sender.
    /// Shared with queue handles so they can remove entries when closed.
    pub flows: flow::Tracker,
    /// Intrusive links for recv-worker pending-ACK burst queue membership.
    pub ack_burst: intrusive::Links,
}

impl Context {
    #[inline]
    pub fn key(&self) -> Key {
        Key {
            id: *self.path_entry.id(),
            remote_sender_id: self.remote_sender_id,
        }
    }

    #[inline]
    pub fn invariants(&self) {
        if cfg!(debug_assertions) {
            assert_eq!(
                self.flows.credential_id(),
                self.path_entry.id(),
                "flow tracker credential does not match path entry id"
            );

            if self.ack_ranges.is_empty() {
                assert!(
                    self.ack_ranges.largest_recv_time().is_none(),
                    "largest_recv_time present while ack_ranges is empty"
                );
            } else {
                assert!(
                    self.ack_ranges.largest_recv_time().is_some(),
                    "ack_ranges has packets but largest_recv_time is missing"
                );
            }

            if self.ack_state.is_scheduled()
                || self.ack_state.is_flushed()
                || self.ack_state.is_flushed_stale()
            {
                assert!(
                    !self.ack_ranges.is_empty(),
                    "ack_state indicates pending/inflight ACK but no ranges are present"
                );
                assert!(
                    self.ack_ranges.largest_recv_time().is_some(),
                    "ack_state indicates pending/inflight ACK but largest_recv_time is missing"
                );
            }
        }
    }

    pub fn new(
        path_entry: Arc<PathSecretEntry>,
        remote_sender_id: VarInt,
        dest_sender_id: VarInt,
        opener: crate::crypto::awslc::open::Application,
        key_id: VarInt,
        now: crate::time::precision::Timestamp,
    ) -> Self {
        let idle_timeout = path_entry.idle_timeout();
        let mut idle_wheel = crate::time::wheel::WheelLinks::new();
        idle_wheel.target_time = Some(now + idle_timeout);

        let flows = flow::Tracker::new(*path_entry.id());

        Self {
            path_entry,
            remote_sender_id,
            opener,
            current_key_id: key_id,
            dedup_filter: Default::default(),
            ack_ranges: Default::default(),
            dest_sender_id,
            ecn_counts: Default::default(),
            idle_wheel,
            created_at: now,
            ack_state: AckState::Idle,
            attempt_dedup: AttemptDedup::new(),
            flows,
            ack_burst: intrusive::Links::new(),
        }
    }

    /// Encode the current ACK state and produce a direct submission for the send worker.
    ///
    /// Only produces a submission when ack_state is Scheduled (new packets arrived
    /// since the last submission). Transitions to Flushed after submitting to enforce
    /// at-most-one-in-flight. When the completion returns, the recv worker checks
    /// whether ack_state went back to Scheduled (new packets arrived) and re-submits.
    ///
    /// Returns `None` if there are no ranges or an ACK is already in flight.
    pub fn encode_and_flush(&mut self, recv_worker_id: usize) -> Option<ack_state::Submission> {
        if !self.ack_state.is_scheduled() {
            return None;
        }

        let has_ecn = self.ecn_counts.as_option().is_some();
        let mtu = self.path_entry.max_datagram_size() as usize;
        let max_body_len = mtu.saturating_sub(ack_ranges::PACKET_OVERHEAD);
        let Some(body) = self
            .ack_ranges
            .encode_body(self.ecn_counts.as_option(), max_body_len)
        else {
            let transition = self.ack_state.on_empty();
            debug_assert!(
                transition.is_ok(),
                "on_empty transition failed from Scheduled"
            );
            self.invariants();
            return None;
        };
        let Some(largest_recv_time) = self.ack_ranges.largest_recv_time() else {
            let transition = self.ack_state.on_empty();
            debug_assert!(
                transition.is_ok(),
                "on_empty transition failed from Scheduled"
            );
            self.invariants();
            return None;
        };

        let transition = self.ack_state.on_flush();
        debug_assert!(
            transition.is_ok(),
            "on_flush transition failed from Scheduled"
        );
        self.invariants();

        Some(ack_state::Submission {
            body,
            largest_recv_time: largest_recv_time.into(),
            has_ecn,
            path_secret_entry: self.path_entry.clone(),
            local_sender_id: self.dest_sender_id,
            remote_sender_id: self.remote_sender_id,
            recv_worker_id,
        })
    }

    pub fn on_ack_completion(&mut self, recv_worker_id: usize) -> Option<ack_state::Submission> {
        if !(self.ack_state.is_flushed() || self.ack_state.is_flushed_stale()) {
            tracing::warn!(
                ?self.ack_state,
                "ack completion observed while context is not in a flushed state"
            );
            self.invariants();
            return None;
        }
        let transition = self.ack_state.on_flush_complete();
        debug_assert!(
            transition.is_ok(),
            "on_flush_complete transition failed from Flushed/FlushedStale"
        );
        let submission = self.encode_and_flush(recv_worker_id);
        self.invariants();
        submission
    }
}

/// Key for sender state lookup — keyed by peer identity (stable) + sender_id,
/// NOT by full Credentials (which includes the per-packet key_id).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Key {
    pub id: credentials::Id,
    pub remote_sender_id: VarInt,
}

impl core::hash::Hash for Key {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        state.write_u64(self.id.to_hash() ^ self.remote_sender_id.as_u64());
    }
}

/// Per-worker sender state cache.
pub(crate) struct Cache {
    pub senders: FxHashMap<Key, Rc<RefCell<Context>>>,
    pub worker_id: usize,
}

impl Cache {
    pub fn new(worker_id: usize) -> Self {
        Self {
            senders: FxHashMap::default(),
            worker_id,
        }
    }

    #[inline]
    pub fn get_or_insert<Clk, Route>(
        &mut self,
        credentials: &Credentials,
        remote_sender_id: VarInt,
        path_secret_map: &path::secret::map::Map,
        clock: &Clk,
        control_out: &mut Vec<u8>,
        route: &Route,
    ) -> Option<(Rc<RefCell<Context>>, bool)>
    where
        Clk: crate::time::precision::Clock + ?Sized,
        Route: super::routing::SenderRoute,
    {
        let key = Key {
            id: credentials.id,
            remote_sender_id,
        };

        Some(match self.senders.entry(key) {
            hash_map::Entry::Occupied(entry) => {
                let ctx = entry.get().clone();
                {
                    let ctx_ref = ctx.borrow();
                    debug_assert_eq!(
                        ctx_ref.key(),
                        key,
                        "recv cache key does not match cached context key"
                    );
                    ctx_ref.invariants();
                }
                (ctx, true)
            }
            hash_map::Entry::Vacant(entry) => {
                tracing::debug!(%credentials, %remote_sender_id, worker_id = self.worker_id, "opener_for_credentials");
                let (opener, path_entry) =
                    path_secret_map.opener_for_credentials(credentials, None, control_out)?;

                let dest_sender_id = route.sender_id_for_ack(&credentials.id, remote_sender_id);

                let ctx = Rc::new(RefCell::new(Context::new(
                    path_entry,
                    remote_sender_id,
                    dest_sender_id,
                    opener,
                    credentials.key_id,
                    clock.now(),
                )));
                ctx.borrow().invariants();
                entry.insert(ctx.clone());
                (ctx, false)
            }
        })
    }

    pub fn remove(&mut self, key: &Key) {
        self.senders.remove(key);
    }

    pub fn invalidate_by_id(&mut self, id: &credentials::Id) {
        let before = self.senders.len();
        self.senders.retain(|key, _| key.id != *id);
        let removed = before - self.senders.len();
        tracing::debug!(%id, removed, worker_id = self.worker_id, "invalidating recv contexts");
    }
}

crate::context_wheel_adapter!(IdleWheelAdapter, Context, idle_wheel);

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;
    use std::collections::VecDeque;

    fn v(n: u64) -> VarInt {
        VarInt::new(n).unwrap()
    }

    /// Oracle implementation using a VecDeque with the same capacity semantics.
    struct Oracle {
        seen: VecDeque<u64>,
        right_edge: Option<u64>,
    }

    impl Oracle {
        fn new() -> Self {
            Self {
                seen: VecDeque::new(),
                right_edge: None,
            }
        }

        fn check(&mut self, id: u64) -> Result<(), AttemptDedupError> {
            let Some(edge) = self.right_edge else {
                self.right_edge = Some(id);
                self.seen.push_back(id);
                return Ok(());
            };

            if id > edge {
                self.right_edge = Some(id);
            }

            // Evict entries that are now too old
            let new_edge = self.right_edge.unwrap();
            while let Some(&oldest) = self.seen.front() {
                if new_edge - oldest >= AttemptDedup::CAPACITY {
                    self.seen.pop_front();
                } else {
                    break;
                }
            }

            if new_edge - id >= AttemptDedup::CAPACITY {
                return Err(AttemptDedupError::TooOld);
            }

            if self.seen.contains(&id) {
                return Err(AttemptDedupError::Duplicate);
            }

            self.seen.push_back(id);
            Ok(())
        }
    }

    #[test]
    fn first_insert_succeeds() {
        let mut dedup = AttemptDedup::new();
        assert!(dedup.check_attempt_id(v(0)).is_ok());
    }

    #[test]
    fn duplicate_right_edge() {
        let mut dedup = AttemptDedup::new();
        assert!(dedup.check_attempt_id(v(5)).is_ok());
        assert_eq!(
            dedup.check_attempt_id(v(5)).unwrap_err(),
            AttemptDedupError::Duplicate
        );
    }

    #[test]
    fn duplicate_within_window() {
        let mut dedup = AttemptDedup::new();
        assert!(dedup.check_attempt_id(v(10)).is_ok());
        assert!(dedup.check_attempt_id(v(8)).is_ok());
        assert_eq!(
            dedup.check_attempt_id(v(8)).unwrap_err(),
            AttemptDedupError::Duplicate
        );
    }

    #[test]
    fn too_old() {
        let mut dedup = AttemptDedup::new();
        assert!(dedup
            .check_attempt_id(v(AttemptDedup::CAPACITY + 10))
            .is_ok());
        assert_eq!(
            dedup.check_attempt_id(v(0)).unwrap_err(),
            AttemptDedupError::TooOld
        );
    }

    #[test]
    fn advance_clears_old_bits() {
        let mut dedup = AttemptDedup::new();
        assert!(dedup.check_attempt_id(v(5)).is_ok());
        assert!(dedup.check_attempt_id(v(3)).is_ok());

        assert!(dedup
            .check_attempt_id(v(5 + AttemptDedup::CAPACITY + 1))
            .is_ok());

        assert_eq!(
            dedup.check_attempt_id(v(3)).unwrap_err(),
            AttemptDedupError::TooOld
        );
        assert_eq!(
            dedup.check_attempt_id(v(5)).unwrap_err(),
            AttemptDedupError::TooOld
        );
    }

    #[test]
    fn advance_clears_reused_positions() {
        let mut dedup = AttemptDedup::new();
        assert!(dedup.check_attempt_id(v(0)).is_ok());
        assert!(dedup.check_attempt_id(v(1)).is_ok());

        let wrap = AttemptDedup::CAPACITY;
        assert!(dedup.check_attempt_id(v(1 + wrap)).is_ok());
        assert!(dedup.check_attempt_id(v(wrap)).is_ok());
    }

    #[test]
    fn sequential_inserts() {
        let mut dedup = AttemptDedup::new();
        for i in 0..AttemptDedup::CAPACITY * 3 {
            assert!(dedup.check_attempt_id(v(i)).is_ok(), "failed at {i}");
        }
    }

    #[test]
    fn out_of_order_within_window() {
        let mut dedup = AttemptDedup::new();
        assert!(dedup.check_attempt_id(v(100)).is_ok());
        assert!(dedup.check_attempt_id(v(50)).is_ok());
        assert!(dedup.check_attempt_id(v(75)).is_ok());
        assert!(dedup.check_attempt_id(v(99)).is_ok());

        assert_eq!(
            dedup.check_attempt_id(v(50)).unwrap_err(),
            AttemptDedupError::Duplicate
        );
        assert_eq!(
            dedup.check_attempt_id(v(100)).unwrap_err(),
            AttemptDedupError::Duplicate
        );
    }

    #[test]
    fn fuzz_matches_oracle() {
        check!().with_type::<Vec<u16>>().for_each(|ops| {
            let mut dedup = AttemptDedup::new();
            let mut oracle = Oracle::new();

            for &id in ops.iter() {
                let id = id as u64;
                let actual = dedup.check_attempt_id(v(id));
                let expected = oracle.check(id);

                assert_eq!(
                    actual.is_ok(),
                    expected.is_ok(),
                    "mismatch at id={id}: actual={actual:?} expected={expected:?}"
                );
                if let (Err(a), Err(e)) = (&actual, &expected) {
                    assert_eq!(a, e, "error kind mismatch at id={id}");
                }
            }
        });
    }
}
