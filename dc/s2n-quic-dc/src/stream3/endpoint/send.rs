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
    socket::channel::ByteCost,
    stream3::{endpoint::inflight, frame::Frame},
};
use core::time::Duration;
use s2n_quic_core::{
    packet::number::PacketNumberSpace, path::INITIAL_PTO_BACKOFF, recovery::RttEstimator,
    varint::VarInt,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

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
}

impl PendingFrames {
    #[inline]
    pub fn new() -> Self {
        Self {
            queue: Queue::new(),
            byte_cost: 0,
        }
    }

    /// Push a frame onto the back of the queue, updating the cost counter.
    #[inline]
    pub fn push_back(&mut self, frame: intrusive_queue::Entry<Frame>) {
        self.byte_cost += frame.byte_cost() as usize;
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
        Some(frame)
    }

    /// Returns `true` if the queue contains no frames.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Returns the number of frames in the queue.
    #[inline]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns the accumulated wire cost of all frames currently in the queue.
    #[inline]
    pub fn byte_cost(&self) -> usize {
        self.byte_cost
    }

    /// Returns a shared reference to the inner queue for read-only operations
    /// (e.g. iteration during segment encoding).
    #[inline]
    pub fn as_queue(&self) -> &Queue<Frame> {
        &self.queue
    }
}

/// Per-peer send state, one per (credentials_id, send_socket) pair.
///
/// Holds crypto material, congestion control, inflight tracking, and the pending frame
/// queue. Frames are pushed in by the Dispatcher, then `assemble()` is called when the
/// local wheel fires to pack them into encrypted packets.
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
    /// Frames waiting to be assembled into packets, with integrated wire-cost tracking.
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
            pending: PendingFrames::new(),
            sender_idx,
            tx_wheel: WheelLinks::new(),
            pto_wheel: WheelLinks::new(),
            idle_wheel: WheelLinks::new(),
        }
    }

    /// Push a frame onto the pending queue.
    #[inline]
    pub fn push_frame(&mut self, frame: intrusive_queue::Entry<Frame>) {
        self.pending.push_back(frame);
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
            self.pending.byte_cost(),
            self.cca.bandwidth(),
        );
    }

    #[inline]
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
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
    contexts: HashMap<credentials::Id, Rc<RefCell<Context>>>,
    inflight_gauge: QueueGauge,
    sender_idx: usize,
}

