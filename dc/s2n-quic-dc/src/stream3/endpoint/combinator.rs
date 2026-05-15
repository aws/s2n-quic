// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Channel combinators specific to the stream3 endpoint pipeline.
//!
//! These are `Receiver` adapters that compose with the generic combinators in
//! `socket::channel` but carry stream3-specific knowledge (path-secret routing,
//! frame batching by peer, etc).

use crate::{
    clock::precision,
    intrusive_queue::{Entry, Queue},
    path::secret::map::Entry as PathSecretEntry,
    socket::{
        channel::{intrusive_queue::unsync, Budget, ByteCost, Receiver, UnboundedSender},
        pool::descriptor,
        rate::Rate,
    },
    stream3::{
        endpoint::{msg, send},
        frame::{self, Frame, Priority, PriorityInput},
    },
};
use core::task::{self, Poll};
use s2n_quic_core::{ready, varint::VarInt};
use std::{cell::RefCell, marker::PhantomData, rc::Rc, sync::Arc};

#[cfg(test)]
mod tests;

// ── PathSecretMapEntry ────────────────────────────────────────────────────

/// Routing key accessor for stream3 send-side load-balancing tasks.
pub trait PathSecretMapEntry {
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry>;
}

impl<T> PathSecretMapEntry for crate::intrusive_queue::Entry<T>
where
    T: PathSecretMapEntry,
{
    #[inline]
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
        (**self).path_secret_entry()
    }
}

impl PathSecretMapEntry for crate::stream3::frame::Frame {
    #[inline]
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
        &self.path_secret_entry
    }
}

// ── FrameBatch ────────────────────────────────────────────────────────────

/// Conservative packet-level overhead estimate for stream3 frame batches.
const MAX_FRAME_BATCH_PACKET_OVERHEAD: u64 =
    crate::stream3::frame::MAX_FLOW_DATA_HEADER_OVERHEAD as u64;

/// A queue of frames grouped for a single path-secret entry, stored in per-priority buckets.
///
/// Frames are routed into one of [`Priority::LEVELS`] intrusive queues at push time
/// (O(1) per frame), preserving priority order all the way to transmission.  Level 0
/// (ACK / [`Priority::Ack`]) bypasses CWND; all other levels are subject to the
/// congestion window.
///
/// Because individual frames are routed into per-priority unsync lanes *before*
/// [`BatchFramesByPathSecret`] coalesces them, frames in a `FrameBatch` that comes out
/// of a given lane typically share the same priority class.  (Frames submitted as a
/// mixed-priority batch via [`PriorityInput`] are pre-sorted into buckets at submission
/// time, so this invariant is maintained across all paths.)
pub struct FrameBatch {
    /// Per-priority queues: index 0 = Ack (immediate, bypass CWND), 1–5 = data/control.
    queues: [Queue<Frame>; Priority::LEVELS],
    /// Accumulated wire cost per priority level (includes per-packet overhead).
    byte_costs: [u64; Priority::LEVELS],
    /// Total wire cost across all levels — used for batch-size budget checks.
    byte_cost: u64,
    /// Sticky sender assignment for this batch.  `VarInt::MAX` means no preference.
    sender_id: VarInt,
}

impl FrameBatch {
    #[inline]
    fn new(first: Entry<Frame>) -> Self {
        let frame_cost = first.byte_cost();
        // Per-packet overhead added once to the first frame's level.
        let byte_cost = MAX_FRAME_BATCH_PACKET_OVERHEAD.saturating_add(frame_cost);
        let sender_id = first.source_sender_id;

        let idx = first.priority().as_index();
        let mut queues = std::array::from_fn(|_| Queue::new());
        let mut byte_costs = [0u64; Priority::LEVELS];
        byte_costs[idx] = byte_cost;
        queues[idx].push_back(first);

        Self {
            queues,
            byte_costs,
            byte_cost,
            sender_id,
        }
    }

    #[inline]
    fn push_with_cost(&mut self, frame: Entry<Frame>, frame_cost: u64) {
        self.byte_cost = self.byte_cost.saturating_add(frame_cost);
        let idx = frame.priority().as_index();
        self.byte_costs[idx] = self.byte_costs[idx].saturating_add(frame_cost);
        self.queues[idx].push_back(frame);
    }

