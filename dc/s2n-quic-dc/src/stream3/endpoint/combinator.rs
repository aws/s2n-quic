// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Channel combinators specific to the stream3 endpoint pipeline.
//!
//! These are `Receiver` adapters that compose with the generic combinators in
//! `socket::channel` but carry stream3-specific knowledge (path-secret routing,
//! frame batching by peer, etc).

use crate::{
    intrusive_queue::{Entry, Queue},
    path::secret::map::Entry as PathSecretEntry,
    socket::channel::{ByteCost, Receiver, Sender, UnboundedSender},
    stream3::frame::Frame,
};
use core::task::{self, Poll};
use s2n_quic_core::{ready, varint::VarInt};
use std::{marker::PhantomData, sync::Arc};

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

/// A queue of frames grouped for a single path-secret entry.
///
/// This wrapper keeps the queue byte-cost estimate and path-secret entry so it can be
/// routed through the priority merger and `PickTwo`.
///
/// Because individual frames are routed into per-priority unsync lanes *before*
/// [`BatchFramesByPathSecret`] coalesces them, all frames in a `FrameBatch` that comes out
/// of a given lane share the same priority class.
pub struct FrameBatch {
    queue: Queue<Frame>,
    byte_cost: u64,
    /// Sticky sender assignment for this batch. `VarInt::MAX` means no preference (use pick-two).
    /// Set by `BatchFramesByPathSecret` when any frame in the batch requires sticky routing.
    sender_id: VarInt,
}

impl FrameBatch {
    #[inline]
    fn new(first: Entry<Frame>) -> Self {
        let byte_cost = MAX_FRAME_BATCH_PACKET_OVERHEAD.saturating_add(first.byte_cost());
        let sender_id = first.source_sender_id;
        let mut queue = Queue::new();
        queue.push_back(first);

        Self {
            queue,
            byte_cost,
            sender_id,
        }
    }

