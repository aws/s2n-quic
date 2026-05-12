// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    byte_vec::ByteVec,
    clock::precision,
    credentials::{self, Credentials},
    flow, intrusive_queue,
    path::{self, secret::map::Entry as PathSecretEntry},
    stream3::frame::{self, Frame, Header},
};
use core::time::Duration;
use s2n_codec::EncoderValue as _;
use s2n_quic_core::{frame::ack::EcnCounts, varint::VarInt};
use std::{
    cell::RefCell,
    collections::{hash_map, HashMap},
    rc::Rc,
    sync::Arc,
};

/// ACK transmission state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AckState {
    /// No ACK pending.
    Idle,
    /// ACK pending — context is in the ACK wheel waiting for the batching delay.
    Scheduled,
    /// ACK was sent early (threshold exceeded). Wheel fires as a no-op.
    Flushed,
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
    pub current_key_id: VarInt,
    /// ACK space for tracking received packets (spans all key_ids for this peer)
    pub ack_space: crate::stream::recv::ack::Space,
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
}

impl Context {
    pub fn new<Clk>(
        path_entry: Arc<PathSecretEntry>,
        remote_sender_id: VarInt,
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
            ack_space: Default::default(),
            ecn_counts: Default::default(),
            idle_timer,
            last_activity: now,
            ack_state: AckState::Idle,
            attempt_dedup: AttemptDedup::new(),
            flows,
            ack_wheel: AckWheelLinks::new(),
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

    pub fn should_transmit(&self) -> bool {
        self.ack_state == AckState::Scheduled
    }

    /// Generate an ACK frame for submission through the frame pipeline.
    ///
    /// Returns `None` if the ack_space has nothing to acknowledge.
    /// Transitions ack_state from Scheduled to Idle on success.
    pub fn generate_ack_frame<Clk, Route>(&mut self, clock: &Clk, route: &Route) -> Option<Frame>
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
        Route: super::routing::SenderRoute,
    {
        let mtu = 1400u16;
        let (ack_frame, _encoding_size) =
            self.ack_space
                .encoding(VarInt::ZERO, self.ecn_counts.as_option(), mtu, clock);

        let ack_frame = ack_frame?;

        let payload: ByteVec = ack_frame.encode_to_vec().into();

        self.ack_state = AckState::Idle;

        // Consistently route through the same local sender id so RTTs are accurate
        // let local_sender_id = route.sender_id_for_ack(self.path_entry.id(), self.remote_sender_id);
        let local_sender_id = VarInt::MAX;

        Some(Frame {
            header: Header::Control {
                dest_sender_id: self.remote_sender_id,
            },
            source_sender_id: local_sender_id,
            payload,
            path_secret_entry: self.path_entry.clone(),
            completion: None,
            status: Default::default(),
            ttl: frame::DEFAULT_TTL,
            transmission_time: None,
        })
    }
}

/// Key for sender state lookup — keyed by peer identity (stable) + sender_id,
/// NOT by full Credentials (which includes the per-packet key_id).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    pub id: credentials::Id,
    pub remote_sender_id: VarInt,
}

/// Per-worker sender state cache.
pub(crate) struct Cache {
    pub senders: HashMap<Key, Context>,
    pub idle_timeout: Duration,
    pub worker_id: usize,
}

impl Cache {
    pub fn new(idle_timeout: Duration, worker_id: usize) -> Self {
        Self {
            senders: HashMap::new(),
            idle_timeout,
            worker_id,
        }
    }

    #[track_caller]
    pub fn get_or_insert<Clk>(
        &mut self,
        credentials: &Credentials,
        remote_sender_id: VarInt,
        path_secret_map: &path::secret::map::Map,
        clock: &Clk,
        control_out: &mut Vec<u8>,
    ) -> Option<&mut Context>
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        let key = Key {
            id: credentials.id,
            remote_sender_id,
        };

        Some(match self.senders.entry(key) {
            hash_map::Entry::Occupied(entry) => entry.into_mut(),
            hash_map::Entry::Vacant(entry) => {
                tracing::debug!(%credentials, %remote_sender_id, caller = %core::panic::Location::caller(), worker_id = self.worker_id, "opener_for_credentials");
                let (opener, path_entry) =
                    path_secret_map.opener_for_credentials(credentials, None, control_out)?;

                entry.insert(Context::new(
                    path_entry,
                    remote_sender_id,
                    opener,
                    credentials.key_id,
                    clock,
                    self.idle_timeout,
                ))
            }
        })
    }

    pub fn cleanup_expired<Clk>(&mut self, clock: &Clk)
    where
        Clk: s2n_quic_core::time::Clock + ?Sized,
    {
        self.senders.retain(|_, state| !state.is_expired(clock));
    }
}
