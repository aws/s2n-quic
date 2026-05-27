// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Client-side peer free list for tracking available server queue IDs.
//!
//! Tracks which server_queue_ids are available for allocation. Deduplicates
//! QueueFree messages using a monotonic free_request_id tracked in an IntervalSet.
//!
//! Allocation model:
//! - A high-water mark counter provides lock-free fresh ID allocation
//! - Once exhausted, consumers wait for recycled IDs pushed via QueueFree
//! - A HierarchicalBitSet provides O(4) pop_first for recycled IDs
//!
//! Dedup model:
//! - Each QueueFree message from the server carries a monotonic free_request_id
//! - The client tracks seen request IDs in an IntervalSet
//! - Duplicate/replayed QueueFree messages are rejected without per-slot state

use super::waiter::{Waiter, WaiterList};
use crate::bitset::HierarchicalBitSet;
use s2n_quic_core::{interval_set::IntervalSet, varint::VarInt};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll, Waker},
};

#[derive(Clone, Copy, Debug)]
pub struct FreeResult {
    pub slots: usize,
    pub ranges: usize,
}

impl FreeResult {
    const DUPLICATE: Self = Self {
        slots: 0,
        ranges: 0,
    };
}

#[derive(Debug)]
pub struct FreeList {
    high_water_mark: AtomicU64,
    max_queues: AtomicU64,
    inner: Mutex<Inner>,
}

struct Inner {
    freed: HierarchicalBitSet,
    seen_requests: IntervalSet<VarInt>,
    waiters: WaiterList,
    closed: bool,
}

impl std::fmt::Debug for Inner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inner")
            .field("freed_len", &self.freed.len())
            .field("seen_requests_count", &self.seen_requests.count())
            .field("waiters_len", &self.waiters.len())
            .field("closed", &self.closed)
            .finish()
    }
}

impl FreeList {
    pub fn new(initial_max_queues: VarInt) -> Self {
        let max = initial_max_queues.as_u64();
        let capacity = max.min(HierarchicalBitSet::MAX_CAPACITY as u64) as u32;
        Self {
            high_water_mark: AtomicU64::new(0),
            max_queues: AtomicU64::new(max),
            inner: Mutex::new(Inner {
                freed: HierarchicalBitSet::new(capacity.max(1)),
                seen_requests: IntervalSet::new(),
                waiters: WaiterList::new(),
                closed: false,
            }),
        }
    }

    #[inline]
    fn try_alloc_fresh(&self) -> Option<VarInt> {
        loop {
            let current = self.high_water_mark.load(Ordering::Relaxed);
            let max = self.max_queues.load(Ordering::Relaxed);
            if current >= max {
                return None;
            }
            match self.high_water_mark.compare_exchange_weak(
                current,
                current + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return VarInt::new(current).ok(),
                Err(_) => continue,
            }
        }
    }

    pub fn try_alloc(&self) -> Option<VarInt> {
        if let Some(id) = self.try_alloc_fresh() {
            return Some(id);
        }

        let mut inner = self.inner.lock().unwrap();
        let index = inner.freed.pop_first()?;
        VarInt::new(index as u64).ok()
    }

    /// Poll for a queue ID allocation.
    ///
    /// Returns `Ready(Some(id))` if an ID is available, `Ready(None)` if closed,
    /// or `Pending` if the caller must wait. The caller-owned `waiter` is registered
    /// into the internal wait list on `Pending`.
    pub fn poll_alloc(
        &self,
        waiter: &mut Option<Arc<Waiter>>,
        cx: &mut Context,
    ) -> Poll<Option<VarInt>> {
        if let Some(id) = self.try_alloc_fresh() {
            return Poll::Ready(Some(id));
        }

        let mut inner = self.inner.lock().unwrap();

        if inner.closed {
            return Poll::Ready(None);
        }

        if let Some(idx) = inner.freed.pop_first() {
            return Poll::Ready(VarInt::new(idx as u64).ok());
        }

        let w = waiter.get_or_insert_with(Waiter::new);

        // SAFETY: all links and waker access is under inner's Mutex
        unsafe {
            if w.links.is_linked() {
                w.set_waker(cx.waker().clone());
            } else {
                w.set_waker(cx.waker().clone());
                inner.waiters.push_back(Arc::clone(w));
            }
        }

        Poll::Pending
    }

