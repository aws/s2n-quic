// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Channel combinators specific to the stream endpoint pipeline.
//!
//! These are `Receiver` adapters that compose with the generic combinators in
//! `socket::channel` but carry stream-specific knowledge (path-secret routing,
//! frame batching by peer, etc).

use crate::{
    endpoint::{
        frame::{self, Frame, Priority, PriorityInput},
        id::{Id, IdMap, LocalSendSocketId, LocalSenderId},
    },
    intrusive::{Entry, Queue},
    path::secret::map::Entry as PathSecretEntry,
    socket::{
        channel::{Budget, ByteCost, ImmediateQueueStatus, Receiver, UnboundedSender},
        pool::descriptor,
        rate::Rate,
    },
    stream::endpoint::{msg, send},
    time::precision,
    tracing::*,
};
use core::task::{self, Poll};
use s2n_quic_core::{packet::number::PacketNumber, ready};
use std::{cell::RefCell, marker::PhantomData, rc::Rc, sync::Arc};

#[cfg(test)]
mod tests;

// ── PathSecretMapEntry ────────────────────────────────────────────────────

/// Routing key accessor for stream send-side load-balancing tasks.
pub trait PathSecretMapEntry {
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry>;
}

impl<T> PathSecretMapEntry for crate::intrusive::Entry<T>
where
    T: PathSecretMapEntry,
{
    #[inline]
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
        (**self).path_secret_entry()
    }
}

impl PathSecretMapEntry for Frame {
    #[inline]
    fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
        &self.path_secret_entry
    }
}

// ── ID-mapped dispatch ─────────────────────────────────────────────────────

/// Items that expose a logical ID used for routing to a destination sender.
pub trait HasId<I> {
    fn id(&self) -> I;
}

/// Dispatches items by first mapping a logical ID to a destination ID, then
/// forwarding to the destination sender.
pub struct MappedSender<T, I, D, S> {
    senders: IdMap<D, S>,
    id_map: IdMap<I, D>,
    _value: PhantomData<fn() -> T>,
}

impl<T, I: Clone, D: Clone, S: Clone> Clone for MappedSender<T, I, D, S> {
    fn clone(&self) -> Self {
        Self {
            senders: self.senders.clone(),
            id_map: self.id_map.clone(),
            _value: PhantomData,
        }
    }
}

impl<T, I, D, S> MappedSender<T, I, D, S> {
    #[inline]
    pub fn new(senders: IdMap<D, S>, id_map: IdMap<I, D>) -> Self {
        Self {
            senders,
            id_map,
            _value: PhantomData,
        }
    }
}

impl<T, I, D, S> UnboundedSender<T> for MappedSender<T, I, D, S>
where
    T: HasId<I>,
    I: Into<usize> + Copy,
    D: Into<usize> + Copy,
    S: UnboundedSender<T>,
{
    #[inline]
    fn send(&mut self, value: T) -> Result<(), T> {
        let logical_id = value.id();
        let logical_id_idx = logical_id.into();
        let destination_id = self.id_map.get(logical_id).copied().unwrap_or_else(|| {
            panic!(
                "logical id {} not found in mapped sender id map (len={})",
                logical_id_idx,
                self.id_map.len()
            )
        });
        let destination_id_idx = destination_id.into();
        let senders_len = self.senders.len();
        let sender = self.senders.get_mut(destination_id).unwrap_or_else(|| {
            panic!(
                "destination id {} (from logical id {}) not found in mapped sender senders (len={})",
                destination_id_idx,
                logical_id_idx,
                senders_len
            )
        });
        sender.send(value)
    }
}

// ── FrameBatch ────────────────────────────────────────────────────────────