    /// Returns the sticky sender index if this batch requires sticky routing.
    #[inline]
    pub fn sender_id(&self) -> Option<usize> {
        if self.sender_id != VarInt::MAX {
            Some(self.sender_id.as_u64() as usize)
        } else {
            None
        }
    }

    #[inline]
    pub fn set_sender_id(&mut self, id: usize) {
        self.sender_id = VarInt::new(id as u64).unwrap_or(VarInt::MAX);
    }

    /// Returns the total number of frames across all priority levels.
    #[inline]
    #[cfg_attr(not(test), expect(dead_code))]
    pub fn len(&self) -> usize {
        self.queues.iter().map(|q| q.len()).sum()
    }

    /// Consumes the batch, returning the per-priority queues and their wire costs for
    /// O(1) insertion into `send::Context`.
    #[inline]
    pub fn into_queues(self) -> ([Queue<Frame>; Priority::LEVELS], [u64; Priority::LEVELS]) {
        (self.queues, self.byte_costs)
    }
}

impl ByteCost for FrameBatch {
    #[inline]
    fn byte_cost(&self) -> u64 {
        self.byte_cost
    }
}

impl PathSecretMapEntry for FrameBatch {
    #[inline]
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
        let Some(front) = self.queues.iter().find_map(|q| q.front()) else {
            unsafe {
                s2n_quic_core::assume!(false, "FrameBatch should always be non-empty");
            }
        };
        front.path_secret_entry()
    }
}

// ── BatchFramesByPathSecret ───────────────────────────────────────────────

/// Receiver combinator that batches consecutive frame entries by path-secret entry and byte budget.
///
/// Uses a timer to pace batch emissions: when a batch isn't full, waits up to
/// `overall_send_rate.nanos_for_bytes(target_bytes)` for more frames to accumulate before
/// emitting. This improves network utilization by sending fewer, fuller packets.
pub struct BatchFramesByPathSecret<R, Clk: precision::Clock> {
    inner: R,
    buffered: Option<Entry<Frame>>,
    pending_batch: Option<FrameBatch>,
    timer: Clk::Timer,
    wait_duration: core::time::Duration,
}