    /// Remove a waiter that was previously registered via `poll_alloc`.
    pub fn cancel_waiter(&self, waiter: &mut Option<Arc<Waiter>>) {
        if let Some(w) = waiter.take() {
            let mut inner = self.inner.lock().unwrap();
            // SAFETY: under the inner Mutex
            unsafe {
                inner.waiters.remove(&w);
            }
        }
    }

    /// Returns a future that resolves to an allocated queue ID, or `None` if closed.
    ///
    /// The waiter node is allocated lazily on first `Pending` — the happy path
    /// (ID immediately available) incurs no allocation.
    pub fn alloc(self: &Arc<Self>) -> AllocFuture {
        AllocFuture {
            free_list: self.clone(),
            waiter: None,
        }
    }

    /// Process a QueueFree message from the server.
    ///
    /// Returns true if the message was accepted (new free_request_id), false if
    /// it was a duplicate/replay.
    ///
    /// Wakers are shipped to `waker_sink` rather than woken inline, because this
    /// is called from the dispatcher thread where syscalls are unacceptable.
    pub fn free(
        &self,
        free_request_id: VarInt,
        queue_ids: impl Iterator<
            Item = Result<core::ops::RangeInclusive<VarInt>, s2n_codec::DecoderError>,
        >,
        waker_sink: &mut impl FnMut(Waker),
    ) -> FreeResult {
        let mut inner = self.inner.lock().unwrap();

        let newly_inserted = inner
            .seen_requests
            .insert_value(free_request_id)
            .unwrap_or(false);
        if !newly_inserted {
            return FreeResult::DUPLICATE;
        }

        let mut slots = 0usize;
        let mut ranges = 0usize;
        for range in queue_ids {
            let Ok(range) = range else {
                break;
            };
            let start_u64 = range.start().as_u64();
            let end_u64 = range.end().as_u64();

            let cap = HierarchicalBitSet::MAX_CAPACITY as u64;
            if start_u64 >= cap {
                continue;
            }
            let start = start_u64 as u32;
            let end = end_u64.min(cap - 1) as u32;

            let needed = end + 1;
            if needed > inner.freed.capacity() {
                inner.freed.grow(needed);
            }
            inner.freed.insert_range(start, end);
            slots += (end - start + 1) as usize;
            ranges += 1;
        }

        // Wake at most as many waiters as IDs we just freed
        let mut woken = 0;
        while woken < slots {
            let Some(waiter_arc) = inner.waiters.pop_front() else {
                break;
            };
            // SAFETY: under the inner Mutex which protects all waiter waker access
            if let Some(w) = unsafe { waiter_arc.take_waker() } {
                waker_sink(w);
                woken += 1;
            }
        }
        FreeResult { slots, ranges }
    }

    /// Wake all blocked waiters without closing the free list.
    ///
    /// Used by peer-dead broadcast: waiters re-poll their alloc future and
    /// check the entry's cooldown state before deciding to bail or re-register.
    pub fn wake_all(&self, waker_sink: &mut impl FnMut(Waker)) {
        let mut inner = self.inner.lock().unwrap();
        while let Some(waiter_arc) = inner.waiters.pop_front() {
            // SAFETY: under the inner Mutex
            if let Some(w) = unsafe { waiter_arc.take_waker() } {
                waker_sink(w);
            }
        }
    }

    pub fn close(&self, waker_sink: &mut impl FnMut(Waker)) {
        let mut inner = self.inner.lock().unwrap();
        inner.closed = true;
        while let Some(waiter_arc) = inner.waiters.pop_front() {
            // SAFETY: under the inner Mutex
            if let Some(w) = unsafe { waiter_arc.take_waker() } {
                waker_sink(w);
            }
        }
    }
}