impl Cache {
    pub fn new(inflight_gauge: QueueGauge, sender_idx: usize) -> Self {
        Self {
            contexts: HashMap::new(),
            inflight_gauge,
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
                    self.sender_idx,
                )))
            })
            .clone()
    }

    pub fn get(&self, id: &credentials::Id) -> Option<Rc<RefCell<Context>>> {
        self.contexts.get(id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        clock::{
            precision::{self, Clock as _},
            testing::Clock,
            wheel::{self, Wheel},
        },
        intrusive_queue::List,
        path::secret::map::Entry as PathSecretEntry,
        socket::channel::Receiver as _,
    };
    use core::task::Poll;
    use s2n_quic_core::task::waker;
    use std::time::Duration;

    // ── Test channel (feeds entries into wheel) ───────────────────────────

    struct TestChannel<A: crate::intrusive_queue::Adapter> {
        queue: std::sync::Mutex<std::collections::VecDeque<List<A>>>,
    }

    impl<A: crate::intrusive_queue::Adapter> TestChannel<A> {
        fn new() -> Self {
            Self {
                queue: std::sync::Mutex::new(std::collections::VecDeque::new()),
            }
        }

        fn send(&self, list: List<A>) {
            self.queue.lock().unwrap().push_back(list);
        }
    }

    impl<A: crate::intrusive_queue::Adapter> crate::socket::channel::Receiver<List<A>>
        for &TestChannel<A>
    {
        fn poll_recv(&mut self, _cx: &mut core::task::Context<'_>) -> Poll<Option<List<A>>> {
            if let Some(batch) = self.queue.lock().unwrap().pop_front() {
                Poll::Ready(Some(batch))
            } else {
                Poll::Pending
            }
        }

        fn on_consumed(&mut self, _bytes: u64) {}
    }

    fn poll_wheel<A, Timer, R>(wheel: &mut Wheel<A, Timer, R, 1>) -> Option<List<A>>
    where
        A: wheel::WheelAdapter,
        Timer: precision::Timer,
        R: crate::socket::channel::Receiver<List<A>>,
    {
        let waker = waker::noop();
        let mut cx = core::task::Context::from_waker(&waker);
        match wheel.poll_recv(&mut cx) {
            Poll::Ready(Some(list)) => Some(list),
            _ => None,
        }
    }

    fn make_wheel<'a, A: wheel::WheelAdapter>(
        channel: &'a TestChannel<A>,
        clock: &Clock,
    ) -> Wheel<A, crate::clock::testing::Timer, &'a TestChannel<A>, 1> {
        Wheel::new(channel, clock.timer())
    }

    // ── Helper to build a test context ────────────────────────────────────

    fn make_context() -> Rc<RefCell<Context>> {
        let entry = PathSecretEntry::fake("127.0.0.1:8080".parse().unwrap(), None);
        let registry = crate::counter::Registry::new();
        let gauge = registry.register_queue_gauge("test.inflight");
        Rc::new(RefCell::new(Context::new(&entry, gauge, 0)))
    }

    // ── Tests ─────────────────────────────────────────────────────────────

    #[test]
    fn tx_wheel_insert_and_fire() {
        let clock = Clock::new(Duration::from_micros(1000));
        let channel: TestChannel<TxWheelAdapter> = TestChannel::new();
        let mut wheel = make_wheel(&channel, &clock);

        let ctx = make_context();

        // Set target time 10µs in the future
        let target = clock.get_time() + Duration::from_micros(10);
        ctx.borrow_mut().tx_wheel.target_time = Some(target);

        // Insert into wheel via channel
        let mut list = List::new();
        list.push_back(ctx.clone());
        channel.send(list);

        // Poll to insert — should not fire yet
        assert!(poll_wheel(&mut wheel).is_none());

        // Advance time past target
        clock.set(target);
        let mut result = poll_wheel(&mut wheel).unwrap();

        // Should get our context back
        let popped: Rc<RefCell<Context>> = result.pop_front().unwrap();
        assert!(Rc::ptr_eq(&popped, &ctx));
        assert!(result.is_empty());
    }

    #[test]
    fn pto_wheel_insert_and_fire() {
        let clock = Clock::new(Duration::from_micros(1000));
        let channel: TestChannel<PtoWheelAdapter> = TestChannel::new();
        let mut wheel = make_wheel(&channel, &clock);

        let ctx = make_context();

        // Set PTO target 50µs in the future
        let target = clock.get_time() + Duration::from_micros(50);
        ctx.borrow_mut().pto_wheel.target_time = Some(target);

        let mut list = List::new();
        list.push_back(ctx.clone());
        channel.send(list);

        assert!(poll_wheel(&mut wheel).is_none());

        clock.set(target);
        let mut result = poll_wheel(&mut wheel).unwrap();

        let popped: Rc<RefCell<Context>> = result.pop_front().unwrap();
        assert!(Rc::ptr_eq(&popped, &ctx));
    }

    #[test]
    fn idle_wheel_insert_and_fire() {
        let clock = Clock::new(Duration::from_micros(1000));
        let channel: TestChannel<IdleWheelAdapter> = TestChannel::new();
        let mut wheel = make_wheel(&channel, &clock);

        let ctx = make_context();

        // Set idle target 200µs in the future
        let target = clock.get_time() + Duration::from_micros(200);
        ctx.borrow_mut().idle_wheel.target_time = Some(target);

        let mut list = List::new();
        list.push_back(ctx.clone());
        channel.send(list);

        assert!(poll_wheel(&mut wheel).is_none());

        clock.set(target);
        let mut result = poll_wheel(&mut wheel).unwrap();

        let popped: Rc<RefCell<Context>> = result.pop_front().unwrap();
        assert!(Rc::ptr_eq(&popped, &ctx));
    }

    #[test]
    fn three_wheels_are_independent() {
        let clock = Clock::new(Duration::from_micros(1000));

        let tx_channel: TestChannel<TxWheelAdapter> = TestChannel::new();
        let pto_channel: TestChannel<PtoWheelAdapter> = TestChannel::new();
        let idle_channel: TestChannel<IdleWheelAdapter> = TestChannel::new();

        let mut tx_wheel = make_wheel(&tx_channel, &clock);
        let mut pto_wheel = make_wheel(&pto_channel, &clock);
        let mut idle_wheel = make_wheel(&idle_channel, &clock);

        let ctx = make_context();

        // Schedule same context in all three wheels at different times
        let tx_target = clock.get_time() + Duration::from_micros(5);
        let pto_target = clock.get_time() + Duration::from_micros(20);
        let idle_target = clock.get_time() + Duration::from_micros(100);

        {
            let mut c = ctx.borrow_mut();
            c.tx_wheel.target_time = Some(tx_target);
            c.pto_wheel.target_time = Some(pto_target);
            c.idle_wheel.target_time = Some(idle_target);
        }

        // Insert into all three wheels
        let mut tx_list = List::new();
        tx_list.push_back(ctx.clone());
        tx_channel.send(tx_list);

        let mut pto_list = List::new();
        pto_list.push_back(ctx.clone());
        pto_channel.send(pto_list);

        let mut idle_list = List::new();
        idle_list.push_back(ctx.clone());
        idle_channel.send(idle_list);

        // Insert all entries
        assert!(poll_wheel(&mut tx_wheel).is_none());
        assert!(poll_wheel(&mut pto_wheel).is_none());
        assert!(poll_wheel(&mut idle_wheel).is_none());

        // Advance to tx_target — only tx_wheel should fire
        clock.set(tx_target);

        assert!(poll_wheel(&mut tx_wheel).is_some());
        assert!(poll_wheel(&mut pto_wheel).is_none());
        assert!(poll_wheel(&mut idle_wheel).is_none());

        // Advance to pto_target — only pto_wheel should fire
        clock.set(pto_target);

        assert!(poll_wheel(&mut pto_wheel).is_some());
        assert!(poll_wheel(&mut idle_wheel).is_none());

        // Advance to idle_target — idle_wheel fires
        clock.set(idle_target);
        assert!(poll_wheel(&mut idle_wheel).is_some());
    }

    #[test]
    fn reinsert_after_expiry() {
        let clock = Clock::new(Duration::from_micros(1000));
        let channel: TestChannel<TxWheelAdapter> = TestChannel::new();
        let mut wheel = make_wheel(&channel, &clock);

        let ctx = make_context();

        // First insertion at t+10µs
        let target1 = clock.get_time() + Duration::from_micros(10);
        ctx.borrow_mut().tx_wheel.target_time = Some(target1);

        let mut list = List::new();
        list.push_back(ctx.clone());
        channel.send(list);
        assert!(poll_wheel(&mut wheel).is_none());

        // Fire
        clock.set(target1);
        let mut result = poll_wheel(&mut wheel).unwrap();
        let _popped = result.pop_front().unwrap();

        // Reinsert with new target at t+30µs
        let target2 = clock.get_time() + Duration::from_micros(30);
        ctx.borrow_mut().tx_wheel.target_time = Some(target2);

        let mut list2 = List::new();
        list2.push_back(ctx.clone());
        channel.send(list2);
        assert!(poll_wheel(&mut wheel).is_none());

        // Should fire at new target
        clock.set(target2);
        let mut result2 = poll_wheel(&mut wheel).unwrap();
        let popped2: Rc<RefCell<Context>> = result2.pop_front().unwrap();
        assert!(Rc::ptr_eq(&popped2, &ctx));
    }

    #[test]
    fn multiple_contexts_same_wheel() {
        let clock = Clock::new(Duration::from_micros(1000));
        let channel: TestChannel<TxWheelAdapter> = TestChannel::new();
        let mut wheel = make_wheel(&channel, &clock);

        let ctx1 = make_context();
        let ctx2 = make_context();
        let ctx3 = make_context();

        // Schedule at different times
        let t1 = clock.get_time() + Duration::from_micros(5);
        let t2 = clock.get_time() + Duration::from_micros(10);
        let t3 = clock.get_time() + Duration::from_micros(15);

        ctx1.borrow_mut().tx_wheel.target_time = Some(t1);
        ctx2.borrow_mut().tx_wheel.target_time = Some(t2);
        ctx3.borrow_mut().tx_wheel.target_time = Some(t3);

        let mut list = List::new();
        list.push_back(ctx1.clone());
        list.push_back(ctx2.clone());
        list.push_back(ctx3.clone());
        channel.send(list);

        // Insert
        assert!(poll_wheel(&mut wheel).is_none());

        // Fire at t1 — only ctx1
        clock.set(t1);
        let mut result = poll_wheel(&mut wheel).unwrap();
        assert_eq!(result.len(), 1);
        let popped = result.pop_front().unwrap();
        assert!(Rc::ptr_eq(&popped, &ctx1));

        // Fire at t2 — only ctx2
        clock.set(t2);
        let mut result = poll_wheel(&mut wheel).unwrap();
        assert_eq!(result.len(), 1);
        let popped = result.pop_front().unwrap();
        assert!(Rc::ptr_eq(&popped, &ctx2));

        // Fire at t3 — only ctx3
        clock.set(t3);
        let mut result = poll_wheel(&mut wheel).unwrap();
        assert_eq!(result.len(), 1);
        let popped = result.pop_front().unwrap();
        assert!(Rc::ptr_eq(&popped, &ctx3));
    }

    // ── pending_bytes tracking tests ──────────────────────────────────────

    fn make_frame(payload_len: usize) -> intrusive_queue::Entry<Frame> {
        use crate::{
            byte_vec::ByteVec,
            packet::datagram::QueuePair,
            path::secret::map::Entry as PathSecretEntry,
            stream3::frame::{Frame, Header, TransmissionStatus, DEFAULT_TTL},
        };
        use bytes::Bytes;

        let entry = PathSecretEntry::fake("127.0.0.1:8080".parse().unwrap(), None);
        let mut payload = ByteVec::new();
        if payload_len > 0 {
            payload.push_back(Bytes::from(vec![0u8; payload_len]));
        }
        Frame {
            header: Header::FlowData {
                queue_pair: QueuePair {
                    source_queue_id: VarInt::ZERO,
                    dest_queue_id: VarInt::ZERO,
                },
                stream_id: VarInt::ZERO,
                offset: VarInt::ZERO,
                is_fin: false,
            },
            source_sender_id: VarInt::MAX,
            payload,
            path_secret_entry: entry,
            completion: None,
            status: TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        }
        .into()
    }

    #[test]
    fn pending_bytes_tracks_push_and_pop() {
        use crate::socket::channel::ByteCost as _;

        let ctx = make_context();
        let mut ctx = ctx.borrow_mut();

        assert_eq!(ctx.pending.byte_cost(), 0);

        let frame1 = make_frame(100);
        let cost1 = frame1.byte_cost() as usize;
        ctx.push_frame(frame1);
        assert_eq!(ctx.pending.byte_cost(), cost1);

        let frame2 = make_frame(200);
        let cost2 = frame2.byte_cost() as usize;
        ctx.push_frame(frame2);
        assert_eq!(ctx.pending.byte_cost(), cost1 + cost2);

        let frame = ctx.pop_pending().unwrap();
        assert_eq!(frame.payload_len(), 100);
        assert_eq!(ctx.pending.byte_cost(), cost2);

        let frame = ctx.pop_pending().unwrap();
        assert_eq!(frame.payload_len(), 200);
        assert_eq!(ctx.pending.byte_cost(), 0);

        assert!(ctx.pop_pending().is_none());
        assert_eq!(ctx.pending.byte_cost(), 0);
    }

    #[test]
    fn pending_bytes_tracks_push_front() {
        use crate::socket::channel::ByteCost as _;

        let ctx = make_context();
        let mut ctx = ctx.borrow_mut();

        let frame = make_frame(100);
        let cost = frame.byte_cost() as usize;
        ctx.push_frame(frame);
        assert_eq!(ctx.pending.byte_cost(), cost);

        // Pop then push back (simulates "doesn't fit" path in assemble)
        let frame = ctx.pop_pending().unwrap();
        assert_eq!(ctx.pending.byte_cost(), 0);

        ctx.push_front_pending(frame);
        assert_eq!(ctx.pending.byte_cost(), cost);
    }

    #[test]
    fn publish_next_transmission_time_stores_to_path_entry() {
        use crate::path::secret::map::Entry as PathSecretEntry;

        // Create an entry with one sender slot so the atomic array is populated.
        let entry = PathSecretEntry::fake_with_socket_senders(
            "127.0.0.1:8080".parse().unwrap(),
            None,
            1,
        );

        let registry = crate::counter::Registry::new();
        let gauge = registry.register_queue_gauge("test.inflight");
        let mut ctx = Context::new(&entry, gauge, 0);

        // Initial value should be zero.
        assert_eq!(entry.sender_next_transmission_micros(0), 0);

        // Push some payload bytes so pending_bytes > 0.
        ctx.push_frame(make_frame(1000));

        // Build a timestamp from a known duration.
        // SAFETY: the duration is positive (5 000 µs) and well within the range of
        // representable Timestamp values (u64 microseconds from process start).
        let now = unsafe {
            s2n_quic_core::time::Timestamp::from_duration(Duration::from_micros(5_000))
        };
        ctx.publish_next_transmission_time(now);

        // With non-zero pending bytes the published time should be >= now (5000µs).
        let published = entry.sender_next_transmission_micros(0);
        assert!(
            published >= 5_000,
            "published micros {published} should be >= now (5000µs)"
        );
    }
}
