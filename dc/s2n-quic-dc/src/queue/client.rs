// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Client-side queue allocation and dispatch.
//!
//! ## Architecture
//!
//! - [`ClientState`] — shared state (one Arc per peer). Owns the page table
//!   and local slot recycling. Stored on the path secret Entry.
//!
//! - [`ClientAllocator`] — stream creation path. Holds a cached `SenderView`
//!   to avoid RwLock on every allocation.
//!
//! - [`ClientDispatch`] — inbound packet dispatch path. Routes an incoming
//!   `msg::Stream` / `msg::Control` entry to the correct slot by `queue_id`
//!   after validating `binding_id`.

use super::{
    half::AutoWake,
    handle::{AllocResult, ControlReceiver, OnFree, StreamReceiver},
    page_table::{PageTable, SenderView},
    Error,
};
use crate::{bitset, endpoint::msg, intrusive, path::secret::map::Entry, sync};
use core::time::Duration;
use s2n_quic_core::varint::VarInt;
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Waker},
};

// ── ClientFreeList ───────────────────────────────────────────────────────────

/// Local slot recycling for the client page table.
///
/// Freed slot indices are stored in a `HierarchicalBitSet` for O(4) pop.
/// The bitset starts with capacity 1 and grows on demand so that fresh
/// allocators pay no up-front memory cost.
/// Fresh slots beyond the current high-water mark are allocated by bumping.
#[derive(Debug)]
pub struct ClientFreeList {
    freed: bitset::HierarchicalBitSet,
    high_water_mark: usize,
    closed: bool,
}

impl ClientFreeList {
    pub(crate) fn new() -> Self {
        Self {
            freed: bitset::HierarchicalBitSet::new(1),
            high_water_mark: 0,
            closed: false,
        }
    }

    /// Pop the next available local slot index.
    ///
    /// Returns `None` if the free list is closed.
    /// Prefers recycled indices (pop from bitset) over fresh ones (bump).
    pub(crate) fn pop(&mut self) -> Option<usize> {
        if self.closed {
            return None;
        }
        if let Some(idx) = self.freed.pop_first() {
            return Some(idx as usize);
        }
        let idx = self.high_water_mark;
        self.high_water_mark += 1;
        Some(idx)
    }

    /// Return a freed slot index back to the recycling set.
    pub(crate) fn push_freed(&mut self, index: usize) {
        let idx = index as u32;
        if idx >= self.freed.capacity() {
            if idx >= bitset::HierarchicalBitSet::MAX_CAPACITY {
                return;
            }
            self.freed.grow(idx + 1);
        }
        let newly_inserted = self.freed.insert(idx);
        debug_assert!(newly_inserted, "double-free of slot {index}");
    }

    // TODO: path secret entry should call close on the queues when removed from the map
    #[cfg_attr(not(test), expect(dead_code))]
    pub(crate) fn close(&mut self) {
        self.closed = true;
    }
}

// ── ClientState (shared, one Arc per peer) ──────────────────────────────────

/// Shared client-side queue state for a single peer connection.
#[derive(Debug)]
pub struct ClientState {
    pub(crate) pages: PageTable,
    /// Monotonically increasing binding_id generator.  Starts at 1 so that
    /// fresh slots (whose stored binding starts at 0) always accept the first bind.
    next_binding: AtomicU64,
    /// Local slot index recycling.
    pub(crate) free: Mutex<ClientFreeList>,
    /// Peer's available queue slots (populated by QueueFree frames).
    pub(crate) peer_free: sync::free_list::FreeList,
}

impl ClientState {
    pub fn new(max_peer_queues: VarInt) -> Self {
        Self {
            pages: PageTable::new(),
            next_binding: AtomicU64::new(1),
            free: Mutex::new(ClientFreeList::new()),
            peer_free: sync::free_list::FreeList::new(max_peer_queues),
        }
    }

    fn next_binding_id(&self) -> VarInt {
        let id = self.next_binding.fetch_add(1, Ordering::Relaxed);
        VarInt::new(id).expect("binding_id overflow")
    }

