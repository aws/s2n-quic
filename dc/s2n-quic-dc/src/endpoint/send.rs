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
    congestion,
    counter::QueueGauge,
    credentials::{self, Credentials},
    endpoint::frame::{self, Frame, Priority},
    intrusive::{self, Queue},
    msg::segment,
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::{ByteCost, UnboundedSender},
    stream::endpoint::{inflight, msg},
    time::precision,
};
use core::time::Duration;
use rustc_hash::FxHashMap;
use s2n_quic_core::{
    frame::ack::EcnCounts, packet::number::PacketNumberSpace, path::INITIAL_PTO_BACKOFF, random,
    recovery::RttEstimator, varint::VarInt,
};
use s2n_quic_platform::features::Gso;
use std::{cell::RefCell, rc::Rc, sync::Arc};

#[cfg(test)]
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
    pub fn push_back(&mut self, frame: intrusive::Entry<Frame>) {
        debug_assert!(
            !matches!(frame.header, frame::Header::Ack { .. }),
            "Ack frames must use pending_acks, not the priority queues"
        );
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
    pub fn push_front(&mut self, frame: intrusive::Entry<Frame>) {
        debug_assert!(
            !matches!(frame.header, frame::Header::Ack { .. }),
            "Ack frames must use pending_acks, not the priority queues"
        );
        self.byte_cost += frame.byte_cost() as usize;
        self.gauge.enqueue(1);
        self.queue.push_front(frame);
    }

    /// Remove the next frame from the front of the queue, updating the cost counter.
    #[inline]
    pub fn pop_front(&mut self) -> Option<intrusive::Entry<Frame>> {
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

    /// Returns the number of frames in the queue.
    #[inline]
    pub fn len(&self) -> usize {
        self.queue.len()
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

/// Pending ACK submissions with integrated queue gauge.
pub(crate) struct PendingAcks {
    queue: Queue<msg::Sender>,
    gauge: QueueGauge,
}

impl PendingAcks {
    pub fn new(gauge: QueueGauge) -> Self {
        Self {
            queue: Queue::new(),
            gauge,
        }
    }

    #[inline]
    pub fn push_back(&mut self, entry: intrusive::Entry<msg::Sender>) {
        self.gauge.enqueue(1);
        self.queue.push_back(entry);
    }

    #[inline]
    pub fn push_front(&mut self, entry: intrusive::Entry<msg::Sender>) {
        self.gauge.enqueue(1);
        self.queue.push_front(entry);
    }

    #[inline]
    pub fn pop_front(&mut self) -> Option<intrusive::Entry<msg::Sender>> {
        let entry = self.queue.pop_front()?;
        self.gauge.dequeue();
        Some(entry)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[must_use = "WheelInterest must be dispatched; ignoring it silently skips wheel scheduling"]
#[derive(Clone, Copy, Debug)]
pub struct WheelInterest {
    pub transmission: bool,
    pub pto: bool,
    pub idle_timeout: bool,
}

pub trait WheelRoutable {
    type Output;
    fn split(self) -> ((Rc<RefCell<Context>>, WheelInterest), Self::Output);
}

impl WheelRoutable for (Rc<RefCell<Context>>, WheelInterest) {
    type Output = ();
    #[inline]
    fn split(self) -> ((Rc<RefCell<Context>>, WheelInterest), ()) {
        (self, ())
    }
}

impl<T> WheelRoutable for (Rc<RefCell<Context>>, WheelInterest, T) {
    type Output = T;
    #[inline]
    fn split(self) -> ((Rc<RefCell<Context>>, WheelInterest), T) {
        ((self.0, self.1), self.2)
    }
}

/// A `Receiver<I::Output>` that takes `WheelRoutable` items from an inner receiver,
/// dispatches each context into the appropriate timing wheel senders, and forwards
/// the remaining output downstream.
pub struct WheelRouter<I, R, TxW, PtoW, IdleW> {
    inner: R,
    tx_wheel: TxW,
    pto_wheel: PtoW,
    idle_wheel: IdleW,
    _item: core::marker::PhantomData<fn() -> I>,
}

impl<I, R, TxW, PtoW, IdleW> WheelRouter<I, R, TxW, PtoW, IdleW> {
    pub fn new(inner: R, tx_wheel: TxW, pto_wheel: PtoW, idle_wheel: IdleW) -> Self {
        Self {
            inner,
            tx_wheel,
            pto_wheel,
            idle_wheel,
            _item: core::marker::PhantomData,
        }
    }
}

impl<I, R, TxW, PtoW, IdleW> crate::socket::channel::Receiver<I::Output>
    for WheelRouter<I, R, TxW, PtoW, IdleW>
where
    I: WheelRoutable,
    R: crate::socket::channel::Receiver<I>,
    TxW: UnboundedSender<Rc<RefCell<Context>>>,
    PtoW: UnboundedSender<Rc<RefCell<Context>>>,
    IdleW: UnboundedSender<Rc<RefCell<Context>>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut core::task::Context<'_>,
        budget: &mut crate::socket::channel::Budget,
    ) -> core::task::Poll<Option<I::Output>> {
        let item = core::task::ready!(self.inner.poll_recv(cx, budget));
        let Some(item) = item else {
            return core::task::Poll::Ready(None);
        };

        let ((context, interest), output) = item.split();

        if interest.idle_timeout {
            let _ = self.idle_wheel.send(context.clone());
        }
        if interest.pto {
            let _ = self.pto_wheel.send(context.clone());
        }
        if interest.transmission {
            let _ = self.tx_wheel.send(context);
        }

        core::task::Poll::Ready(Some(output))
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

/// PTO probe state for a send context.
///
/// The assembler checks this on every assembly round:
/// - `Idle`: no probe pending; perform normal immediate + pending drain.
/// - `Requested`: a probe must be sent. If `pending` data is present it serves as the
///   ack-eliciting packet (CWND is bypassed per RFC 9002 §6.2.4). Otherwise the assembler
///   retransmits frames from the oldest non-shell inflight entry under a new packet number,
///   linking the two via `inflight::Packet::probed_to`. After an ack-eliciting packet is
///   successfully encoded, the assembler calls `on_transmit()` to transition back to `Idle`.
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProbeState {
    /// No probe pending.
    #[default]
    Idle,
    /// A probe has been requested by the PTO handler.
    Requested,
}

impl ProbeState {
    s2n_quic_core::state::is!(
        /// Returns `true` when a probe is pending.
        is_requested, Requested
    );

    s2n_quic_core::state::event! {
        /// Transition `Idle → Requested` when a PTO fires.
        request(Idle => Requested);
        /// Transition `Requested → Idle` after the assembler transmits an ack-eliciting probe.
        on_transmit(Requested => Idle);
        /// Transition `Requested → Idle` when all inflight data is ACKed before the probe fires.
        on_all_acked(Requested => Idle);
    }

    #[cfg(test)]
    pub fn dot_test() {
        insta::assert_snapshot!(Self::dot());
    }
}

/// Per-peer send state, one per (credentials_id, send_socket) pair.
///
/// Holds crypto material, congestion control, inflight tracking, and the pending frame
/// queues. Frames are pushed in by the Dispatcher, then `assemble()` is called when the
/// local wheel fires to pack them into encrypted packets.
///
/// Data frames are held in a priority-indexed array of queues, all subject to CWND gating.
/// ACKs are handled separately via `pending_acks` (direct path, bypasses CWND).
pub(crate) struct Context {
    pub path_secret_entry: Arc<PathSecretEntry>,
    pub sealer: crate::crypto::awslc::seal::Application,
    pub credentials: Credentials,
    /// Resolved destination address for this sender (cached at context creation).
    pub peer_addr: std::net::SocketAddr,
    /// Next packet number to assign
    pub next_packet_number: VarInt,
    /// Next attempt ID for FlowInit deduplication (per-sender counter)
    pub flow_attempt_id_counter: VarInt,
    pub cca: congestion::Controller,
    pub rtt_estimator: RttEstimator,
    pub inflight: inflight::Map,
    pub pto: Pto,
    /// Per-priority frame queues.  Index 0 (`Priority::Ack`) bypasses CWND; indices 1–N
    /// are subject to congestion-window gating and are drained highest-priority first.
    pub queues: [PendingFrames; Priority::LEVELS],
    /// Pending direct ACK submissions from recv dispatch workers.
    pub pending_acks: PendingAcks,
    /// Index of this socket in the path secret entry's `sender_load_scores` array.
    ///
    /// Used by `publish_sender_load_score` to write the correct slot so the
    /// load-balancer pick-two logic has up-to-date per-socket load information.
    pub sender_idx: usize,
    /// Intrusive links and target time for the transmission pacing wheel
    pub tx_wheel: WheelLinks,
    /// Intrusive links and target time for the PTO (probe timeout) wheel
    pub pto_wheel: WheelLinks,
    /// Intrusive links and target time for the idle timeout wheel
    pub idle_wheel: WheelLinks,
    /// Last-seen ECN counts from peer ACK frames; used to compute deltas for the CCA.
    pub peer_ecn_counts: EcnCounts,
    pub created_at: precision::Timestamp,
}

#[derive(Debug)]
pub enum ContextError {
    PeerDataAddrsNotReady,
    PeerDataAddrsEmpty,
}

impl std::fmt::Display for ContextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PeerDataAddrsNotReady => write!(f, "peer data addrs not yet exchanged"),
            Self::PeerDataAddrsEmpty => write!(f, "peer data addrs list is empty"),
        }
    }
}

impl Context {
    pub fn new(
        entry: &Arc<PathSecretEntry>,
        inflight_gauge: QueueGauge,
        ack_gauge: QueueGauge,
        pending_gauge: QueueGauge,
        sender_idx: usize,
        clock: &impl precision::Clock,
    ) -> Result<Self, ContextError> {
        let (sealer, credentials) = entry.reusable_sealer();
        let cca = congestion::Controller::new(entry.max_datagram_size());
        let rtt_estimator = RttEstimator::new(Duration::from_millis(2));
        let inflight = inflight::Map::new(inflight_gauge);

        let addrs = entry
            .peer_data_addrs()
            .get()
            .ok_or(ContextError::PeerDataAddrsNotReady)?;
        if addrs.is_empty() {
            return Err(ContextError::PeerDataAddrsEmpty);
        }
        let peer_addr = std::net::SocketAddr::from(addrs[sender_idx % addrs.len()].unmap());

        Ok(Self {
            path_secret_entry: entry.clone(),
            sealer,
            credentials,
            peer_addr,
            next_packet_number: VarInt::ZERO,
            flow_attempt_id_counter: VarInt::ZERO,
            cca,
            rtt_estimator,
            inflight,
            pto: Pto::default(),
            queues: core::array::from_fn(|_| PendingFrames::new(pending_gauge.clone())),
            pending_acks: PendingAcks::new(ack_gauge),
            sender_idx,
            tx_wheel: WheelLinks::new(),
            pto_wheel: WheelLinks::new(),
            idle_wheel: WheelLinks::new(),
            peer_ecn_counts: EcnCounts::default(),
            created_at: clock.now(),
        })
    }

    /// Append all frames from a batch and return wheel interest indicating which wheels
    /// need this context inserted.
    ///
    /// Each priority level is appended in O(1) via an intrusive list splice.
    pub fn push_batch<Clk: precision::Clock + ?Sized>(
        &mut self,
        batch: super::combinator::FrameBatch,
        clock: &Clk,
    ) -> WheelInterest {
        let (queues, byte_costs) = batch.into_queues();
        for (queue, (slot, cost)) in queues
            .into_iter()
            .zip(self.queues.iter_mut().zip(byte_costs))
        {
            if !queue.is_empty() {
                slot.append_queue(queue, cost);
            }
        }
        // Refresh the load score immediately so pick-two sees the updated backlog.
        let now: s2n_quic_core::time::Timestamp = clock.now().into();
        self.publish_sender_load_score(now);
        let interest = self.wheel_interest(clock);
        self.invariants();
        interest
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
        ack_delay: Duration,
        counters: &super::counters::Send,
        completed: &mut impl UnboundedSender<intrusive::Entry<Frame>>,
        lost: &mut impl UnboundedSender<intrusive::Entry<Frame>>,
        cancelled: &mut impl UnboundedSender<intrusive::Entry<Frame>>,
        clock: &Clk,
        random: &mut Rand,
    ) -> WheelInterest
    where
        Clk: s2n_quic_core::time::Clock + precision::Clock + ?Sized,
        Rand: random::Generator,
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
                        &ack_frame, ack_delay, self, counters, completed, lost, cancelled, clock,
                        random,
                    );
                }
                s2n_quic_core::frame::FrameMut::Padding(_)
                | s2n_quic_core::frame::FrameMut::Ping(_) => {}
                frame => {
                    tracing::debug!(?frame, "unexpected control frame type in ACK payload");
                }
            }
        }

        let interest = self.wheel_interest(clock);
        self.invariants();
        interest
    }

    /// Compute wheel interest after a state change
    pub(crate) fn wheel_interest<Clk>(&mut self, clock: &Clk) -> WheelInterest
    where
        Clk: precision::Clock + ?Sized,
    {
        debug_assert!(
            !self.pto.probe_state.is_requested()
                || self.inflight.has_inflight()
                || self.has_pending(),
            "probe_state is Requested but nothing to probe with"
        );

        let transmission = if
        // Check we have queued packets and we're not already linked
        !self.is_tx_scheduled()
            && (self.has_pending_acks()
                || self.pto.probe_state.is_requested()
                || (self.has_pending_data() && self.can_send_pending_frames()))
        {
            // Probes bypass pacing: if one is pending schedule immediately so the
            // assembler can encode it without waiting for the CCA departure time.
            let target = if self.pto.probe_state.is_requested() {
                None
            } else {
                self.cca
                    .earliest_departure_time()
                    .map(precision::Timestamp::from)
            };
            // If target time is `None` then the wheel will schedule it immediately
            self.tx_wheel.target_time = target;
            true
        } else {
            false
        };

        let pto = if !self.is_pto_scheduled() && self.inflight.has_inflight() {
            if let Some(target) = self.pto.next_target(clock, &self.rtt_estimator) {
                self.pto_wheel.target_time = Some(target);
                true
            } else {
                false
            }
        } else {
            false
        };

        let idle_timeout = if !self.is_idle_scheduled() {
            let timeout = self.path_secret_entry.idle_timeout();
            let target = clock.now() + timeout;
            self.idle_wheel.target_time = Some(target);
            true
        } else {
            false
        };

        let interest = WheelInterest {
            transmission,
            pto,
            idle_timeout,
        };

        self.invariants();
        interest
    }

    /// Pop the next pending frame, draining from highest priority first.
    #[inline]
    pub fn pop_pending(&mut self) -> Option<intrusive::Entry<Frame>> {
        for queue in &mut self.queues {
            if let Some(frame) = queue.pop_front() {
                return Some(frame);
            }
        }
        None
    }

    /// Push a frame back to the front of whichever priority queue it belongs to.
    ///
    /// Only call this with a frame just removed via [`pop_pending`].
    #[inline]
    pub fn push_front_frame(&mut self, frame: intrusive::Entry<Frame>) {
        self.queues[frame.priority().as_index()].push_front(frame);
    }

    /// Push a frame to the back of whichever priority queue it belongs to.
    #[inline]
    pub fn push_back_frame(&mut self, frame: intrusive::Entry<Frame>) {
        self.queues[frame.priority().as_index()].push_back(frame);
    }

    /// Publish the sender load score to the path secret entry.
    ///
    /// The score is a composite estimate of how busy this sender is:
    ///
    ///   score = base + queued_bytes / pacing_rate
    ///
    /// where `base = max(now, earliest_departure_time) + congestion_penalty`.
    ///
    /// Using `earliest_departure_time` as the floor means BBR pacing information is
    /// incorporated directly — a sender that is pacing-gated looks more loaded than one
    /// that is idle.  The congestion penalty (one smoothed RTT when cwnd-limited) steers
    /// new batches toward senders that still have room in their congestion window.
    ///
    /// Call this whenever the pending queue or CCA state changes (on send, on ACK, and
    /// on enqueue).
    #[inline]
    pub fn publish_sender_load_score(&self, now: s2n_quic_core::time::Timestamp) {
        let total_cost: usize = self.queues.iter().map(|q| q.byte_cost()).sum();

        // Use earliest_departure_time as the base if it is in the future so that
        // pacing-limited senders appear more loaded than idle ones.
        let base = self
            .cca
            .earliest_departure_time()
            .map(|edt| edt.max(now))
            .unwrap_or(now);

        // Add a penalty equal to one smoothed RTT when cwnd-limited.  This makes
        // congested senders look more expensive, steering new work to peers with
        // available congestion window.
        let congestion_penalty = if self.cca.is_congestion_limited() {
            self.rtt_estimator.smoothed_rtt()
        } else {
            Duration::ZERO
        };

        self.path_secret_entry.update_sender_load_score(
            self.sender_idx,
            base + congestion_penalty,
            total_cost,
            self.cca.bandwidth(),
        );
    }

    #[inline]
    pub fn has_pending(&self) -> bool {
        self.queues.iter().any(|q| !q.is_empty())
    }

    #[cfg_attr(not(test), expect(dead_code))]
    #[inline]
    pub fn pending_count(&self) -> usize {
        self.queues.iter().map(|q| q.len()).sum()
    }

    #[inline]
    pub fn has_pending_acks(&self) -> bool {
        !self.pending_acks.is_empty()
    }

    #[inline]
    pub fn has_pending_data(&self) -> bool {
        self.queues.iter().any(|q| !q.is_empty())
    }

    #[inline]
    pub fn can_send_pending_frames(&self) -> bool {
        self.cca.requires_fast_retransmission() || !self.cca.is_congestion_limited()
    }

    #[inline]
    pub fn is_tx_scheduled(&self) -> bool {
        self.tx_wheel.is_scheduled()
    }

    #[inline]
    pub fn is_pto_scheduled(&self) -> bool {
        self.pto_wheel.is_scheduled()
    }

    #[inline]
    pub fn is_idle_scheduled(&self) -> bool {
        self.idle_wheel.is_scheduled()
    }

    #[inline]
    pub fn path_info(&self, gso: &Gso) -> PathInfo {
        let mtu = self.path_secret_entry.max_datagram_size();
        let send_quantum_segments = (self.cca.send_quantum() as u64).div_ceil(mtu as u64) as usize;
        let max_segments = gso
            .max_segments()
            .min(segment::MAX_COUNT)
            .min(send_quantum_segments);
        PathInfo {
            max_datagram_size: mtu,
            max_segments,
        }
    }

    /// Called when the PTO wheel fires for this context.
    ///
    /// Transitions the probe state from `Idle` to `Requested` if a real probe
    /// should be sent, then computes and returns the updated `WheelInterest` for
    /// rescheduling. The caller only needs to dispatch the returned interest.
    pub fn on_pto_timeout<Clk: precision::Clock + ?Sized>(&mut self, clock: &Clk) -> WheelInterest {
        if self.pto.on_timeout() && (self.inflight.has_inflight() || self.has_pending()) {
            // The only failure case is `NoOp` — the state is already `Requested`
            // because a previous probe hasn't been consumed by the assembler yet.
            // That's harmless: the assembler will send the probe on its next run.
            let _ = self.pto.probe_state.request();
        }
        let interest = self.wheel_interest(clock);
        self.invariants();
        interest
    }

    /// Verify structural invariants of the context.
    ///
    /// Runs assertions guarded by `cfg!(debug_assertions)` — in release builds this
    /// compiles away to nothing. Call this after any mutation that could violate these
    /// invariants:
    /// - PTO target should be `None` when there is no inflight data (no need to probe).
    /// - Every inflight packet must either have a `probed_to` link (shell) or contain
    ///   non-empty, all-ack-eliciting frames (no stale ACK frames stored).
    /// - The sum of `sent_bytes` in the inflight map must equal `cca.bytes_in_flight()`.
    #[inline]
    pub fn invariants(&self) {
        if cfg!(debug_assertions) {
            let has_pending_data = self.queues.iter().any(|q| !q.is_empty());
            let has_pending_data_predicate = self.has_pending_data();
            let has_pending_predicate = self.has_pending();
            assert_eq!(
                has_pending_data_predicate,
                has_pending_data,
                "has_pending_data predicate drifted from queue contents"
            );
            assert_eq!(
                has_pending_predicate,
                has_pending_data,
                "has_pending predicate drifted from queue contents"
            );

            if self.tx_wheel.is_scheduled() {
                assert!(
                    self.has_pending_acks()
                        || self.pto.probe_state.is_requested()
                        || (self.has_pending_data() && self.can_send_pending_frames()),
                    "tx wheel scheduled without any sendable work"
                );
                assert!(
                    self.tx_wheel.target_time.is_some()
                        || self.pto.probe_state.is_requested()
                        || self.cca.earliest_departure_time().is_none(),
                    "tx wheel has no target despite probe not requested and EDT present"
                );
            }

            if self.pto_wheel.is_scheduled() {
                assert!(
                    self.inflight.has_inflight(),
                    "pto wheel scheduled without inflight packets"
                );
                assert!(
                    self.pto_wheel.target_time.is_some(),
                    "pto wheel scheduled without target_time"
                );
            }

            if self.idle_wheel.is_scheduled() {
                assert!(
                    self.idle_wheel.target_time.is_some(),
                    "idle wheel scheduled without target_time"
                );
            }

            if self.pto.probe_state.is_requested() {
                assert!(
                    self.inflight.has_inflight() || self.has_pending(),
                    "probe_state is Requested but there is no inflight or pending work"
                );
            }

            if !self.inflight.has_inflight() {
                assert!(
                    !self.pto.is_armed(),
                    "PTO is armed but there is no inflight data to probe"
                );
            }

            if let Some(max_inflight_pn) = self.inflight.max_packet_number() {
                assert!(
                    self.next_packet_number.as_u64() > max_inflight_pn.as_u64(),
                    "next_packet_number ({}) must be > max inflight pn ({})",
                    self.next_packet_number.as_u64(),
                    max_inflight_pn.as_u64()
                );
            }

            let sender_slots = self.path_secret_entry.socket_sender_count();
            if sender_slots > 0 {
                assert!(
                    self.sender_idx < sender_slots,
                    "sender_idx ({}) out of range for sender_load_scores_len ({sender_slots})",
                    self.sender_idx
                );
            }

            let map_bytes: u32 = self.inflight.sum_sent_bytes();
            let cca_bytes = self.cca.bytes_in_flight();
            assert_eq!(
                map_bytes, cca_bytes,
                "inflight map bytes ({map_bytes}) != CCA bytes_in_flight ({cca_bytes})"
            );
        }
        self.inflight.invariants();
    }

    pub fn drain_frames(
        &mut self,
        reason: frame::FailureReason,
        cancelled: &mut impl UnboundedSender<intrusive::Entry<Frame>>,
    ) {
        let range = self.inflight.get_range();
        for (_pn, packet) in self.inflight.remove_range(range) {
            for mut frame in packet.frames {
                frame.status = frame::TransmissionStatus::Failed(reason);
                let _ = cancelled.send(frame.into());
            }
        }
        for queue in &mut self.queues {
            while let Some(mut frame) = queue.pop_front() {
                frame.status = frame::TransmissionStatus::Failed(reason);
                let _ = cancelled.send(frame);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PathInfo {
    pub max_datagram_size: u16,
    pub max_segments: usize,
}

// ── Wheel Links ───────────────────────────────────────────────────────────

pub(crate) use crate::time::wheel::WheelLinks;

// ── Wheel Adapters ────────────────────────────────────────────────────────
//
// Each adapter tells the intrusive list infrastructure how to reach the Links
// field for its wheel, and tells the timing wheel how to read/write target_time.
// The pointer type is Rc<RefCell<Context>> for all three.

crate::context_wheel_adapter!(TxWheelAdapter, Context, tx_wheel);
crate::context_wheel_adapter!(PtoWheelAdapter, Context, pto_wheel);
crate::context_wheel_adapter!(IdleWheelAdapter, Context, idle_wheel);

/// PTO (Probe Timeout) state for tail loss recovery.
///
/// When all inflight packets may be lost (no ACKs arriving), PTO fires to send a probe.
/// This ensures the peer generates an ACK, which either confirms delivery or triggers
/// loss detection.
///
/// ## Constant-period wheel arming
///
/// The intrusive timing wheel does not support updating existing entries, so we always
/// arm at one base PTO period (1× `pto_period(INITIAL_PTO_BACKOFF)`) and track the
/// effective backoff as a fire count. The wheel fires cheaply at the base rate;
/// the handler decrements the counter and only sends a real probe when it reaches zero.
///
/// On ACK: reset `firings_remaining` to 0 so the very next wheel firing is a probe
/// (equivalent to resetting backoff to 1×). After at most one base period, we know
/// whether more inflight data needs probing.
///
/// `arm_base` advances by one base period on every arm so successive firings are evenly
/// spaced, even when `last_sent_time` stops advancing. Reset to `None` on packet-sent
/// or ACK so the next arm re-anchors to the freshest send timestamp.
pub(crate) struct Pto {
    /// Number of base-period wheel firings to consume before sending the next probe.
    ///
    /// Set to `backoff - 1` after a probe fires (so the next `backoff` firings elapse
    /// before probing again). Reset to 0 on ACK.
    pub firings_remaining: u32,
    /// Current effective backoff multiplier (doubles after each probe, capped at 16×).
    pub backoff: u32,
    /// Rolling base for `next_target` computations.
    ///
    /// `next_target` sets `target = arm_base + base_period` then advances `arm_base`
    /// to that value so consecutive arms are evenly spaced. Reset to `None` on
    /// packet-sent and on ACK so the next arm re-anchors to `last_sent_time`.
    pub arm_base: Option<precision::Timestamp>,
    pub last_sent_time: Option<precision::Timestamp>,
    pub needs_update: bool,
    /// PTO probe state: set to `Requested` by the PTO handler via `on_pto_timeout`;
    /// cleared by the assembler after the probe segment is encoded.
    pub probe_state: ProbeState,
}

impl Default for Pto {
    fn default() -> Self {
        Self {
            firings_remaining: 0,
            backoff: INITIAL_PTO_BACKOFF,
            arm_base: None,
            last_sent_time: None,
            needs_update: false,
            probe_state: ProbeState::Idle,
        }
    }
}

impl Pto {
    pub fn on_packet_sent(&mut self, now: precision::Timestamp) {
        self.last_sent_time = Some(now);
        // Reset arm_base so the next update_target re-anchors to this send time.
        self.arm_base = None;
        self.needs_update = true;
    }

    pub fn on_ack_received(&mut self, has_remaining_inflight: bool) {
        self.backoff = INITIAL_PTO_BACKOFF;
        self.firings_remaining = 0;
        // Reset arm_base so the next arm is relative to the freshest last_sent_time.
        self.arm_base = None;

        if !has_remaining_inflight {
            let _ = self.probe_state.on_all_acked();
        }

        self.needs_update = has_remaining_inflight;
    }

    /// Called when the PTO wheel fires for this context.
    ///
    /// Returns `true` if a probe should be sent now, `false` if this was a
    /// countdown firing (or a needs-update re-sync) that simply re-arms the wheel.
    pub fn on_timeout(&mut self) -> bool {
        if self.needs_update {
            // A packet was sent since the last arm; re-sync the arm base to the new
            // last_sent_time rather than firing a spurious probe.
            self.needs_update = false;
            self.arm_base = None;
            return false;
        }

        if self.firings_remaining > 0 {
            self.firings_remaining -= 1;
            return false;
        }

        // Time to probe: double backoff for next round (capped at 16×).
        self.backoff = self.backoff.saturating_mul(2).min(16);
        self.firings_remaining = self.backoff - 1;
        true
    }

    /// Returns `true` if the PTO needs to fire (has remaining countdown or pending state).
    pub fn is_armed(&self) -> bool {
        self.needs_update || self.arm_base.is_some()
    }

    /// Compute the next wheel arm target.
    ///
    /// Always uses one base period (1× `pto_period(INITIAL_PTO_BACKOFF)`) regardless
    /// of the current backoff level. `arm_base` advances by one period each call so
    /// consecutive firings are evenly spaced.
    pub fn next_target<Clk: precision::Clock + ?Sized>(
        &mut self,
        clock: &Clk,
        rtt_estimator: &RttEstimator,
    ) -> Option<precision::Timestamp> {
        let mut base_period =
            rtt_estimator.pto_period(INITIAL_PTO_BACKOFF, PacketNumberSpace::Initial);
        base_period = base_period.max(Duration::from_millis(2));

        // Anchor to arm_base if available, otherwise to last_sent_time, otherwise now.
        let base = self
            .arm_base
            .unwrap_or_else(|| self.last_sent_time.unwrap_or_else(|| clock.now()));
        let next = base + base_period;
        // Advance arm_base so the next call steps forward by another period.
        self.arm_base = Some(next);
        Some(next)
    }
}

/// Per-socket cache of send contexts, keyed by credentials ID.
///
/// Each send socket has its own Cache. A single peer always routes through the same
/// send socket (via credential hashing), so there's exactly one Context per peer per socket.
pub(crate) struct Cache {
    contexts: FxHashMap<credentials::Id, Rc<RefCell<Context>>>,
    inflight_gauge: QueueGauge,
    ack_gauge: QueueGauge,
    pending_gauge: QueueGauge,
    sender_idx: usize,
}

impl Cache {
    pub fn new(counter_registry: &crate::counter::Registry, sender_idx: usize) -> Self {
        Self {
            contexts: FxHashMap::default(),
            inflight_gauge: counter_registry.register_queue_gauge("send.inflight"),
            ack_gauge: counter_registry.register_queue_gauge("send.ack"),
            pending_gauge: counter_registry.register_queue_gauge("send.pending"),
            sender_idx,
        }
    }

    #[cfg(test)]
    pub fn context_count(&self) -> usize {
        self.contexts.len()
    }

    pub fn get_or_insert(
        &mut self,
        entry: &Arc<PathSecretEntry>,
        clock: &impl precision::Clock,
    ) -> Result<Rc<RefCell<Context>>, ContextError> {
        use std::collections::hash_map::Entry as MapEntry;

        let id = *entry.id();

        match self.contexts.entry(id) {
            MapEntry::Occupied(e) => Ok(e.get().clone()),
            MapEntry::Vacant(e) => {
                let ctx = Context::new(
                    entry,
                    self.inflight_gauge.clone(),
                    self.ack_gauge.clone(),
                    self.pending_gauge.clone(),
                    self.sender_idx,
                    clock,
                )?;
                Ok(e.insert(Rc::new(RefCell::new(ctx))).clone())
            }
        }
    }

    pub fn get(&self, id: &credentials::Id) -> Option<Rc<RefCell<Context>>> {
        self.contexts.get(id).cloned()
    }

    pub fn remove(&mut self, id: &credentials::Id) {
        self.contexts.remove(id);
    }

    pub fn invalidate(
        &mut self,
        id: &credentials::Id,
        reason: frame::FailureReason,
        cancelled: &mut impl UnboundedSender<intrusive::Entry<Frame>>,
    ) {
        let Some(ctx) = self.contexts.remove(id) else {
            tracing::trace!(%id, sender_idx = self.sender_idx, "invalidate: no context found");
            return;
        };
        let mut ctx = ctx.borrow_mut();
        tracing::debug!(%id, sender_idx = self.sender_idx, "invalidating send context");
        ctx.drain_frames(reason, cancelled);
    }
}