impl<R, Clk> BatchFramesByPathSecret<R, Clk>
where
    R: Receiver<Entry<Frame>>,
    Clk: precision::Clock,
{
    #[inline]
    pub fn new(inner: R, clock: &Clk, rate: Rate) -> Self {
        let target_bytes = u16::MAX as u64 - 3000;
        let wait_nanos = rate.nanos_for_bytes(target_bytes);
        Self {
            inner,
            buffered: None,
            pending_batch: None,
            timer: clock.timer(),
            wait_duration: core::time::Duration::from_nanos(wait_nanos),
        }
    }

    #[inline]
    fn take_first(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<Entry<Frame>>> {
        if let Some(frame) = self.buffered.take() {
            return Poll::Ready(Some(frame));
        }

        self.inner.poll_recv(cx, budget)
    }

    /// Try to append frames from inner to the batch. Returns true if the batch is full or
    /// must be emitted (different path secret / sticky conflict / channel closed).
    fn try_fill_batch(
        &mut self,
        batch: &mut FrameBatch,
        target_bytes: u64,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> FillResult {
        loop {
            if batch.byte_cost() >= target_bytes {
                return FillResult::Full;
            }

            match self.inner.poll_recv(cx, budget) {
                Poll::Ready(Some(frame_entry)) => {
                    if !Arc::ptr_eq(batch.path_secret_entry(), frame_entry.path_secret_entry()) {
                        self.buffered = Some(frame_entry);
                        return FillResult::Full;
                    }

                    let frame_sticky = frame_entry.source_sender_id;
                    if batch.sender_id != frame_sticky
                        && (batch.sender_id != VarInt::MAX && frame_sticky != VarInt::MAX)
                    {
                        self.buffered = Some(frame_entry);
                        return FillResult::Full;
                    }

                    if batch.sender_id == VarInt::MAX {
                        batch.sender_id = frame_sticky;
                    }

                    let frame_cost = frame_entry.byte_cost();
                    let next_cost = batch.byte_cost().saturating_add(frame_cost);
                    if next_cost > target_bytes {
                        self.buffered = Some(frame_entry);
                        return FillResult::Full;
                    }

                    batch.push_with_cost(frame_entry, frame_cost);
                }
                Poll::Ready(None) => return FillResult::Closed,
                Poll::Pending => return FillResult::Pending,
            }
        }
    }
}

enum FillResult {
    Full,
    Pending,
    Closed,
}

impl<R, Clk> Receiver<FrameBatch> for BatchFramesByPathSecret<R, Clk>
where
    R: Receiver<Entry<Frame>>,
    Clk: precision::Clock,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<FrameBatch>> {
        use precision::Timer;

        let target_bytes = u16::MAX as u64 - 3000;

        // If we have a pending batch from a previous poll, try to keep filling it.
        if let Some(mut batch) = self.pending_batch.take() {
            match self.try_fill_batch(&mut batch, target_bytes, cx, budget) {
                FillResult::Full => {
                    self.timer.cancel();
                    return Poll::Ready(Some(batch));
                }
                FillResult::Closed => {
                    self.timer.cancel();
                    return Poll::Ready(Some(batch));
                }
                FillResult::Pending => {
                    // Batch still not full — check if timer expired.
                    match self.timer.poll_ready(cx) {
                        Poll::Ready(()) => {
                            return Poll::Ready(Some(batch));
                        }
                        Poll::Pending => {
                            self.pending_batch = Some(batch);
                            return Poll::Pending;
                        }
                    }
                }
            }
        }

        // No pending batch — get first frame.
        let Some(first) = (match self.take_first(cx, budget) {
            Poll::Ready(frame) => frame,
            Poll::Pending => return Poll::Pending,
        }) else {
            return Poll::Ready(None);
        };

        let mut batch = FrameBatch::new(first);

        // Greedily fill from whatever is immediately available.
        match self.try_fill_batch(&mut batch, target_bytes, cx, budget) {
            FillResult::Full | FillResult::Closed => {
                return Poll::Ready(Some(batch));
            }
            FillResult::Pending => {}
        }

        // Batch isn't full — arm timer and wait for more frames.
        let now = self.timer.now();
        let target = now + self.wait_duration;
        self.timer.update(target);

        match self.timer.poll_ready(cx) {
            Poll::Ready(()) => Poll::Ready(Some(batch)),
            Poll::Pending => {
                self.pending_batch = Some(batch);
                Poll::Pending
            }
        }
    }

    #[inline]
    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── StickyRoute ───────────────────────────────────────────────────────────

/// Items that may carry a sticky sender preference, bypassing pick-two routing.
pub trait StickyRoute {
    /// Returns `Some(idx)` if this item must be routed to a specific sender.
    fn sticky_sender_idx(&self) -> Option<usize>;

    fn set_sender_id(&mut self, id: usize);
}

impl StickyRoute for FrameBatch {
    #[inline]
    fn sticky_sender_idx(&self) -> Option<usize> {
        FrameBatch::sender_id(self)
    }

    #[inline]
    fn set_sender_id(&mut self, id: usize) {
        FrameBatch::set_sender_id(self, id);
    }
}

impl<T: StickyRoute> StickyRoute for crate::intrusive_queue::Entry<T> {
    #[inline]
    fn sticky_sender_idx(&self) -> Option<usize> {
        (**self).sticky_sender_idx()
    }

    #[inline]
    fn set_sender_id(&mut self, id: usize) {
        (**self).set_sender_id(id);
    }
}

// ── PickTwo ───────────────────────────────────────────────────────────────

/// Receiver combinator that routes items to socket senders using pick-two path scheduling
/// from the path secret map entry associated with each item.
///
/// If an item implements [`StickyRoute`] and returns a sender index, that sender is used
/// directly (retransmissions must go back through the same socket). Otherwise pick-two
/// selects the sender with the earliest next-transmission time.
///
/// Implements `Receiver<()>` so it can be drained via `ReceiverExt::drain_budgeted`.
pub struct PickTwo<T, R, S> {
    rx: R,
    senders: Vec<S>,
    rng: crate::xorshift::Rng,
    pick_counters: Vec<crate::counter::Counter>,
    time_delta: crate::counter::Summary,
    value: PhantomData<fn() -> T>,
}

impl<T, R, S> PickTwo<T, R, S>
where
    T: ByteCost + PathSecretMapEntry + StickyRoute,
    R: Receiver<T>,
    S: UnboundedSender<T>,
{
    pub fn new(
        rx: R,
        senders: Vec<S>,
        rng: crate::xorshift::Rng,
        counter_registry: &crate::counter::Registry,
    ) -> Self {
        let pick_counters = (0..senders.len())
            .map(|i| counter_registry.register_nominal("pick_two.chosen", format_args!("send.{i}")))
            .collect();
        let time_delta = counter_registry.register_summary(
            "pick_two.time_delta",
            crate::counter::Unit::Microsecond,
        );
        Self {
            rx,
            senders,
            rng,
            pick_counters,
            time_delta,
            value: PhantomData,
        }
    }

    fn try_send_pick_two(
        mut value: T,
        senders: &mut Vec<S>,
        rng: &mut crate::xorshift::Rng,
        pick_counters: &[crate::counter::Counter],
        time_delta: &crate::counter::Summary,
    ) -> Result<(), T> {
        debug_assert!(!senders.is_empty());
        let chosen_idx = if let Some(sticky_idx) = value.sticky_sender_idx() {
            sticky_idx
        } else {
            let entry = value.path_secret_entry();
            let len = entry.socket_sender_count();

            if len <= 1 {
                0
            } else {
                let idx1 = rng.next_usize(len);
                let idx2 = if len == 2 {
                    idx1 ^ 1
                } else {
                    let mut idx2 = rng.next_usize(len - 1);
                    if idx2 >= idx1 {
                        idx2 += 1;
                    }
                    idx2
                };

                let time1 = entry.sender_next_transmission_micros(idx1);
                let time2 = entry.sender_next_transmission_micros(idx2);

                let delta_us = time1.abs_diff(time2);
                time_delta.record_value(delta_us);

                if time1 <= time2 {
                    idx1
                } else {
                    idx2
                }
            }
        };

        debug_assert!(
            chosen_idx < senders.len(),
            "sender index out of bounds: chosen={} senders={}",
            chosen_idx,
            senders.len()
        );

        pick_counters[chosen_idx].add(1);
        value.set_sender_id(chosen_idx);

        senders[chosen_idx].send(value)
    }
}

impl<T, R, S> Receiver<()> for PickTwo<T, R, S>
where
    T: ByteCost + PathSecretMapEntry + StickyRoute,
    R: Receiver<T>,
    S: UnboundedSender<T>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<()>> {
        let Some(value) = ready!(self.rx.poll_recv(cx, budget)) else {
            return Poll::Ready(None);
        };

        match Self::try_send_pick_two(
            value,
            &mut self.senders,
            &mut self.rng,
            &self.pick_counters,
            &self.time_delta,
        ) {
            Ok(()) => Poll::Ready(Some(())),
            Err(_) => Poll::Ready(None),
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.rx.on_consumed(bytes);
    }
}

// ── Assembler Receiver ───────────────────────────────────────────────────────

/// A [`Receiver`] adapter that resolves frame batches to per-peer contexts, pushes frames,
/// and yields assembled [`Segments`] ready for socket transmission.
///
/// Internally buffers the active context between polls: after pushing a batch's frames, it
/// calls [`assemble`] repeatedly until the CCA window fills, yielding one `Segments` per
/// poll. When the context is drained, it pulls the next batch from `inner`.
///
/// [`assemble`]: crate::stream3::endpoint::assemble::assemble
pub(crate) struct Assembler<R, Clk, C, A> {
    inner: R,
    clock: Clk,
    source_sender_id: s2n_quic_core::varint::VarInt,
    source_control_port: u16,
    gso: s2n_quic_platform::features::Gso,
    pool: crate::socket::pool::Pool,
    header_buf: Vec<u8>,
    cancelled_tx: C,
    ack_completions_tx: A,
    tx_wheel_tx: unsync::Sender<send::TxWheelAdapter>,
    pto_wheel_tx: unsync::Sender<send::PtoWheelAdapter>,
    idle_wheel_tx: unsync::Sender<send::IdleWheelAdapter>,
    pub(crate) counters: AssemblerCounters,
}

#[derive(Clone)]
pub(crate) struct AssemblerCounters {
    pub segments: crate::counter::Summary,
    pub packet_size: crate::counter::Summary,
    pub encrypt_time: crate::counter::Timer,
    pub tx_data: crate::counter::Counter,
    pub tx_probe: crate::counter::Counter,
    pub tx_frames_per_packet: crate::counter::Summary,
    pub tx_payload_size: crate::counter::Summary,
    pub q_tx_wheel: crate::counter::QueueGauge,
}

impl AssemblerCounters {
    pub fn new(registry: &crate::counter::Registry) -> Self {
        Self {
            segments: registry.register_summary("asm.segments", crate::counter::Unit::Count),
            packet_size: registry.register_summary("tx.packet_size", crate::counter::Unit::Byte),
            encrypt_time: registry.register_timer("tx.encrypt_time"),
            tx_data: registry.register("tx.data"),
            tx_probe: registry.register("tx.probe"),
            tx_frames_per_packet: registry
                .register_summary("tx.frames_per_packet", crate::counter::Unit::Count),
            tx_payload_size: registry
                .register_summary("tx.payload_size", crate::counter::Unit::Byte),
            q_tx_wheel: registry.register_queue_gauge("q.tx_wheel"),
        }
    }
}

impl<R, Clk, C, A> Assembler<R, Clk, C, A> {
    pub(crate) fn new(
        inner: R,
        clock: Clk,
        source_sender_id: s2n_quic_core::varint::VarInt,
        source_control_port: u16,
        gso: s2n_quic_platform::features::Gso,
        pool: crate::socket::pool::Pool,
        cancelled_tx: C,
        ack_completions_tx: A,
        tx_wheel_tx: unsync::Sender<send::TxWheelAdapter>,
        pto_wheel_tx: unsync::Sender<send::PtoWheelAdapter>,
        idle_wheel_tx: unsync::Sender<send::IdleWheelAdapter>,
        counters: AssemblerCounters,
    ) -> Self {
        Self {
            inner,
            clock,
            source_sender_id,
            source_control_port,
            gso,
            pool,
            header_buf: Vec::new(),
            cancelled_tx,
            ack_completions_tx,
            tx_wheel_tx,
            pto_wheel_tx,
            idle_wheel_tx,
            counters,
        }
    }
}

impl<R, Clk, C, A> Receiver<descriptor::Segments> for Assembler<R, Clk, C, A>
where
    R: Receiver<Rc<RefCell<send::Context>>>,
    Clk: precision::Clock,
    C: UnboundedSender<Queue<Frame>>,
    A: UnboundedSender<Queue<msg::Sender>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<descriptor::Segments>> {
        use crate::stream3::endpoint::assemble;

        let Some(context) = ready!(self.inner.poll_recv(cx, budget)) else {
            return Poll::Ready(None);
        };

        let (segments, wheel_interest) = {
            let mut context = context.borrow_mut();
            let mut cancelled = Queue::new();
            let mut ack_completions = Queue::new();
            let segments = assemble::assemble(
                &mut *context,
                &self.clock,
                self.source_sender_id,
                self.source_control_port,
                &self.gso,
                &self.pool,
                &mut self.header_buf,
                &mut cancelled,
                &mut ack_completions,
                &self.counters,
            );
            if !cancelled.is_empty() {
                let _ = self.cancelled_tx.send(cancelled);
            }
            if !ack_completions.is_empty() {
                let _ = self.ack_completions_tx.send(ack_completions);
            }
            let wheel_interest = context.wheel_interest(&self.clock);
            (segments, wheel_interest)
        };

        if wheel_interest.transmission {
            self.counters.q_tx_wheel.enqueue(1);
        }
        wheel_interest.dispatch(
            context,
            &mut self.tx_wheel_tx,
            &mut self.pto_wheel_tx,
            &mut self.idle_wheel_tx,
        );

        self.counters
            .segments
            .record_value(segments.as_ref().map_or(0, |s| s.segment_count() as u64));

        if let Some(segments) = segments {
            Poll::Ready(Some(segments))
        } else {
            budget.set_needs_wake();
            Poll::Pending
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── CompletionDispatcher ─────────────────────────────────────────────────

/// Receiver combinator that groups frames by completion channel and delivers each group
/// as a single batch (one lock acquisition per channel).
///
/// Each `poll_recv` call consumes as many frames from the upstream as budget allows.
/// If a frame belongs to the same completion channel as the current batch, it's appended.
/// Otherwise the accumulated batch is flushed via `send_batch` and a new batch begins with
/// the incoming frame.
///
/// Frames without a completion sender are silently dropped (best-effort frames).
///
/// The caller controls throughput via `drain_budgeted` — each returned `()` represents one
/// frame consumed from upstream.
pub(crate) struct CompletionDispatcher<R> {
    inner: R,
    batch: Queue<Frame>,
}

impl<R> CompletionDispatcher<R>
where
    R: Receiver<Entry<Frame>>,
{
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            batch: Queue::new(),
        }
    }

    fn current_queue_id(&self) -> Option<usize> {
        self.batch
            .front()
            .and_then(|f| f.completion.as_ref())
            .map(|c| c.queue_id())
    }

    #[inline]
    fn should_notify(frame: &Frame) -> bool {
        use crate::socket::channel::intrusive_queue::datagram_completion::SubscriptionMode;

        let Some(sender) = frame.completion.as_ref() else {
            return false;
        };

        match sender.subscription_mode() {
            SubscriptionMode::All => true,
            SubscriptionMode::FailuresOnly => {
                matches!(frame.status, frame::TransmissionStatus::Failed(_))
            }
        }
    }

    fn flush(&mut self) -> crate::flow::queue::AutoWake {
        if self.batch.is_empty() {
            return Default::default();
        }
        let sender = self.batch.front_mut().and_then(|f| f.completion.take());
        let batch = core::mem::take(&mut self.batch);
        if let Some(sender) = sender {
            sender.send_batch(batch).unwrap_or_default()
        } else {
            Default::default()
        }
    }
}

impl<R> Receiver<crate::flow::queue::AutoWake> for CompletionDispatcher<R>
where
    R: Receiver<Entry<Frame>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<crate::flow::queue::AutoWake>> {
        loop {
            if budget.is_exhausted() {
                budget.set_needs_wake();

                if self.batch.is_empty() {
                    return Poll::Pending;
                }

                return Poll::Ready(Some(self.flush()));
            }

            let frame = match self.inner.poll_recv(cx, budget) {
                Poll::Ready(Some(frame)) => frame,
                Poll::Ready(None) => {
                    if self.batch.is_empty() {
                        return Poll::Ready(None);
                    }

                    return Poll::Ready(Some(self.flush()));
                }
                Poll::Pending => {
                    let waker = self.flush();
                    if waker.is_some() {
                        return Poll::Ready(Some(waker));
                    }
                    return Poll::Pending;
                }
            };

            if !Self::should_notify(&frame) {
                continue;
            }

            if self.batch.is_empty() {
                self.batch.push_back(Entry::from(frame));
                continue;
            }

            let is_same_queue = self.current_queue_id()
                == Some(
                    frame
                        .completion
                        .as_ref()
                        .map(|c| c.queue_id())
                        .expect("invariant violation: frame.completion is None after should_notify returned true"),
                );

            if is_same_queue {
                self.batch.push_back(Entry::from(frame));
                continue;
            }

            let waker = self.flush();
            self.batch.push_back(Entry::from(frame));

            if waker.is_some() {
                return Poll::Ready(Some(waker));
            }
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}

// ── AckProcessor ────────────────────────────────────────────────────────

/// Processes `msg::Sender` messages: resolves the target send::Context and dispatches
/// either loss detection (ReceivedAck) or direct ACK state (PendingAck).
pub(crate) struct AckProcessor<R, Clk, Rand, C> {
    inner: R,
    send_caches: Vec<Rc<RefCell<send::Cache>>>,
    sender_idx_to_local: Vec<usize>,
    total_sender_ids: usize,
    clock: Clk,
    random: Rand,
    frame_tx: frame::SubmissionSender,
    completed_tx: C,
    cancelled_tx: C,
    tx_wheel_tx: unsync::Sender<send::TxWheelAdapter>,
    pto_wheel_tx: unsync::Sender<send::PtoWheelAdapter>,
    idle_wheel_tx: unsync::Sender<send::IdleWheelAdapter>,
    counters: Arc<super::counters::Send>,
    q_tx_wheel: crate::counter::QueueGauge,
}

impl<R, Clk, Rand, C> AckProcessor<R, Clk, Rand, C> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        inner: R,
        send_caches: Vec<Rc<RefCell<send::Cache>>>,
        sender_idx_to_local: Vec<usize>,
        total_sender_ids: usize,
        clock: Clk,
        random: Rand,
        frame_tx: frame::SubmissionSender,
        completed_tx: C,
        cancelled_tx: C,
        tx_wheel_tx: unsync::Sender<send::TxWheelAdapter>,
        pto_wheel_tx: unsync::Sender<send::PtoWheelAdapter>,
        idle_wheel_tx: unsync::Sender<send::IdleWheelAdapter>,
        counters: Arc<super::counters::Send>,
        q_tx_wheel: crate::counter::QueueGauge,
    ) -> Self {
        Self {
            inner,
            send_caches,
            sender_idx_to_local,
            total_sender_ids,
            clock,
            random,
            frame_tx,
            completed_tx,
            cancelled_tx,
            tx_wheel_tx,
            pto_wheel_tx,
            idle_wheel_tx,
            counters,
            q_tx_wheel,
        }
    }

    fn resolve_cache(&mut self, sender_idx: usize) -> Option<&mut Rc<RefCell<send::Cache>>> {
        if sender_idx >= self.total_sender_ids {
            self.counters.on_invalid_sender_idx();
            return None;
        }
        let Some(local_id) = self.sender_idx_to_local.get(sender_idx).copied() else {
            self.counters.on_invalid_sender_idx();
            return None;
        };
        let Some(cache) = self.send_caches.get_mut(local_id) else {
            self.counters.on_invalid_sender_idx();
            return None;
        };
        Some(cache)
    }

    fn dispatch_wheel_interest(
        &mut self,
        ctx_rc: Rc<RefCell<send::Context>>,
        wheel_interest: send::WheelInterest,
    ) {
        if wheel_interest.transmission {
            self.q_tx_wheel.enqueue(1);
        }
        wheel_interest.dispatch(
            ctx_rc,
            &mut self.tx_wheel_tx,
            &mut self.pto_wheel_tx,
            &mut self.idle_wheel_tx,
        );
    }
}

impl<R, Clk, Rand, C> Receiver<()> for AckProcessor<R, Clk, Rand, C>
where
    R: Receiver<Entry<msg::Sender>>,
    Clk: precision::Clock + s2n_quic_core::time::Clock,
    Rand: crate::random::Generator,
    C: UnboundedSender<Entry<Frame>>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<()>> {
        let Some(mut entry) = ready!(self.inner.poll_recv(cx, budget)) else {
            return Poll::Ready(None);
        };

        let sender_idx = entry.sender_idx();
        let is_ack = matches!(&*entry, msg::Sender::ReceivedAck { .. });
        if is_ack {
            self.counters.on_received_ack();
        }

        let Some(cache) = self.resolve_cache(sender_idx) else {
            return Poll::Ready(Some(()));
        };

        match &mut *entry {
            msg::Sender::ReceivedAck {
                payload,
                path_secret_entry,
                ack_delay,
                ..
            } => {
                let ctx_rc = {
                    let cache = cache.borrow();
                    cache.get(path_secret_entry.id())
                };

                let Some(ctx_rc) = ctx_rc else {
                    self.counters.on_received_ack_no_ctx();
                    return Poll::Ready(Some(()));
                };

                let mut lost_queue = PriorityInput::default();

                let wheel_interest = {
                    let mut ctx = ctx_rc.borrow_mut();
                    let interest = ctx.process_ack_payload(
                        payload,
                        *ack_delay,
                        &self.counters,
                        &mut self.completed_tx,
                        &mut lost_queue,
                        &mut self.cancelled_tx,
                        &self.clock,
                        &mut self.random,
                    );
                    self.counters.on_rtt(ctx.rtt_estimator.smoothed_rtt());
                    interest
                };

                if !lost_queue.is_empty() {
                    self.counters.on_lost(lost_queue.len() as u64);
                    let _ = self.frame_tx.send_batch(lost_queue);
                }

                self.dispatch_wheel_interest(ctx_rc, wheel_interest);
            }
            msg::Sender::PendingAck(_) => {
                let ctx_rc = {
                    let mut cache = cache.borrow_mut();
                    match cache.get_or_insert(entry.path_secret_entry()) {
                        Ok(ctx) => ctx,
                        Err(error) => {
                            tracing::warn!(?error, "dropping ack: send context not ready");
                            return Poll::Ready(Some(()));
                        }
                    }
                };

                let wheel_interest = {
                    let mut ctx = ctx_rc.borrow_mut();
                    ctx.pending_acks.push_back(entry);
                    ctx.wheel_interest(&self.clock)
                };

                self.dispatch_wheel_interest(ctx_rc, wheel_interest);
            }
        }

        Poll::Ready(Some(()))
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}