    /// Allocate a queue slot, waiting for a peer-side free ID if needed.
    ///
    /// Returns `None` if the peer is dead (free list closed or entry in cooldown).
    /// Checks `entry.is_dead_during_cooldown` on each poll so that a peer-dead
    /// broadcast's `wake_all` causes blocked callers to bail promptly.
    pub fn alloc<'a>(
        self: &'a Arc<Self>,
        entry: &'a Entry,
        cooldown: Duration,
    ) -> ClientAllocFuture<'a> {
        ClientAllocFuture {
            state: self,
            entry,
            cooldown,
            waiter: None,
        }
    }

    /// Push Reset into all allocated slots and wake blocked alloc waiters.
    ///
    /// Does NOT permanently close the slots — this is a transient peer-dead
    /// notification.  After cooldown expires, new alloc() calls proceed normally.
    pub fn broadcast_reset(&self, error_code: VarInt, waker_sink: &mut impl FnMut(AutoWake)) {
        let mut view = SenderView::new();
        view.for_each_slot(&self.pages, |slot| {
            let (sw, cw) = slot.broadcast_reset(error_code);
            waker_sink(sw);
            waker_sink(cw);
        });
        self.peer_free
            .wake_all(&mut |w| waker_sink(AutoWake::new(Some(w))));
    }

    pub(crate) fn alloc_local(self: &Arc<Self>, dest_queue_id: VarInt) -> Option<AllocResult> {
        let index = {
            let mut free = self.free.lock().unwrap();
            free.pop()?
        };

        self.pages.grow_to_fit(index);

        let mut view = SenderView::new();
        view.grow_to_fit(index, &self.pages);
        let slot_ref = view
            .get(index, &self.pages)
            .expect("slot index out of range after grow");

        let binding_id = self.next_binding_id();
        if slot_ref.allocate_and_open(binding_id).is_err() {
            let mut free = self.free.lock().unwrap();
            free.push_freed(index);
            return None;
        }

        let slot_ptr = slot_ref.as_ptr();
        let local_queue_id = VarInt::new(index as u64).expect("slot index exceeds VarInt range");
        let on_free = OnFree::Client {
            state: self.clone(),
        };

        Some(AllocResult {
            stream: StreamReceiver::new(slot_ptr, on_free.clone()),
            control: ControlReceiver::new(slot_ptr, on_free),
            local_queue_id,
            dest_queue_id,
            binding_id,
        })
    }
}

// ── ClientAllocFuture ───────────────────────────────────────────────────────

pub struct ClientAllocFuture<'a> {
    state: &'a Arc<ClientState>,
    entry: &'a Entry,
    cooldown: Duration,
    waiter: Option<Arc<sync::waiter::Waiter>>,
}

impl Future for ClientAllocFuture<'_> {
    type Output = Option<AllocResult>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();

        // Reads the clock — bach-aware in test contexts.
        let now = crate::time::DefaultClock::default().now();
        if this.entry.is_dead_during_cooldown(now, this.cooldown) {
            return Poll::Ready(None);
        }

        match this.state.peer_free.poll_alloc(&mut this.waiter, cx) {
            Poll::Ready(Some(dest_queue_id)) => Poll::Ready(this.state.alloc_local(dest_queue_id)),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for ClientAllocFuture<'_> {
    fn drop(&mut self) {
        self.state.peer_free.cancel_waiter(&mut self.waiter);
    }
}

// ── ClientDispatch ──────────────────────────────────────────────────────────

/// Routes inbound packets to allocated client slots.
///
/// `queue_id` is the local slot index (what the peer sent as `dest_queue_id`).
/// `binding_id` is validated against the slot's stored value before pushing.
pub struct ClientDispatch {
    state: Arc<ClientState>,
    view: SenderView,
}

impl ClientDispatch {
    pub fn new(state: Arc<ClientState>) -> Self {
        Self {
            state,
            view: SenderView::new(),
        }
    }

    #[inline]
    pub fn send_stream(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Stream>,
    ) -> Result<AutoWake, Error<intrusive::Entry<msg::Stream>>> {
        let index = queue_id.as_u64() as usize;
        let Some(slot) = self.view.get(index, &self.state.pages) else {
            return Err(Error::Unallocated(entry));
        };
        slot.push_stream(binding_id, entry)
    }

