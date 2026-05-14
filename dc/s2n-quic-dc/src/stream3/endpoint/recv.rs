// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub(crate) mod ack_ranges;

use crate::{
    clock::precision,
    credentials::{self, Credentials},
    flow, intrusive_queue,
    path::{self, secret::map::Entry as PathSecretEntry},
    stream3::endpoint::ack::state as ack_state,
};
use core::time::Duration;
use rustc_hash::FxHashMap;
use s2n_quic_core::{frame::ack::EcnCounts, varint::VarInt};
use std::{cell::RefCell, collections::hash_map, rc::Rc, sync::Arc};

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

// ── ACK Wheel Adapter ─────────────────────────────────────────────────────

/// Intrusive links + target time for ACK batching wheel membership.
pub(crate) struct AckWheelLinks {
    pub links: intrusive_queue::Links,
    pub target_time: Option<precision::Timestamp>,
}

impl AckWheelLinks {
    pub const fn new() -> Self {
        Self {
            links: intrusive_queue::Links::new(),
            target_time: None,
        }
    }
}

pub(crate) struct AckWheelAdapter;

impl crate::intrusive_queue::Adapter for AckWheelAdapter {
    type Value = RefCell<Context>;
    type Target = RefCell<Context>;
    type Pointer = Rc<RefCell<Context>>;

    unsafe fn links(value: *mut Self::Value) -> *mut intrusive_queue::Links {
        core::ptr::addr_of_mut!((*(*value).as_ptr()).ack_wheel.links)
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

impl crate::clock::wheel::WheelAdapter for AckWheelAdapter {
    unsafe fn target_time(value: *const Self::Value) -> Option<precision::Timestamp> {
        (*value).borrow().ack_wheel.target_time
    }

    unsafe fn set_target_time(value: *mut Self::Value, time: precision::Timestamp) {
        (*value).borrow_mut().ack_wheel.target_time = Some(time);
    }
}

pub(crate) struct AckBurstAdapter;

impl crate::intrusive_queue::Adapter for AckBurstAdapter {
    type Value = RefCell<Context>;
    type Target = RefCell<Context>;
    type Pointer = Rc<RefCell<Context>>;

    unsafe fn links(value: *mut Self::Value) -> *mut intrusive_queue::Links {
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

/// Attempt deduplication window for tracking seen attempt_ids.
///
/// Uses a sliding window to efficiently deduplicate FlowInit packets within
/// a bounded memory footprint. This is the fast path for recent attempt_ids.
pub(crate) struct AttemptDedup {
    /// Sliding window for recent attempt_ids (same as packet number dedup)
    window: s2n_quic_core::packet::number::SlidingWindow,
}

impl AttemptDedup {
    pub fn new() -> Self {
        Self {
            window: Default::default(),
        }
    }

    /// Check if an attempt_id has been seen before in the recent window.
    ///
    /// Returns:
    /// - Ok(()) if attempt_id is new and within window
    /// - Err(Duplicate) if already seen in window
    /// - Err(TooOld) if outside window (check DashMap or retry)
    pub fn check_attempt_id(&mut self, attempt_id: VarInt) -> Result<(), AttemptDedupError> {
        use s2n_quic_core::packet::number::{PacketNumberSpace, SlidingWindowError};

        let packet_number = PacketNumberSpace::Initial.new_packet_number(attempt_id);
        match self.window.insert(packet_number) {
            Ok(()) => Ok(()),
            Err(SlidingWindowError::TooOld) => Err(AttemptDedupError::TooOld),
            Err(SlidingWindowError::Duplicate) => Err(AttemptDedupError::Duplicate),
        }
    }
}

#[derive(Debug)]
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
    pub dedup_filter: crate::stream::recv::ack::StreamFilter,
    /// Lightweight ACK range tracker for the direct ACK path.
    pub ack_ranges: ack_ranges::AckRanges,
    /// Which local sender_id outgoing ACKs for this peer route through.
    pub dest_sender_id: VarInt,
    /// Accumulated ECN counts for received packets, reported back to the sender
    /// in each ACK frame so the sender can validate ECN support and detect congestion.
    pub ecn_counts: EcnCounts,
    pub idle_timer: s2n_quic_core::time::Timer,
    pub last_activity: s2n_quic_core::time::Timestamp,
    pub ack_state: AckState,
    pub attempt_dedup: AttemptDedup,
    /// Map from stream_id to allocated queue_id for this sender.
    /// Shared with queue handles so they can remove entries when closed.
    pub flows: flow::Tracker,
    /// Intrusive links for ACK batching wheel
    pub ack_wheel: AckWheelLinks,
    /// Intrusive links for recv-worker pending-ACK burst queue membership.
    pub ack_burst: intrusive_queue::Links,
}

impl Context {
    pub fn new<Clk>(
        path_entry: Arc<PathSecretEntry>,
        remote_sender_id: VarInt,
        dest_sender_id: VarInt,
        opener: crate::crypto::awslc::open::Application,
        key_id: VarInt,
        clock: &Clk,
        idle_timeout: Duration,
    ) -> Self
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        let now = clock.get_time();
        let mut idle_timer = s2n_quic_core::time::Timer::default();
        idle_timer.set(now + idle_timeout);

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
            idle_timer,
            last_activity: now,
            ack_state: AckState::Idle,
            attempt_dedup: AttemptDedup::new(),
            flows,
            ack_wheel: AckWheelLinks::new(),
            ack_burst: intrusive_queue::Links::new(),
        }
    }

    pub fn update_activity<Clk>(&mut self, clock: &Clk, idle_timeout: Duration)
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        let now = clock.get_time();
        self.last_activity = now;
        self.idle_timer.set(now + idle_timeout);
    }

    pub fn is_expired<Clk>(&mut self, clock: &Clk) -> bool
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        self.idle_timer.poll_expiration(clock.get_time()).is_ready()
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
            let _ = self.ack_state.on_empty();
            return None;
        };
        let Some(largest_recv_time) = self.ack_ranges.largest_recv_time() else {
            let _ = self.ack_state.on_empty();
            return None;
        };

        let _ = self.ack_state.on_flush();

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
        debug_assert!(
            self.ack_state.is_flushed() || self.ack_state.is_flushed_stale(),
            "ack completion should only be observed for Flushed/FlushedStale states"
        );
        let _ = self.ack_state.on_flush_complete();
        self.encode_and_flush(recv_worker_id)
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
    pub idle_timeout: Duration,
    pub worker_id: usize,
}

impl Cache {
    pub fn new(idle_timeout: Duration, worker_id: usize) -> Self {
        Self {
            senders: FxHashMap::default(),
            idle_timeout,
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
        Clk: s2n_quic_core::time::Clock + ?Sized,
        Route: super::routing::SenderRoute,
    {
        let key = Key {
            id: credentials.id,
            remote_sender_id,
        };

        Some(match self.senders.entry(key) {
            hash_map::Entry::Occupied(entry) => (entry.get().clone(), true),
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
                    clock,
                    self.idle_timeout,
                )));
                entry.insert(ctx.clone());
                (ctx, false)
            }
        })
    }

    #[expect(dead_code)] // TODO implement expiration
    pub fn cleanup_expired<Clk>(&mut self, clock: &Clk)
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        self.senders.retain(|_, state| !state.borrow_mut().is_expired(clock));
    }
}