    #[inline]
    fn push_with_cost(&mut self, frame: Entry<Frame>, frame_cost: u64) {
        self.byte_cost = self.byte_cost.saturating_add(frame_cost);
        self.queue.push_back(frame);
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

    /// Returns the number of frames currently buffered in this batch.
    #[inline]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns true when this batch contains no frames.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Borrows the underlying intrusive queue of frames.
    #[inline]
    pub fn queue(&self) -> &Queue<Frame> {
        &self.queue
    }

    /// Consumes the batch and returns the underlying frame queue.
    #[inline]
    pub fn into_queue(self) -> Queue<Frame> {
        self.queue
    }
}

impl From<FrameBatch> for Queue<Frame> {
    #[inline]
    fn from(value: FrameBatch) -> Self {
        value.into_queue()
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
        let Some(front) = &self.queue.front() else {
            unsafe {
                s2n_quic_core::assume!(false, "FrameBatch should always be non-empty");
            }
        };
        front.path_secret_entry()
    }
}

// ── BatchFramesByPathSecret ───────────────────────────────────────────────

const BATCH_FRAMES_POLL_BUDGET: usize = 10;

/// Receiver combinator that batches consecutive frame entries by path-secret entry and byte budget.
///
/// Batches target roughly one datagram (`path_secret_entry.max_datagram_size()`) while accounting
/// for frame metadata and conservative packet overhead. A batch always contains at least one frame.
pub struct BatchFramesByPathSecret<R> {
    inner: R,
    buffered: Option<Entry<Frame>>,
}

impl<R> BatchFramesByPathSecret<R>
where
    R: Receiver<Entry<Frame>>,
{
    #[inline]
    pub fn new(inner: R) -> Self {
        Self {
            inner,
            buffered: None,
        }
    }

    #[inline]
    fn take_first(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<Entry<Frame>>> {
        if let Some(frame) = self.buffered.take() {
            return Poll::Ready(Some(frame));
        }

        self.inner.poll_recv(cx)
    }
}

impl<R> Receiver<FrameBatch> for BatchFramesByPathSecret<R>
where
    R: Receiver<Entry<Frame>>,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<FrameBatch>> {
        let Some(first) = (match self.take_first(cx) {
            Poll::Ready(frame) => frame,
            Poll::Pending => return Poll::Pending,
        }) else {
            return Poll::Ready(None);
        };

        let target_bytes = first.path_secret_entry.max_datagram_size() as u64;
        let mut batch = FrameBatch::new(first);

        for _ in 0..BATCH_FRAMES_POLL_BUDGET {
            if batch.byte_cost() >= target_bytes {
                break;
            }

            match self.inner.poll_recv(cx) {
                Poll::Ready(Some(frame_entry)) => {
                    if !Arc::ptr_eq(batch.path_secret_entry(), frame_entry.path_secret_entry()) {
                        self.buffered = Some(frame_entry);
                        break;
                    }

                    // Break on conflicting sticky assignments: if the batch already has a
                    // sticky sender and this frame wants a different one (or vice versa),
                    // yield the current batch and buffer this frame for the next poll.
                    let frame_sticky = frame_entry.source_sender_id;
                    if batch.sender_id != frame_sticky
                        && (batch.sender_id != VarInt::MAX && frame_sticky != VarInt::MAX)
                    {
                        self.buffered = Some(frame_entry);
                        break;
                    }

                    // Adopt the frame's sticky preference if the batch doesn't have one yet.
                    if batch.sender_id == VarInt::MAX {
                        batch.sender_id = frame_sticky;
                    }

                    let frame_cost = frame_entry.byte_cost();
                    let next_cost = batch.byte_cost().saturating_add(frame_cost);
                    if next_cost > target_bytes {
                        self.buffered = Some(frame_entry);
                        break;
                    }

                    batch.push_with_cost(frame_entry, frame_cost);
                }
                Poll::Ready(None) | Poll::Pending => break,
            }
        }

        Poll::Ready(Some(batch))
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
pub struct PickTwo<T, R, S, Rand> {
    rx: R,
    senders: Vec<S>,
    random: Rand,
    value: PhantomData<fn() -> T>,
}

impl<T, R, S, Rand> PickTwo<T, R, S, Rand>
where
    T: ByteCost + PathSecretMapEntry + StickyRoute,
    R: Receiver<T>,
    S: UnboundedSender<T>,
    Rand: FnMut(usize) -> usize,
{
    pub fn new(rx: R, senders: Vec<S>, random: Rand) -> Self {
        Self {
            rx,
            senders,
            random,
            value: PhantomData,
        }
    }

    fn try_send_pick_two(mut value: T, senders: &mut Vec<S>, random: &mut Rand) -> Result<(), T> {
        debug_assert!(!senders.is_empty());
        let chosen_idx = if let Some(sticky_idx) = value.sticky_sender_idx() {
            // Sticky routing — retransmissions must go back through the same socket.
            sticky_idx
        } else {
            let picked = value
                .path_secret_entry()
                .pick_sender_by_next_transmission(random);
            picked
        };

        debug_assert!(
            chosen_idx < senders.len(),
            "sticky sender index out of bounds: sticky={} senders={}",
            chosen_idx,
            senders.len()
        );

        value.set_sender_id(chosen_idx);

        senders[chosen_idx].send(value)
    }
}

impl<T, R, S, Rand> Receiver<()> for PickTwo<T, R, S, Rand>
where
    T: ByteCost + PathSecretMapEntry + StickyRoute,
    R: Receiver<T>,
    S: UnboundedSender<T>,
    Rand: FnMut(usize) -> usize,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<()>> {
        let Some(value) = ready!(self.rx.poll_recv(cx)) else {
            return Poll::Ready(None);
        };

        match Self::try_send_pick_two(value, &mut self.senders, &mut self.random) {
            Ok(()) => {
                // Sent successfully. Compute byte cost before clearing slot.
                Poll::Ready(Some(()))
            }
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
struct Assembler<R, Clk> {
    inner: R,
    clock: Clk,
    source_sender_id: s2n_quic_core::varint::VarInt,
    source_control_port: u16,
    gso: s2n_quic_platform::features::Gso,
    pool: crate::socket::pool::Pool,
    header_buf: Vec<u8>,
    cancelled_tx: CancelledFrameSink,
}

impl<R, Clk> Assembler<R, Clk> {
    fn new(
        inner: R,
        clock: Clk,
        source_sender_id: s2n_quic_core::varint::VarInt,
        source_control_port: u16,
        gso: s2n_quic_platform::features::Gso,
        pool: crate::socket::pool::Pool,
    ) -> Self {
        Self {
            inner,
            clock,
            source_sender_id,
            source_control_port,
            gso,
            pool,
            header_buf: Vec::new(),
            cancelled_tx: CancelledFrameSink,
        }
    }
}

impl<R, Clk> Receiver<descriptor::Segments> for Assembler<R, Clk>
where
    R: Receiver<Rc<RefCell<send::Context>>>,
    Clk: precision::Clock,
{
    fn poll_recv(&mut self, cx: &mut task::Context<'_>) -> Poll<Option<descriptor::Segments>> {
        use crate::stream3::endpoint::assemble;

        // Get the next context that's ready for transmission
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
            // ask the context if it want's to be inserted into any wheels
            let wheel_interest = todo!();
            (segments, wheel_interest)
        };

        // TODO reinsert the context if it has interest - those channels should be stored on the Assembler

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

#[cfg(test)]
mod tests;
