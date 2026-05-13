// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Channel combinators specific to the stream3 endpoint pipeline.
//!
//! These are `Receiver` adapters that compose with the generic combinators in
//! `socket::channel` but carry stream3-specific knowledge (path-secret routing,
//! frame batching by peer, etc).

use crate::{
    clock::precision,
    datagram::batch::Priority,
    intrusive_queue::{Entry, Queue},
    path::secret::map::Entry as PathSecretEntry,
    socket::{
        channel::{intrusive_queue::unsync, ByteCost, Receiver, UnboundedSender},
        pool::descriptor,
        rate::Rate,
    },
    stream3::{endpoint::send, frame::Frame},
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
///
/// Uses the same upper-bound constant as datagram partials so batching leaves room for packet
/// fields that are added later by workers (credentials, packet number, routing, tag, etc).
const MAX_FRAME_BATCH_PACKET_OVERHEAD: u64 =
    crate::packet::datagram::partial::MAX_FLOW_DATA_HEADER_OVERHEAD as u64;

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
    pub fn len(&self) -> usize {
        self.queues.iter().map(|q| q.len()).sum()
    }

    /// Returns true when this batch contains no frames.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queues.iter().all(|q| q.is_empty())
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

const BATCH_FRAMES_POLL_BUDGET: usize = 100;

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
    fn take_first(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<Entry<Frame>>> {
        if let Some(frame) = self.buffered.take() {
            return Poll::Ready(Some(frame));
        }

        self.inner.poll_recv(cx)
    }

    /// Try to append frames from inner to the batch. Returns true if the batch is full or
    /// must be emitted (different path secret / sticky conflict / channel closed).
    fn try_fill_batch(
        &mut self,
        batch: &mut FrameBatch,
        target_bytes: u64,
        cx: &mut task::Context<'_>,
    ) -> FillResult {
        for _ in 0..BATCH_FRAMES_POLL_BUDGET {
            if batch.byte_cost() >= target_bytes {
                return FillResult::Full;
            }

            match self.inner.poll_recv(cx) {
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
        FillResult::Full
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
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<FrameBatch>> {
        use precision::Timer;

        let target_bytes = u16::MAX as u64 - 3000;

        // If we have a pending batch from a previous poll, try to keep filling it.
        if let Some(mut batch) = self.pending_batch.take() {
            match self.try_fill_batch(&mut batch, target_bytes, cx) {
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
        let Some(first) = (match self.take_first(cx) {
            Poll::Ready(frame) => frame,
            Poll::Pending => return Poll::Pending,
        }) else {
            return Poll::Ready(None);
        };

        let mut batch = FrameBatch::new(first);

        // Greedily fill from whatever is immediately available.
        match self.try_fill_batch(&mut batch, target_bytes, cx) {
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
    value: PhantomData<fn() -> T>,
}

impl<T, R, S> PickTwo<T, R, S>
where
    T: ByteCost + PathSecretMapEntry + StickyRoute,
    R: Receiver<T>,
    S: UnboundedSender<T>,
{
    pub fn new(rx: R, senders: Vec<S>, rng: crate::xorshift::Rng) -> Self {
        Self {
            rx,
            senders,
            rng,
            value: PhantomData,
        }
    }

    fn try_send_pick_two(
        mut value: T,
        senders: &mut Vec<S>,
        rng: &mut crate::xorshift::Rng,
    ) -> Result<(), T> {
        debug_assert!(!senders.is_empty());
        let chosen_idx = if let Some(sticky_idx) = value.sticky_sender_idx() {
            sticky_idx
        } else {
            value
                .path_secret_entry()
                .pick_sender_by_next_transmission(rng)
        };

        debug_assert!(
            chosen_idx < senders.len(),
            "sender index out of bounds: chosen={} senders={}",
            chosen_idx,
            senders.len()
        );

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
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<()>> {
        let Some(value) = ready!(self.rx.poll_recv(cx)) else {
            return Poll::Ready(None);
        };

        match Self::try_send_pick_two(value, &mut self.senders, &mut self.rng) {
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
pub(crate) struct Assembler<R, Clk, C> {
    inner: R,
    clock: Clk,
    source_sender_id: s2n_quic_core::varint::VarInt,
    source_control_port: u16,
    gso: s2n_quic_platform::features::Gso,
    pool: crate::socket::pool::Pool,
    header_buf: Vec<u8>,
    cancelled_tx: C,
    tx_wheel_tx: unsync::Sender<send::TxWheelAdapter>,
    pto_wheel_tx: unsync::Sender<send::PtoWheelAdapter>,
    idle_wheel_tx: unsync::Sender<send::IdleWheelAdapter>,
    pub(crate) counters: AssemblerCounters,
}

#[derive(Clone)]
pub(crate) struct AssemblerCounters {
    pub segments: crate::counter::Summary,
    pub q_tx_wheel: crate::counter::QueueGauge,
}

impl AssemblerCounters {
    pub fn new(registry: &crate::counter::Registry) -> Self {
        Self {
            segments: registry.register_summary("asm.segments", crate::counter::Unit::Count),
            q_tx_wheel: registry.register_queue_gauge("q.tx_wheel"),
        }
    }
}

impl<R, Clk, C> Assembler<R, Clk, C> {
    pub(crate) fn new(
        inner: R,
        clock: Clk,
        source_sender_id: s2n_quic_core::varint::VarInt,
        source_control_port: u16,
        gso: s2n_quic_platform::features::Gso,
        pool: crate::socket::pool::Pool,
        cancelled_tx: C,
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
            tx_wheel_tx,
            pto_wheel_tx,
            idle_wheel_tx,
            counters,
        }
    }
}

impl<R, Clk, C> Receiver<descriptor::Segments> for Assembler<R, Clk, C>
where
    R: Receiver<Rc<RefCell<send::Context>>>,
    Clk: precision::Clock,
    C: UnboundedSender<Queue<Frame>>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<descriptor::Segments>> {
        use crate::stream3::endpoint::assemble;

        let Some(context) = ready!(self.inner.poll_recv(cx)) else {
            return Poll::Ready(None);
        };

        let (segments, wheel_interest) = {
            let mut context = context.borrow_mut();
            let segments = assemble::assemble(
                &mut *context,
                &self.clock,
                self.source_sender_id,
                self.source_control_port,
                &self.gso,
                &self.pool,
                &mut self.header_buf,
                &mut self.cancelled_tx,
            );
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
            cx.waker().wake_by_ref();
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
/// Each `poll_recv` call consumes one frame from the upstream. If the frame belongs to the
/// same completion channel as the current batch, it's appended. Otherwise the accumulated
/// batch is flushed via `send_batch` and a new batch begins with the incoming frame.
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
    ) -> Poll<Option<crate::flow::queue::AutoWake>> {
        let frame = match self.inner.poll_recv(cx) {
            Poll::Ready(Some(frame)) => frame,
            Poll::Ready(None) => {
                let waker = self.flush();
                return Poll::Ready(Some(waker));
            }
            Poll::Pending => {
                let waker = self.flush();
                if waker.is_some() {
                    return Poll::Ready(Some(waker));
                }
                return Poll::Pending;
            }
        };

        let incoming_id = frame.completion.as_ref().map(|c| c.queue_id());

        let Some(_) = incoming_id else {
            return Poll::Ready(Some(Default::default()));
        };

        if self.current_queue_id() == incoming_id {
            self.batch.push_back(Entry::from(frame));
            Poll::Ready(Some(Default::default()))
        } else {
            let waker = self.flush();
            self.batch.push_back(Entry::from(frame));
            Poll::Ready(Some(waker))
        }
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}