/// Future returned by [`FreeList::alloc`].
///
/// Lazily allocates an intrusive waiter node on first `Pending`.  On the happy
/// path (ID immediately available) no allocation occurs.
pub struct AllocFuture {
    free_list: Arc<FreeList>,
    waiter: Option<Arc<Waiter>>,
}

impl Future for AllocFuture {
    type Output = Option<VarInt>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        let this = self.get_mut();

        if let Some(id) = this.free_list.try_alloc_fresh() {
            return Poll::Ready(Some(id));
        }

        let mut inner = this.free_list.inner.lock().unwrap();

        if inner.closed {
            return Poll::Ready(None);
        }

        if let Some(idx) = inner.freed.pop_first() {
            return Poll::Ready(VarInt::new(idx as u64).ok());
        }

        let waiter = this.waiter.get_or_insert_with(Waiter::new);

        // SAFETY: all links and waker access is under inner's Mutex
        unsafe {
            if waiter.links.is_linked() {
                waiter.set_waker(cx.waker().clone());
            } else {
                waiter.set_waker(cx.waker().clone());
                inner.waiters.push_back(Arc::clone(waiter));
            }
        }

        Poll::Pending
    }
}

impl Drop for AllocFuture {
    fn drop(&mut self) {
        if let Some(waiter) = self.waiter.take() {
            // MUST always lock — Links uses Cell (!Sync), reading is_linked
            // without the lock while free() drains is UB.
            let mut inner = self.free_list.inner.lock().unwrap();
            // SAFETY: under the inner Mutex
            if let Some(owned) = unsafe { inner.waiters.remove(&waiter) } {
                drop(owned);
            }
            drop(inner);
        }
    }
}

impl FreeList {
    #[cfg(test)]
    fn free_for_test(&self, free_request_id: VarInt, queue_ids: &IntervalSet<VarInt>) -> bool {
        self.free(
            free_request_id,
            queue_ids.inclusive_ranges().map(Ok),
            &mut |w| w.wake(),
        )
        .slots
            > 0
    }
}

#[cfg(test)]
mod tests {
    use super::FreeList;
    use s2n_quic_core::{interval_set::IntervalSet, varint::VarInt};

    fn interval_set_single(id: VarInt) -> IntervalSet<VarInt> {
        let mut set = IntervalSet::new();
        let _ = set.insert_value(id);
        set
    }

    #[test]
    fn fresh_allocation_up_to_max() {
        let list = FreeList::new(VarInt::from_u8(3));
        assert_eq!(list.try_alloc(), Some(VarInt::from_u8(0)));
        assert_eq!(list.try_alloc(), Some(VarInt::from_u8(1)));
        assert_eq!(list.try_alloc(), Some(VarInt::from_u8(2)));
        assert_eq!(list.try_alloc(), None);
    }

    #[test]
    fn free_and_recycle() {
        let list = FreeList::new(VarInt::from_u8(2));
        let id0 = list.try_alloc().unwrap();
        let id1 = list.try_alloc().unwrap();
        assert_eq!(list.try_alloc(), None);

        let ids = interval_set_single(id1);
        assert!(list.free_for_test(VarInt::from_u8(1), &ids));
        assert_eq!(list.try_alloc(), Some(id1));
        assert_eq!(list.try_alloc(), None);

        let mut both = IntervalSet::new();
        let _ = both.insert_value(id0);
        let _ = both.insert_value(id1);
        assert!(list.free_for_test(VarInt::from_u8(2), &both));
        assert_eq!(list.try_alloc(), Some(id0));
        assert_eq!(list.try_alloc(), Some(id1));
    }

    #[test]
    fn duplicate_request_ids_rejected() {
        let list = FreeList::new(VarInt::from_u8(0));
        let ids = interval_set_single(VarInt::from_u8(7));

        assert!(list.free_for_test(VarInt::from_u8(1), &ids));
        assert_eq!(list.try_alloc(), Some(VarInt::from_u8(7)));

        assert!(!list.free_for_test(VarInt::from_u8(1), &ids));
        assert_eq!(list.try_alloc(), None);

        assert!(list.free_for_test(VarInt::from_u8(2), &ids));
        assert_eq!(list.try_alloc(), Some(VarInt::from_u8(7)));
    }
}
