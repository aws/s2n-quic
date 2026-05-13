// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Per-peer send context: crypto state, congestion control, and inflight tracking.
//!
//! Each send socket maintains a map from credentials ID to a Context. The Context owns
//! the sealer for encryption, the CCA for pacing/windowing, and the inflight map for
//! tracking sent packets. Packet assembly (packing multiple frames into a segment,
//! encrypting, and registering in the inflight map) happens here.
//!
//! Contexts participate in three independent timing wheels via separate intrusive links:
//!
//! - **Transmission wheel**: fires at CCA pacing intervals to assemble and send packets.
//! - **PTO wheel**: fires at RTT-multiples for probe timeout / tail loss recovery.
//! - **Idle wheel**: fires after prolonged silence to reclaim resources.

use crate::{
    clock::precision,
    congestion,
    counter::QueueGauge,
    credentials::{self, Credentials},
    intrusive_queue::{self, Queue},
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::{intrusive_queue::unsync, ByteCost, UnboundedSender},
    stream3::{endpoint::inflight, frame::Frame},
};
use core::time::Duration;
use rustc_hash::FxHashMap;
use s2n_quic_core::{
    packet::number::PacketNumberSpace, path::INITIAL_PTO_BACKOFF, recovery::RttEstimator,
    varint::VarInt,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

#[cfg(todo)]
mod tests;

/// Pending frame queue with an integrated wire-cost counter.
///
/// This struct ensures that `byte_cost` always mirrors the true accumulated
/// [`ByteCost`] of every frame in the queue. All mutations (push/pop) go through
/// this type, so callers cannot accidentally desync the counter.
pub(crate) struct PendingFrames {
    queue: Queue<Frame>,
    /// Accumulated wire cost of all frames currently in the queue.
    ///
    /// Wire cost = payload bytes + header metadata bytes (type tag + routing
    /// varints + optional payload-length varint) for every frame.
    byte_cost: usize,
    gauge: QueueGauge,
}

impl PendingFrames {
    #[inline]
    pub fn new(gauge: QueueGauge) -> Self {
        Self {
            queue: Queue::new(),
            byte_cost: 0,
            gauge,
        }
    }

    /// Push a frame onto the back of the queue, updating the cost counter.
    #[inline]
    pub fn push_back(&mut self, frame: intrusive_queue::Entry<Frame>) {
        self.byte_cost += frame.byte_cost() as usize;
        self.gauge.enqueue(1);
        self.queue.push_back(frame);
    }

    /// Push a frame onto the front of the queue, updating the cost counter.
    ///
    /// Only call this with a frame that was just removed via [`pop_front`] — for
    /// example when a frame does not fit in the current segment and must be
    /// returned for the next assembly round. Calling this with a frame that was
    /// *not* previously popped will double-count its wire cost.
    #[inline]
    pub fn push_front(&mut self, frame: intrusive_queue::Entry<Frame>) {
        self.byte_cost += frame.byte_cost() as usize;
        self.gauge.enqueue(1);
        self.queue.push_front(frame);
    }

    /// Remove the next frame from the front of the queue, updating the cost counter.
    #[inline]
    pub fn pop_front(&mut self) -> Option<intrusive_queue::Entry<Frame>> {
        let frame = self.queue.pop_front()?;
        let cost = frame.byte_cost() as usize;
        debug_assert!(
            self.byte_cost >= cost,
            "byte_cost underflow: counter={} frame_cost={}",
            self.byte_cost,
            cost
        );
        self.byte_cost = self.byte_cost.saturating_sub(cost);
        self.gauge.dequeue();
        Some(frame)
    }

    /// Returns `true` if the queue contains no frames.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Returns the accumulated wire cost of all frames currently in the queue.
    #[inline]
    pub fn byte_cost(&self) -> usize {
        self.byte_cost
    }

    /// Append frames from a pre-split queue in O(1).
    ///
    /// `byte_cost` is the pre-computed wire cost for all frames in `queue`, including
    /// per-packet overhead. Callers should use the values returned by
    /// `FrameBatch::into_split` directly.
    #[inline]
    pub fn append_queue(&mut self, mut queue: Queue<Frame>, byte_cost: u64) {
        let count = queue.len() as u64;
        self.byte_cost += byte_cost as usize;
        self.gauge.enqueue(count);
        self.queue.append(&mut queue);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WheelInterest {
    pub transmission: bool,
    pub pto: bool,
    pub idle_timeout: bool,
}

impl WheelInterest {
    /// Route a context into the appropriate wheel sender channels based on interest flags.
    pub fn dispatch(
        self,
        context: Rc<RefCell<Context>>,
        tx_wheel_tx: &mut unsync::Sender<TxWheelAdapter>,
        pto_wheel_tx: &mut unsync::Sender<PtoWheelAdapter>,
        idle_wheel_tx: &mut unsync::Sender<IdleWheelAdapter>,
    ) {
        if self.idle_timeout {
            let _ = UnboundedSender::send(idle_wheel_tx, context.clone());
        }

        if self.pto {
            let _ = UnboundedSender::send(pto_wheel_tx, context.clone());
        }

        if self.transmission {
            let _ = UnboundedSender::send(tx_wheel_tx, context);
        }
    }
}

/// Per-peer send state, one per (credentials_id, send_socket) pair.
///
/// Holds crypto material, congestion control, inflight tracking, and the pending frame
/// queues. Frames are pushed in by the Dispatcher, then `assemble()` is called when the
/// local wheel fires to pack them into encrypted packets.
///
/// Frames are held in two queues based on whether they bypass CWND:
/// - `immediate`: ACK frames (`Control` header) — drained unconditionally by the assembler.
/// - `pending`: all other frames (data, resets, flow control) — subject to CWND gating.
pub(crate) struct Context {
    pub path_secret_entry: Arc<PathSecretEntry>,
    pub sealer: crate::crypto::awslc::seal::Application,
    pub credentials: Credentials,
    /// Next packet number to assign
    pub next_packet_number: VarInt,
    /// Next attempt ID for FlowInit deduplication (per-sender counter)
    pub flow_attempt_id_counter: VarInt,
    pub cca: congestion::Controller,
    pub rtt_estimator: RttEstimator,
    pub inflight: inflight::Map,
    pub pto: Pto,
    /// ACK frames (Control header) that bypass CWND enforcement. Drained unconditionally
    /// by the assembler before data frames.
    pub immediate: PendingFrames,
    /// Data frames subject to CWND. Drained only when the congestion window allows.
    pub pending: PendingFrames,
    /// Index of this socket in the path secret entry's `next_transmission_by_sender` array.
    ///
    /// Used by `publish_next_transmission_time` to write the correct slot so the
    /// load-balancer pick-two logic has up-to-date per-socket load information.
    pub sender_idx: usize,
    /// Intrusive links and target time for the transmission pacing wheel
    pub tx_wheel: WheelLinks,
    /// Intrusive links and target time for the PTO (probe timeout) wheel
    pub pto_wheel: WheelLinks,
    /// Intrusive links and target time for the idle timeout wheel
    pub idle_wheel: WheelLinks,
}

impl Context {
    pub fn new(
        entry: &Arc<PathSecretEntry>,
        inflight_gauge: QueueGauge,
        immediate_gauge: QueueGauge,
        pending_gauge: QueueGauge,
        sender_idx: usize,
    ) -> Self {
        let (sealer, credentials) = entry.reusable_sealer();
        let cca = congestion::Controller::new(entry.max_datagram_size());
        let rtt_estimator = RttEstimator::new(Duration::from_millis(2));
        let inflight = inflight::Map::new(inflight_gauge);

        Self {
            path_secret_entry: entry.clone(),
            sealer,
            credentials,
            next_packet_number: VarInt::ZERO,
            flow_attempt_id_counter: VarInt::ZERO,
            cca,
            rtt_estimator,
            inflight,
            pto: Pto::default(),
            immediate: PendingFrames::new(immediate_gauge),
            pending: PendingFrames::new(pending_gauge),
            sender_idx,
            tx_wheel: WheelLinks::new(),
            pto_wheel: WheelLinks::new(),
            idle_wheel: WheelLinks::new(),
        }
    }

    /// Append all frames from a batch and return wheel interest indicating which wheels
    /// need this context inserted.
    ///
    /// The batch already carries pre-split immediate and pending queues — routing was done
    /// once in the batcher where every frame was already inspected. This method performs
    /// two O(1) queue-append operations.
    pub fn push_batch<Clk: precision::Clock + ?Sized>(
        &mut self,
        batch: super::combinator::FrameBatch,
        clock: &Clk,
    ) -> WheelInterest {
        let (imm_q, imm_cost, pend_q, pend_cost) = batch.into_split();
        if !imm_q.is_empty() {
            self.immediate.append_queue(imm_q, imm_cost);
        }
        if !pend_q.is_empty() {
            self.pending.append_queue(pend_q, pend_cost);
        }
        self.wheel_interest(clock)
    }

    /// Decode an ACK payload and process it against this context's inflight state.
    ///
    /// Each ACK frame in the payload triggers loss detection and CCA updates. Acknowledged
    /// frames go to `acked`, retransmittable lost frames to `lost`, and cancelled/expired
    /// frames to `cancelled`. After all frames are processed, computes and returns a single
    /// `WheelInterest` for rescheduling.
    pub fn process_ack_payload<Clk, Rand>(
        &mut self,
        payload: &mut [u8],
        completed: &mut impl UnboundedSender<intrusive_queue::Entry<Frame>>,
        lost: &mut impl UnboundedSender<intrusive_queue::Entry<Frame>>,
        cancelled: &mut impl UnboundedSender<intrusive_queue::Entry<Frame>>,
        clock: &Clk,
        random: &mut Rand,
    ) -> WheelInterest
    where
        Clk: s2n_quic_core::time::Clock + precision::Clock + ?Sized,
        Rand: crate::random::Generator,
    {
        let frames_iter = crate::packet::control::decoder::ControlFramesMut::new(payload);

        for frame in frames_iter {
            let Ok(frame) = frame else {
                tracing::debug!("failed to decode control frame in ACK payload");
                break;
            };

            match frame {
                s2n_quic_core::frame::FrameMut::Ack(ack_frame) => {
                    super::ack::process_ack(
                        &ack_frame, self, completed, lost, cancelled, clock, random,
                    );
                }
                s2n_quic_core::frame::FrameMut::Padding(_)
                | s2n_quic_core::frame::FrameMut::Ping(_) => {}
                frame => {
                    tracing::debug!(?frame, "unexpected control frame type in ACK payload");
                }
            }
        }

        self.wheel_interest(clock)
    }

    /// Compute wheel interest after a state change
    pub(crate) fn wheel_interest<Clk>(&mut self, clock: &Clk) -> WheelInterest
    where
        Clk: precision::Clock + ?Sized,
    {
        let transmission = if
        // Check we have queued packets
        self.has_pending()
            // Make sure we're not already linked
            && !self.is_tx_scheduled()
            // Check if the congestion controller is allowing a send.
            // TODO (PTO task 4): remove `|| true` once CWND is enforced only for
            // `pending` frames; `immediate` frames always bypass the window.
            && (self.cca.requires_fast_retransmission() || !self.cca.is_congestion_limited() || true)
        {
            let target = self
                .cca
                .earliest_departure_time()
                .map(precision::Timestamp::from);
            // If target time is `None` then the wheel will schedule it immediately
            self.tx_wheel.target_time = target;
            true
        } else {
            false
        };

        let pto = if !self.is_pto_scheduled() {
            self.pto.update_target(clock, &self.rtt_estimator);
            if let Some(target) = self.pto.target_time {
                self.pto_wheel.target_time = Some(target);
                true
            } else {
                false
            }
        } else {
            false
        };

        WheelInterest {
            transmission,
            pto,
            idle_timeout: false,
        }
    }

    /// Pop the next immediate (ACK) frame from the immediate queue.
    #[inline]
    pub fn pop_immediate(&mut self) -> Option<intrusive_queue::Entry<Frame>> {
        self.immediate.pop_front()
    }

    /// Push a frame back to the front of the immediate queue.
    ///
    /// Only call this with a frame just removed via [`pop_immediate`].
    #[inline]
    pub fn push_front_immediate(&mut self, frame: intrusive_queue::Entry<Frame>) {
        self.immediate.push_front(frame);
    }

    /// Pop the next frame from the pending queue.
    #[inline]
    pub fn pop_pending(&mut self) -> Option<intrusive_queue::Entry<Frame>> {
        self.pending.pop_front()
    }

    /// Push a frame back to the front of the pending queue.
    ///
    /// Only call this with a frame just removed via [`pop_pending`].
    #[inline]
    pub fn push_front_pending(&mut self, frame: intrusive_queue::Entry<Frame>) {
        self.pending.push_front(frame);
    }

    /// Push a frame back to the front of whichever queue it belongs to.
    ///
    /// Routes immediate frames (`is_immediate()`) to `immediate` and all other frames to
    /// `pending`. Only call this with a frame just removed via [`pop_immediate`] or
    /// [`pop_pending`].
    #[inline]
    pub fn push_front_frame(&mut self, frame: intrusive_queue::Entry<Frame>) {
        if frame.is_immediate() {
            self.immediate.push_front(frame);
        } else {
            self.pending.push_front(frame);
        }
    }

    /// Publish the estimated next transmission time to the path secret entry.
    ///
    /// Derives the estimate from the current pending wire cost and the CCA bandwidth
    /// sample, then stores it in the path-secret entry so the load-balancer
    /// (`pick_sender_by_next_transmission`) can compare per-socket load.
    ///
    /// Call this whenever the pending queue or CCA state changes.
    #[inline]
    pub fn publish_next_transmission_time(&self, now: s2n_quic_core::time::Timestamp) {
        self.path_secret_entry.update_sender_next_transmission_time(
            self.sender_idx,
            now,
            self.immediate.byte_cost() + self.pending.byte_cost(),
            self.cca.bandwidth(),
        );
    }

    #[inline]
    pub fn has_pending(&self) -> bool {
        !self.immediate.is_empty() || !self.pending.is_empty()
    }

    #[inline]
    pub fn is_tx_scheduled(&self) -> bool {
        self.tx_wheel.links.is_linked()
    }

    #[inline]
    pub fn is_pto_scheduled(&self) -> bool {
        self.pto_wheel.links.is_linked()
    }

    #[inline]
    pub fn is_idle_scheduled(&self) -> bool {
        self.idle_wheel.links.is_linked()
    }
}

// ── Wheel Links ───────────────────────────────────────────────────────────

/// Intrusive links + target time for a single wheel membership.
///
/// Each Context has three of these — one per wheel. The target_time is set before
/// insertion and read by the wheel to determine the correct slot. Once the wheel
/// pops an entry, target_time can be stale — the handler decides whether to act
/// or reinsert with a new target.
pub(crate) struct WheelLinks {
    pub links: intrusive_queue::Links,
    pub target_time: Option<precision::Timestamp>,
}

impl WheelLinks {
    pub const fn new() -> Self {
        Self {
            links: intrusive_queue::Links::new(),
            target_time: None,
        }
    }
}

// ── Wheel Adapters ────────────────────────────────────────────────────────
//
// Each adapter tells the intrusive list infrastructure how to reach the Links
// field for its wheel, and tells the timing wheel how to read/write target_time.
// The pointer type is Rc<RefCell<Context>> for all three.

macro_rules! context_wheel_adapter {
    ($adapter:ident, $field:ident) => {
        pub(crate) struct $adapter;

        impl crate::intrusive_queue::Adapter for $adapter {
            type Value = RefCell<Context>;
            type Target = RefCell<Context>;
            type Pointer = Rc<RefCell<Context>>;

            unsafe fn links(value: *mut Self::Value) -> *mut intrusive_queue::Links {
                core::ptr::addr_of_mut!((*(*value).as_ptr()).$field.links)
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

        impl crate::clock::wheel::WheelAdapter for $adapter {
            unsafe fn target_time(value: *const Self::Value) -> Option<precision::Timestamp> {
                (*value).borrow().$field.target_time
            }

            unsafe fn set_target_time(value: *mut Self::Value, time: precision::Timestamp) {
                (*value).borrow_mut().$field.target_time = Some(time);
            }
        }
    };
}

context_wheel_adapter!(TxWheelAdapter, tx_wheel);
context_wheel_adapter!(PtoWheelAdapter, pto_wheel);
context_wheel_adapter!(IdleWheelAdapter, idle_wheel);

/// PTO (Probe Timeout) state for tail loss recovery.
///
/// When all inflight packets may be lost (no ACKs arriving), PTO fires to send a probe.
/// This ensures the peer generates an ACK, which either confirms delivery or triggers
/// loss detection.
pub(crate) struct Pto {
    pub backoff: u32,
    pub target_time: Option<precision::Timestamp>,
    pub last_sent_time: Option<precision::Timestamp>,
    pub needs_update: bool,
}

impl Default for Pto {
    fn default() -> Self {
        Self {
            backoff: INITIAL_PTO_BACKOFF,
            target_time: None,
            last_sent_time: None,
            needs_update: false,
        }
    }
}

impl Pto {
    pub fn on_packet_sent(&mut self, now: precision::Timestamp) {
        self.last_sent_time = Some(now);
        self.needs_update = true;
    }

    pub fn on_ack_received(&mut self, has_remaining_inflight: bool) {
        self.backoff = INITIAL_PTO_BACKOFF;

        if has_remaining_inflight {
            self.needs_update = true;
        } else {
            self.target_time = None;
            self.needs_update = false;
        }
    }

    /// Returns true if we should send a probe.
    pub fn on_timeout(&mut self) -> bool {
        self.target_time = None;

        if self.needs_update {
            self.needs_update = false;
            return false;
        }

        self.backoff = self.backoff.saturating_mul(2).min(16);
        true
    }

    pub fn update_target<Clk: precision::Clock + ?Sized>(
        &mut self,
        clock: &Clk,
        rtt_estimator: &RttEstimator,
    ) {
        let mut pto_period = rtt_estimator.pto_period(self.backoff, PacketNumberSpace::Initial);
        pto_period = pto_period.max(Duration::from_millis(2));

        let base_time = self.last_sent_time.unwrap_or_else(|| clock.now());
        self.target_time = Some(base_time + pto_period);
    }
}

/// Per-socket cache of send contexts, keyed by credentials ID.
///
/// Each send socket has its own Cache. A single peer always routes through the same
/// send socket (via credential hashing), so there's exactly one Context per peer per socket.
pub(crate) struct Cache {
    contexts: FxHashMap<credentials::Id, Rc<RefCell<Context>>>,
    inflight_gauge: QueueGauge,
    immediate_gauge: QueueGauge,
    pending_gauge: QueueGauge,
    sender_idx: usize,
}

impl Cache {
    pub fn new(counter_registry: &crate::counter::Registry, sender_idx: usize) -> Self {
        Self {
            contexts: FxHashMap::default(),
            inflight_gauge: counter_registry.register_queue_gauge("send.inflight"),
            immediate_gauge: counter_registry.register_queue_gauge("send.immediate"),
            pending_gauge: counter_registry.register_queue_gauge("send.pending"),
            sender_idx,
        }
    }

    pub fn get_or_insert(&mut self, entry: &Arc<PathSecretEntry>) -> Rc<RefCell<Context>> {
        let id = *entry.id();

        self.contexts
            .entry(id)
            .or_insert_with(|| {
                Rc::new(RefCell::new(Context::new(
                    entry,
                    self.inflight_gauge.clone(),
                    self.immediate_gauge.clone(),
                    self.pending_gauge.clone(),
                    self.sender_idx,
                )))
            })
            .clone()
    }

    pub fn get(&self, id: &credentials::Id) -> Option<Rc<RefCell<Context>>> {
        self.contexts.get(id).cloned()
    }
}