    #[inline]
    pub fn send_control(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Control>,
    ) -> Result<AutoWake, Error<intrusive::Entry<msg::Control>>> {
        let index = queue_id.as_u64() as usize;
        let Some(slot) = self.view.get(index, &self.state.pages) else {
            return Err(Error::Unallocated(entry));
        };
        slot.push_control(binding_id, entry)
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    pub fn send_msg<E>(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        msg_id: u64,
        stream_offset: u64,
        message_size: u32,
        chunk_size: u16,
        chunk_index: u32,
        payload_len: u32,
        is_fin: bool,
        is_wakeup: bool,
        write_fn: impl FnOnce(*mut u8, u32) -> Result<(), E>,
    ) -> Result<AutoWake, super::MsgError<E>> {
        let index = queue_id.as_u64() as usize;
        let Some(slot) = self.view.get(index, &self.state.pages) else {
            return Err(super::MsgError::Queue(Error::Unallocated(())));
        };
        slot.push_msg(
            binding_id,
            msg_id,
            stream_offset,
            message_size,
            chunk_size,
            chunk_index,
            payload_len,
            is_fin,
            is_wakeup,
            write_fn,
        )
    }

    /// Process a received QueueFree frame, returning freed queue_ids to the peer_free list.
    pub fn free(
        &self,
        free_request_id: VarInt,
        queue_ids: impl Iterator<
            Item = Result<core::ops::RangeInclusive<VarInt>, s2n_codec::DecoderError>,
        >,
        waker_sink: &mut impl FnMut(Waker),
    ) -> crate::sync::free_list::FreeResult {
        self.state
            .peer_free
            .free(free_request_id, queue_ids, waker_sink)
    }

    /// Broadcast-close all currently allocated slots.
    ///
    /// Called when the path secret entry is evicted.  `AutoWake` tokens are
    /// passed to `waker_sink` — the caller can `.take()` to batch wakers for
    /// later, or simply drop the token to wake immediately.
    pub fn close(&mut self, waker_sink: &mut impl FnMut(AutoWake)) {
        self.view.for_each_slot(&self.state.pages, |slot| {
            let (sw, cw) = slot.broadcast_close();
            waker_sink(sw);
            waker_sink(cw);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::testing::*;
    use s2n_quic_core::varint::VarInt;

    fn v(n: u64) -> VarInt {
        VarInt::new(n).unwrap()
    }

    fn test_state(max_queues: u64) -> Arc<ClientState> {
        Arc::new(ClientState::new(VarInt::new(max_queues).unwrap()))
    }

    fn try_alloc(state: &Arc<ClientState>) -> Option<AllocResult> {
        let dest_queue_id = state.peer_free.try_alloc()?;
        state.alloc_local(dest_queue_id)
    }

    // ── ClientFreeList ──────────────────────────────────────────────────────

    #[test]
    fn pop_bumps_high_water_mark() {
        let mut fl = ClientFreeList::new();
        assert_eq!(fl.pop(), Some(0));
        assert_eq!(fl.pop(), Some(1));
        assert_eq!(fl.pop(), Some(2));
    }

    #[test]
    fn push_freed_recycles() {
        let mut fl = ClientFreeList::new();
        let _ = fl.pop(); // 0
        let _ = fl.pop(); // 1
        fl.push_freed(0);
        assert_eq!(fl.pop(), Some(0));
    }

    #[test]
    fn close_stops_allocation() {
        let mut fl = ClientFreeList::new();
        fl.close();
        assert_eq!(fl.pop(), None);
    }

    // ── ClientState alloc ──────────────────────────────────────────────────

    #[test]
    fn alloc_returns_sequential_ids() {
        let state = test_state(100);
        let r1 = try_alloc(&state).unwrap();
        let r2 = try_alloc(&state).unwrap();
        assert_eq!(r1.local_queue_id, v(0));
        assert_eq!(r2.local_queue_id, v(1));
        assert_ne!(r1.binding_id, r2.binding_id);
    }

    #[test]
    fn alloc_respects_peer_free_cap() {
        let state = test_state(2);
        assert!(try_alloc(&state).is_some());
        assert!(try_alloc(&state).is_some());
        assert!(try_alloc(&state).is_none());
    }

    #[test]
    fn close_prevents_further_alloc() {
        let state = test_state(100);
        state.free.lock().unwrap().close();
        assert!(try_alloc(&state).is_none());
    }

    // ── ClientDispatch ──────────────────────────────────────────────────────

    #[test]
    fn dispatch_to_allocated_slot() {
        let state = test_state(10);
        let result = try_alloc(&state).unwrap();
        let mut dispatch = ClientDispatch::new(state);

        let wake = dispatch.send_stream(
            result.local_queue_id,
            result.binding_id,
            make_stream_entry(),
        );
        assert!(wake.is_ok());

        drop(result);
    }

    #[test]
    fn dispatch_to_unallocated_slot() {
        let state = test_state(10);
        let mut dispatch = ClientDispatch::new(state);
        let result = dispatch.send_stream(v(99), v(1), make_stream_entry());
        assert!(matches!(result, Err(Error::Unallocated(_))));
    }

    #[test]
    fn dispatch_stale_binding() {
        let state = test_state(10);
        let result = try_alloc(&state).unwrap();
        let mut dispatch = ClientDispatch::new(state);

        let wrong_binding = VarInt::new(result.binding_id.as_u64() + 99).unwrap();
        let err = dispatch.send_stream(result.local_queue_id, wrong_binding, make_stream_entry());
        assert!(matches!(err, Err(Error::FutureBinding(_))));

        drop(result);
    }
}