/// Conservative packet-level overhead estimate for stream frame batches.
const MAX_FRAME_BATCH_PACKET_OVERHEAD: u64 = frame::MAX_QUEUE_DATA_HEADER_OVERHEAD as u64;

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
    sender_id: LocalSenderId,
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
    pub fn sender_id(&self) -> Option<LocalSenderId> {
        if self.sender_id != LocalSenderId::UNSPECIFIED {
            Some(self.sender_id)
        } else {
            None
        }
    }

    #[inline]
    pub fn set_sender_id(&mut self, id: LocalSenderId) {
        self.sender_id = id;
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

#[cfg(test)]
impl FrameBatch {
    /// Wrap a single frame in a `FrameBatch`, for use in unit tests that need to
    /// call `send::Context::push_batch` without going through the full combinator pipeline.
    pub fn single(frame: Entry<Frame>) -> Self {
        Self::new(frame)
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
            panic!("FrameBatch should never be empty");
            // unsafe {
            // s2n_quic_core::assume!(false, "FrameBatch should always be non-empty");
            // }
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
                        && (batch.sender_id != LocalSenderId::UNSPECIFIED
                            && frame_sticky != LocalSenderId::UNSPECIFIED)
                    {
                        self.buffered = Some(frame_entry);
                        return FillResult::Full;
                    }

                    if batch.sender_id == LocalSenderId::UNSPECIFIED {
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
    fn sticky_sender_idx(&self) -> Option<LocalSenderId>;

    fn set_sender_id(&mut self, id: LocalSenderId);
}

impl StickyRoute for FrameBatch {
    #[inline]
    fn sticky_sender_idx(&self) -> Option<LocalSenderId> {
        FrameBatch::sender_id(self)
    }

    #[inline]
    fn set_sender_id(&mut self, id: LocalSenderId) {
        FrameBatch::set_sender_id(self, id);
    }
}

impl<T: StickyRoute> StickyRoute for crate::intrusive::Entry<T> {
    #[inline]
    fn sticky_sender_idx(&self) -> Option<LocalSenderId> {
        (**self).sticky_sender_idx()
    }

    #[inline]
    fn set_sender_id(&mut self, id: LocalSenderId) {
        (**self).set_sender_id(id);
    }
}

// ── PickTwo ───────────────────────────────────────────────────────────────

/// Receiver combinator that routes items to socket senders using pick-two path scheduling
/// from the path secret map entry associated with each item.
///
/// If an item implements [`StickyRoute`] and returns a sender index, that sender is used
/// directly (retransmissions must go back through the same socket). Otherwise pick-two
/// selects the sender with the lowest load score.
///
/// Implements `Receiver<()>` so it can be drained via `ReceiverExt::drain_budgeted`.
pub struct PickTwo<T, R, S, Clk> {
    rx: R,
    senders: IdMap<LocalSenderId, S>,
    socket_edts: crate::endpoint::edt::Local,
    clock: Clk,
    rng: crate::xorshift::Rng,
    pick_counters: IdMap<LocalSenderId, crate::counter::Counter>,
    rejected_counters: IdMap<LocalSenderId, crate::counter::Summary>,
    score_delta: crate::counter::Summary,
    value: PhantomData<fn() -> T>,
}

impl<T, R, S, Clk> PickTwo<T, R, S, Clk>
where
    T: ByteCost + PathSecretMapEntry + StickyRoute,
    R: Receiver<T>,
    S: UnboundedSender<T>,
    Clk: precision::Clock,
{
    pub fn new(
        rx: R,
        senders: IdMap<LocalSenderId, S>,
        clock: Clk,
        per_socket_send_rate: Rate,
        rng: crate::xorshift::Rng,
        counter_registry: &crate::counter::Registry,
    ) -> Self {
        let socket_edts = crate::endpoint::edt::Local::new(senders.len(), per_socket_send_rate);
        let pick_counters = senders
            .iter()
            .map(|(id, _)| {
                let counter =
                    counter_registry.register_nominal("pick_two.chosen", format_args!("send.{id}"));
                (id, counter)
            })
            .collect();
        let rejected_counters = senders
            .iter()
            .map(|(id, _)| {
                let summary = counter_registry.register_nominal_summary(
                    "pick_two.rejected",
                    format_args!("send.{id}"),
                    crate::counter::Unit::Microsecond,
                );
                (id, summary)
            })
            .collect();
        let score_delta = counter_registry
            .register_summary("pick_two.score_delta", crate::counter::Unit::Microsecond);
        Self {
            rx,
            senders,
            socket_edts,
            clock,
            rng,
            pick_counters,
            rejected_counters,
            score_delta,
            value: PhantomData,
        }
    }

    fn try_send_pick_two(
        mut value: T,
        senders: &mut IdMap<LocalSenderId, S>,
        socket_edts: &mut crate::endpoint::edt::Local,
        now: precision::Timestamp,
        rng: &mut crate::xorshift::Rng,
        pick_counters: &IdMap<LocalSenderId, crate::counter::Counter>,
        rejected_counters: &IdMap<LocalSenderId, crate::counter::Summary>,
        score_delta: &crate::counter::Summary,
    ) -> Result<(), T> {
        debug_assert!(!senders.is_empty());
        debug_assert_eq!(senders.len(), pick_counters.len());
        debug_assert_eq!(senders.len(), rejected_counters.len());
        debug_assert_eq!(senders.len(), socket_edts.len());
        let entry = value.path_secret_entry();
        debug_assert_eq!(senders.len(), entry.socket_sender_count());

        let byte_cost = value.byte_cost();
        let chosen_idx = if let Some(sticky_idx) = value.sticky_sender_idx() {
            sticky_idx
        } else {
            let len = senders.len();
            if len <= 1 {
                LocalSenderId::from_index(0)
            } else {
                let idx1 = LocalSenderId::from_index(rng.next_usize(len));
                let idx2 = if len == 2 {
                    LocalSenderId::from_index(idx1.as_usize() ^ 1)
                } else {
                    let mut raw2 = rng.next_usize(len - 1);
                    if raw2 >= idx1.as_usize() {
                        raw2 += 1;
                    }
                    LocalSenderId::from_index(raw2)
                };

                let score1 = entry
                    .sender_load_score(idx1)
                    .saturating_add(socket_edts.load_score(idx1));
                let score2 = entry
                    .sender_load_score(idx2)
                    .saturating_add(socket_edts.load_score(idx2));

                let delta = score1.abs_diff(score2);
                score_delta.record_value(delta);

                if score1 <= score2 {
                    rejected_counters[idx2].record_value(delta);
                    idx1
                } else {
                    rejected_counters[idx1].record_value(delta);
                    idx2
                }
            }
        };

        debug_assert!(
            chosen_idx.as_usize() < senders.len(),
            "sender index out of bounds: chosen={} senders={}",
            chosen_idx,
            senders.len()
        );

        socket_edts.advance(chosen_idx, now, byte_cost);
        pick_counters[chosen_idx].add(1);
        value.set_sender_id(chosen_idx);

        senders[chosen_idx].send(value)
    }
}

impl<T, R, S, Clk> Receiver<()> for PickTwo<T, R, S, Clk>
where
    T: ByteCost + PathSecretMapEntry + StickyRoute,
    R: Receiver<T>,
    S: UnboundedSender<T>,
    Clk: precision::Clock,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>, budget: &mut Budget) -> Poll<Option<()>> {
        let Some(value) = ready!(self.rx.poll_recv(cx, budget)) else {
            return Poll::Ready(None);
        };

        let now = self.clock.now();
        match Self::try_send_pick_two(
            value,
            &mut self.senders,
            &mut self.socket_edts,
            now,
            &mut self.rng,
            &self.pick_counters,
            &self.rejected_counters,
            &self.score_delta,
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
/// [`assemble`]: crate::stream::endpoint::assemble::assemble
pub(crate) struct Assembler<R, Clk, C, A> {
    inner: R,
    clock: Clk,
    source_sender_id: LocalSenderId,
    source_control_port: u16,
    gso: s2n_quic_platform::features::Gso,
    pool: crate::socket::pool::Pool,
    header_buf: Vec<u8>,
    cancelled_tx: C,
    ack_completions_tx: A,
    pub(crate) counters: AssemblerCounters,
    send_counters: Rc<super::counters::Send>,
}

#[derive(Clone)]
pub(crate) struct AssemblerCounters {
    pub segments: crate::counter::Summary,
    pub max_datagram_size: crate::counter::Summary,
    pub packet_size: crate::counter::Summary,
    pub encrypt_time: crate::counter::Timer,
    pub tx_data: crate::counter::Counter,
    pub tx_probe: crate::counter::Counter,
    pub tx_frames_per_packet: crate::counter::Summary,
    pub tx_payload_size: crate::counter::Summary,

    // Per-frame-type TX counters (one per transmitted frame, all phases).
    pub tx_frame_queue_init: crate::counter::Counter,
    pub tx_frame_queue_data: crate::counter::Counter,
    pub tx_frame_queue_data_fin: crate::counter::Counter,
    pub tx_frame_queue_control: crate::counter::Counter,
    pub tx_frame_queue_max_data: crate::counter::Counter,
    pub tx_frame_queue_reset: crate::counter::Counter,
    pub tx_frame_queue_init_reset: crate::counter::Counter,
    pub tx_frame_queue_init_fin: crate::counter::Counter,
    pub tx_frame_queue_init_validate: crate::counter::Counter,
    pub tx_frame_queue_validate_request: crate::counter::Counter,
    pub tx_frame_ack: crate::counter::Counter,

    // Per-frame-type probe TX counters (Phase 2 retransmit + Phase 3 PTO bypass).
    pub tx_probe_frame_queue_init: crate::counter::Counter,
    pub tx_probe_frame_queue_data: crate::counter::Counter,
    pub tx_probe_frame_queue_data_fin: crate::counter::Counter,
    pub tx_probe_frame_queue_control: crate::counter::Counter,
    pub tx_probe_frame_queue_max_data: crate::counter::Counter,
    pub tx_probe_frame_queue_reset: crate::counter::Counter,
    pub tx_probe_frame_queue_init_reset: crate::counter::Counter,
    pub tx_probe_frame_queue_init_fin: crate::counter::Counter,
    pub tx_probe_frame_queue_init_validate: crate::counter::Counter,
    pub tx_probe_frame_queue_validate_request: crate::counter::Counter,
}

impl AssemblerCounters {
    pub fn new(registry: &crate::counter::Registry) -> Self {
        Self {
            segments: registry.register_summary("asm.segments", crate::counter::Unit::Count),
            max_datagram_size: registry
                .register_summary("asm.max_datagram_size", crate::counter::Unit::Byte),
            packet_size: registry.register_summary("tx.packet_size", crate::counter::Unit::Byte),
            encrypt_time: registry.register_timer("tx.encrypt_time"),
            tx_data: registry.register("tx.data"),
            tx_probe: registry.register("tx.probe"),
            tx_frames_per_packet: registry
                .register_summary("tx.frames_per_packet", crate::counter::Unit::Count),
            tx_payload_size: registry
                .register_summary("tx.payload_size", crate::counter::Unit::Byte),

            tx_frame_queue_init: registry.register_nominal("tx.frame", "queue_init"),
            tx_frame_queue_data: registry.register_nominal("tx.frame", "queue_data"),
            tx_frame_queue_data_fin: registry.register_nominal("tx.frame", "queue_data_fin"),
            tx_frame_queue_control: registry.register_nominal("tx.frame", "queue_control"),
            tx_frame_queue_max_data: registry.register_nominal("tx.frame", "queue_max_data"),
            tx_frame_queue_reset: registry.register_nominal("tx.frame", "queue_reset"),
            tx_frame_queue_init_reset: registry.register_nominal("tx.frame", "queue_init_reset"),
            tx_frame_queue_init_fin: registry.register_nominal("tx.frame", "queue_init_fin"),
            tx_frame_queue_init_validate: registry
                .register_nominal("tx.frame", "queue_init_validate"),
            tx_frame_queue_validate_request: registry
                .register_nominal("tx.frame", "queue_validate_request"),
            tx_frame_ack: registry.register_nominal("tx.frame", "ack"),

            tx_probe_frame_queue_init: registry.register_nominal("tx.probe.frame", "queue_init"),
            tx_probe_frame_queue_data: registry.register_nominal("tx.probe.frame", "queue_data"),
            tx_probe_frame_queue_data_fin: registry
                .register_nominal("tx.probe.frame", "queue_data_fin"),
            tx_probe_frame_queue_control: registry
                .register_nominal("tx.probe.frame", "queue_control"),
            tx_probe_frame_queue_max_data: registry
                .register_nominal("tx.probe.frame", "queue_max_data"),
            tx_probe_frame_queue_reset: registry.register_nominal("tx.probe.frame", "queue_reset"),
            tx_probe_frame_queue_init_reset: registry
                .register_nominal("tx.probe.frame", "queue_init_reset"),
            tx_probe_frame_queue_init_fin: registry
                .register_nominal("tx.probe.frame", "queue_init_fin"),
            tx_probe_frame_queue_init_validate: registry
                .register_nominal("tx.probe.frame", "queue_init_validate"),
            tx_probe_frame_queue_validate_request: registry
                .register_nominal("tx.probe.frame", "queue_validate_request"),
        }
    }

    /// Bump the per-frame-type TX counter for the given frame header.
    #[inline]
    pub fn on_tx_frame(&self, header: &frame::Header) {
        match header {
            frame::Header::QueueInit { .. } => self.tx_frame_queue_init.add(1),
            frame::Header::QueueData { is_fin: false, .. } => self.tx_frame_queue_data.add(1),
            frame::Header::QueueData { is_fin: true, .. } => self.tx_frame_queue_data_fin.add(1),
            frame::Header::QueueControl { .. } => self.tx_frame_queue_control.add(1),
            frame::Header::QueueMaxData { .. } => self.tx_frame_queue_max_data.add(1),
            frame::Header::QueueReset { .. } => self.tx_frame_queue_reset.add(1),
            frame::Header::QueueInitReset { .. } => self.tx_frame_queue_init_reset.add(1),
            frame::Header::QueueInitFin { .. } => self.tx_frame_queue_init_fin.add(1),
            frame::Header::QueueInitValidate { .. } => self.tx_frame_queue_init_validate.add(1),
            frame::Header::QueueValidateRequest { .. } => self.tx_frame_queue_validate_request.add(1),
            frame::Header::Ack { .. } => self.tx_frame_ack.add(1),
        }
    }

    /// Bump the per-frame-type probe TX counter for the given frame header.
    /// Called for frames assembled as PTO probes (Phase 2 retransmit and Phase 3
    /// CWND-bypass).
    #[inline]
    pub fn on_probe_frame(&self, header: &frame::Header) {
        match header {
            frame::Header::QueueInit { .. } => self.tx_probe_frame_queue_init.add(1),
            frame::Header::QueueData { is_fin: false, .. } => self.tx_probe_frame_queue_data.add(1),
            frame::Header::QueueData { is_fin: true, .. } => {
                self.tx_probe_frame_queue_data_fin.add(1)
            }
            frame::Header::QueueControl { .. } => self.tx_probe_frame_queue_control.add(1),
            frame::Header::QueueMaxData { .. } => self.tx_probe_frame_queue_max_data.add(1),
            frame::Header::QueueReset { .. } => self.tx_probe_frame_queue_reset.add(1),
            frame::Header::QueueInitReset { .. } => self.tx_probe_frame_queue_init_reset.add(1),
            frame::Header::QueueInitFin { .. } => self.tx_probe_frame_queue_init_fin.add(1),
            frame::Header::QueueInitValidate { .. } => self.tx_probe_frame_queue_init_validate.add(1),
            frame::Header::QueueValidateRequest { .. } => {
                self.tx_probe_frame_queue_validate_request.add(1)
            }
            // ACK frames are stripped before inflight insertion and are never retransmitted
            // as probes; this branch should be unreachable in practice.
            frame::Header::Ack { .. } => {
                debug_assert!(false, "ACK frames should never appear as inflight entries")
            }
        }
    }
}

type AssemblerOutput = (
    Rc<RefCell<send::Context>>,
    send::WheelInterest,
    Option<descriptor::Segments>,
);

impl<R, Clk, C, A> Assembler<R, Clk, C, A> {
    pub(crate) fn new(
        inner: R,
        clock: Clk,
        source_sender_id: LocalSenderId,
        source_control_port: u16,
        gso: s2n_quic_platform::features::Gso,
        pool: crate::socket::pool::Pool,
        cancelled_tx: C,
        ack_completions_tx: A,
        counters: AssemblerCounters,
        send_counters: Rc<super::counters::Send>,
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
            counters,
            send_counters,
        }
    }
}

impl<R, Clk, C, A> Receiver<AssemblerOutput> for Assembler<R, Clk, C, A>
where
    R: Receiver<(Rc<RefCell<send::Context>>, ImmediateQueueStatus)>,
    Clk: precision::Clock,
    C: UnboundedSender<Queue<Frame>>,
    A: UnboundedSender<Queue<msg::Sender>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<AssemblerOutput>> {
        use crate::stream::endpoint::assemble;

        let Some((context, immediate_queue_status)) = ready!(self.inner.poll_recv(cx, budget))
        else {
            return Poll::Ready(None);
        };

        let (segments, wheel_interest) = {
            let mut context = context.borrow_mut();
            let mut cancelled = Queue::new();
            let mut ack_completions = Queue::new();
            let segments = assemble::assemble(
                &mut context,
                immediate_queue_status,
                &self.clock,
                self.source_sender_id,
                self.source_control_port,
                &self.gso,
                &self.pool,
                &mut self.header_buf,
                &mut cancelled,
                &mut ack_completions,
                &self.counters,
                &self.send_counters,
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

        self.counters
            .segments
            .record_value(segments.as_ref().map_or(0, |s| s.segment_count() as u64));

        Poll::Ready(Some((context, wheel_interest, segments)))
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
        use crate::socket::channel::intrusive::datagram_completion::SubscriptionMode;

        let Some(sender) = frame.completion.as_ref() else {
            return false;
        };
        debug_assert!(
            !matches!(frame.status, frame::TransmissionStatus::Pending),
            "completion notification must not be emitted for pending frames"
        );

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
                self.batch.push_back(frame);
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
                self.batch.push_back(frame);
                continue;
            }

            let waker = self.flush();
            self.batch.push_back(frame);

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
    send_caches: IdMap<LocalSendSocketId, Rc<RefCell<send::Cache>>>,
    sender_idx_to_local: IdMap<LocalSenderId, LocalSendSocketId>,
    total_sender_ids: usize,
    clock: Clk,
    random: Rand,
    frame_tx: frame::SubmissionSender,
    completed_tx: C,
    cancelled_tx: C,
    invalid_sender_idx: crate::counter::Counter,
    /// Storage space for packet number/recovery states
    deferred: Vec<PacketNumber>,
}

impl<R, Clk, Rand, C> AckProcessor<R, Clk, Rand, C> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        inner: R,
        send_caches: IdMap<LocalSendSocketId, Rc<RefCell<send::Cache>>>,
        sender_idx_to_local: IdMap<LocalSenderId, LocalSendSocketId>,
        total_sender_ids: usize,
        clock: Clk,
        random: Rand,
        frame_tx: frame::SubmissionSender,
        completed_tx: C,
        cancelled_tx: C,
        invalid_sender_idx: crate::counter::Counter,
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
            invalid_sender_idx,
            deferred: Vec::with_capacity(8),
        }
    }

    fn resolve_cache(
        &mut self,
        sender_idx: LocalSenderId,
    ) -> Option<&mut Rc<RefCell<send::Cache>>> {
        if sender_idx.as_usize() >= self.total_sender_ids {
            self.invalid_sender_idx.add(1);
            return None;
        }
        let Some(local_id) = self.sender_idx_to_local.get(sender_idx).copied() else {
            self.invalid_sender_idx.add(1);
            return None;
        };
        let Some(cache) = self.send_caches.get_mut(local_id) else {
            self.invalid_sender_idx.add(1);
            return None;
        };
        Some(cache)
    }
}

type MaybeWheelDispatch = Option<(Rc<RefCell<send::Context>>, send::WheelInterest)>;

impl<R, Clk, Rand, C> Receiver<MaybeWheelDispatch> for AckProcessor<R, Clk, Rand, C>
where
    R: Receiver<Entry<msg::Sender>>,
    Clk: precision::Clock + s2n_quic_core::time::Clock,
    Rand: s2n_quic_core::random::Generator,
    C: UnboundedSender<Entry<Frame>>,
{
    fn poll_recv(
        &mut self,
        cx: &mut task::Context<'_>,
        budget: &mut Budget,
    ) -> Poll<Option<MaybeWheelDispatch>> {
        let Some(mut entry) = ready!(self.inner.poll_recv(cx, budget)) else {
            return Poll::Ready(None);
        };

        let sender_idx = entry.sender_idx();
        let is_ack = matches!(&*entry, msg::Sender::ReceivedAck { .. });

        let cache = match self.resolve_cache(sender_idx) {
            Some(cache) => cache.clone(),
            None => return Poll::Ready(Some(None)),
        };

        let counters = cache.borrow().send_counters().clone();
        if is_ack {
            counters.on_received_ack();
        }

        let dispatch = match &mut *entry {
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
                    counters.on_received_ack_no_ctx();
                    return Poll::Ready(Some(None));
                };

                let mut lost_queue = PriorityInput::default();

                let wheel_interest = {
                    let mut ctx = ctx_rc.borrow_mut();
                    let interest = ctx.process_ack_payload(
                        payload,
                        *ack_delay,
                        &counters,
                        &mut self.completed_tx,
                        &mut lost_queue,
                        &mut self.cancelled_tx,
                        &self.clock,
                        &mut self.random,
                        &mut self.deferred,
                    );
                    counters.on_rtt(ctx.rtt_estimator.smoothed_rtt());
                    interest
                };

                if !lost_queue.is_empty() {
                    counters.on_lost(lost_queue.len() as u64);
                    let _ = self.frame_tx.send_batch(lost_queue);
                }

                Some((ctx_rc, wheel_interest))
            }
            msg::Sender::PendingAck(_) => {
                let ctx_rc = {
                    let mut cache = cache.borrow_mut();
                    match cache.get_or_insert(entry.path_secret_entry(), &self.clock) {
                        Ok(ctx) => ctx,
                        Err(error) => {
                            warn!(?error, peer = %entry.path_secret_entry().peer(), "dropping ack: send context not ready");
                            return Poll::Ready(Some(None));
                        }
                    }
                };

                let wheel_interest = {
                    let mut ctx = ctx_rc.borrow_mut();
                    ctx.pending_acks.push_back(entry);
                    ctx.wheel_interest(&self.clock)
                };

                Some((ctx_rc, wheel_interest))
            }
        };

        Poll::Ready(Some(dispatch))
    }

    fn on_consumed(&mut self, bytes: u64) {
        self.inner.on_consumed(bytes);
    }
}
