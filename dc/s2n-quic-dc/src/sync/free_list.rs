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
            self.remove_waiter_and_heal(&w);
        }
    }

    /// Unlink `waiter` from the wait list and re-wake other parked waiters if any
    /// IDs are sitting available in `freed`.
    ///
    /// This future may have been woken by `free()` — which pops a waiter off the
    /// list and "spends" a wake credit for an ID it deposited in `freed` — and then
    /// dropped before it ever re-polled to collect that ID (a stream-create
    /// cancel/timeout). The wake credit is gone but the ID is still sitting in
    /// `freed`, so without this another parked waiter would never be told and would
    /// hang forever despite an available ID.
    ///
    /// Restore the invariant "an available ID never coexists with a parked waiter"
    /// by waking up to `freed.len()` waiters. Over-waking is benign (a spurious wake
    /// just re-polls and re-parks); under-waking is the bug. Waking inline is safe
    /// here: a future's drop runs on the owning task, not the dispatcher thread that
    /// `free()` must offload from.
    fn remove_waiter_and_heal(&self, waiter: &Arc<Waiter>) {
        let mut inner = self.inner.lock().unwrap();
        // SAFETY: under the inner Mutex
        unsafe {
            inner.waiters.remove(waiter);
        }
        let available = inner.freed.len() as usize;
        if available > 0 {
            Self::wake_up_to(&mut inner, available, &mut |w| w.wake());
        }
    }

    /// Return a previously-allocated peer queue ID to the free list.
    ///
    /// Used when a caller pops an ID from `poll_alloc`/`try_alloc` but then fails
    /// to complete the allocation (e.g. the local slot free list is exhausted), so
    /// the peer ID would otherwise be dropped on the floor — leaking it and the
    /// wake it was handed. Re-inserting heals any parked waiter waiting on an ID.
    pub fn release(&self, id: VarInt) {
        let idx = id.as_u64();
        if idx >= HierarchicalBitSet::MAX_CAPACITY as u64 {
            return;
        }
        let idx = idx as u32;
        let mut inner = self.inner.lock().unwrap();
        let needed = idx + 1;
        if needed > inner.freed.capacity() {
            inner.freed.grow(needed);
        }
        inner.freed.insert(idx);
        let available = inner.freed.len() as usize;
        Self::wake_up_to(&mut inner, available, &mut |w| w.wake());
    }

    /// Ship the wakers of up to `budget` parked waiters to `waker_sink`, popping
    /// each off the wait list. Each woken waiter is expected to re-poll and
    /// collect one available ID from `freed`. Must be called under the inner lock.
    fn wake_up_to(inner: &mut Inner, budget: usize, waker_sink: &mut impl FnMut(Waker)) {
        let mut woken = 0;
        while woken < budget {
            let Some(waiter_arc) = inner.waiters.pop_front() else {
                break;
            };
            // SAFETY: under the inner Mutex which protects all waiter waker access
            if let Some(w) = unsafe { waiter_arc.take_waker() } {
                waker_sink(w);
                woken += 1;
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

        // Latch the request_id up front. This MUST stay up front, not deferred to
        // after the ranges are consumed: a QueueFree is retransmitted with the
        // *byte-identical* payload and the same request_id (queue/freed.rs
        // RetryEntry stores the original encoded bytes), and the network may also
        // duplicate it. Reprocessing the same payload twice would re-insert IDs
        // the client has since allocated, double-allocating live queue slots. An
        // honest peer's payload never decode-errors anyway (the encoder emits
        // well-formed deltas and queue IDs are bounded far below VarInt::MAX, and
        // on-wire tampering fails AEAD before reaching here), so a mid-stream
        // `break` only drops the trailing ranges of a *malicious* peer's own
        // corrupt message — there is no honest leak to recover.
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

        // Wake based on the TOTAL number of IDs available in `freed`, not just the
        // `slots` we added this call. Any IDs stranded by a prior woken-then-
        // cancelled waiter (see `cancel_waiter`) are still sitting here; waking
        // only `slots` would leave them — and their would-be consumers — stuck.
        // Over-waking is harmless (a waiter that finds no ID re-parks); the cap is
        // just to avoid waking more waiters than there are IDs to hand out.
        let available = inner.freed.len() as usize;
        Self::wake_up_to(&mut inner, available, waker_sink);
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
            // Unlink and, if this future was woken-but-never-collected, hand its
            // stranded ID's wake to another parked waiter. See
            // `FreeList::remove_waiter_and_heal`. (MUST always lock — `Links` uses
            // Cell (!Sync); reading `is_linked` without the lock while `free()`
            // drains is UB.)
            self.free_list.remove_waiter_and_heal(&waiter);
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
    use super::{AllocFuture, FreeList};
    use s2n_quic_core::{interval_set::IntervalSet, varint::VarInt};
    use std::{
        future::Future,
        pin::Pin,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        task::{Context, Poll, Wake, Waker},
    };

    fn interval_set_single(id: VarInt) -> IntervalSet<VarInt> {
        let mut set = IntervalSet::new();
        let _ = set.insert_value(id);
        set
    }

    /// A waker that counts how many times it has been woken.
    struct CountingWaker(AtomicUsize);
    impl Wake for CountingWaker {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn counting_waker() -> (Waker, Arc<CountingWaker>) {
        let inner = Arc::new(CountingWaker(AtomicUsize::new(0)));
        (Waker::from(inner.clone()), inner)
    }

    fn poll(fut: &mut Pin<Box<AllocFuture>>, waker: &Waker) -> Poll<Option<VarInt>> {
        let mut cx = Context::from_waker(waker);
        fut.as_mut().poll(&mut cx)
    }

    /// Regression guard for the FreeList stranded-ID lost-wakeup.
    ///
    /// Setup: max_queues = 0, so every allocation must come from the recycled
    /// `freed` set. Three futures park as waiters [W0, W1, W2]. We then `free`
    /// two IDs. `free()` wakes two waiters (W0, W1) by popping them off the front
    /// and sending their wakers. Crucially the IDs are NOT yet handed out — they
    /// sit in `freed` until a woken future re-polls and pops one.
    ///
    /// Now W0 is DROPPED before it re-polls (stream-create cancel/timeout). It was
    /// woken for an ID it never collected, so one ID is left stranded in `freed`.
    /// W1 re-polls and consumes one of the two IDs, leaving exactly one available.
    ///
    /// The bug was that W0's drop neither consumed its ID nor handed its
    /// wake-credit on, so W2 — still parked — was never told and hung forever
    /// despite the free ID. The fix (`remove_waiter_and_heal`) re-wakes a parked
    /// waiter whenever a future drops while IDs remain available, so W2 is woken.
    #[test]
    fn stranded_id_lost_wakeup() {
        let list = Arc::new(FreeList::new(VarInt::from_u8(0)));

        // Three independent waiters.
        let (w0, c0) = counting_waker();
        let (w1, c1) = counting_waker();
        let (w2, c2) = counting_waker();

        let mut f0 = Box::pin(list.alloc());
        let mut f1 = Box::pin(list.alloc());
        let mut f2 = Box::pin(list.alloc());

        // All three park: waiter list = [W0, W1, W2].
        assert!(poll(&mut f0, &w0).is_pending());
        assert!(poll(&mut f1, &w1).is_pending());
        assert!(poll(&mut f2, &w2).is_pending());

        // Free two IDs (free_request_id = 1). free() wakes exactly two waiters
        // (W0, W1) and leaves W2 parked. Two IDs now sit in `freed`, unconsumed.
        let mut ids = IntervalSet::new();
        let _ = ids.insert_value(VarInt::from_u8(0));
        let _ = ids.insert_value(VarInt::from_u8(1));
        let res = list.free(VarInt::from_u8(1), ids.inclusive_ranges().map(Ok), &mut |w| {
            w.wake()
        });
        assert_eq!(res.slots, 2, "two slots freed");

        // free woke W0 and W1 (the budget equals the two IDs freed), not W2 yet.
        assert_eq!(c0.0.load(Ordering::SeqCst), 1, "W0 woken");
        assert_eq!(c1.0.load(Ordering::SeqCst), 1, "W1 woken");
        assert_eq!(c2.0.load(Ordering::SeqCst), 0, "W2 not yet woken");

        // W0 is cancelled before it re-polls (e.g. stream-create timeout). It was
        // woken for an ID it never collected — that ID is still in `freed`. The
        // fix's heal-on-drop must hand that stranded ID's wake to a parked waiter
        // (W2), since W1 is the only other woken-but-not-dropped future.
        drop(f0);

        // W1 re-polls and consumes ONE of the two free IDs.
        assert!(matches!(poll(&mut f1, &w1), Poll::Ready(Some(_))));

        // After W1 consumes one ID, EXACTLY ONE ID is still sitting in `freed` —
        // the one W0 was woken for but, having been cancelled, never collected.
        {
            let inner = list.inner.lock().unwrap();
            assert_eq!(
                inner.freed.len(),
                1,
                "one ID stranded in `freed` after W0's cancellation"
            );
        }

        // The regression assertion: a still-parked waiter must be woken while an
        // ID is available. Before the fix W2's waker never fired (W0's drop only
        // unlinked itself), so the task hung forever despite the free ID. With
        // `remove_waiter_and_heal`, W0's drop re-wakes W2; if W2 now re-polls it
        // collects the stranded ID.
        assert!(
            c2.0.load(Ordering::SeqCst) >= 1,
            "W2 must be woken: a free ID was stranded by W0's cancellation and \
             heal-on-drop must hand the wake to a still-parked waiter"
        );
        // And W2 actually allocates the stranded ID when it re-polls.
        assert!(matches!(poll(&mut f2, &w2), Poll::Ready(Some(_))));
    }

    /// `release` returns a popped-but-unused peer ID and wakes a parked waiter.
    ///
    /// Mirrors the `ClientAllocFuture` path where `poll_alloc` hands out an ID but
    /// pairing it with a local slot fails, so the caller must hand the peer ID back
    /// rather than drop it. The returned ID must become allocatable again and any
    /// parked waiter must be woken to collect it.
    #[test]
    fn release_returns_id_and_wakes_waiter() {
        let list = Arc::new(FreeList::new(VarInt::from_u8(0)));

        // A waiter parks: nothing is available yet.
        let (w0, c0) = counting_waker();
        let mut f0 = Box::pin(list.alloc());
        assert!(poll(&mut f0, &w0).is_pending());
        assert_eq!(c0.0.load(Ordering::SeqCst), 0);

        // A caller "allocated" peer ID 7 elsewhere but couldn't use it; it hands
        // the ID back. The parked waiter must be woken and the ID allocatable.
        list.release(VarInt::from_u8(7));
        assert_eq!(c0.0.load(Ordering::SeqCst), 1, "parked waiter woken on release");
        assert!(matches!(poll(&mut f0, &w0), Poll::Ready(Some(id)) if id == VarInt::from_u8(7)));
    }

    /// End-to-end regression guard under the bach discrete-event runtime.
    ///
    /// Same scenario as `stranded_id_lost_wakeup`, but driven through real async
    /// tasks so a regression manifests as an indefinite hang ("Runtime stalled")
    /// rather than an assertion. Three tasks await `alloc()` and park. A
    /// controller frees two IDs (waking the first two parked waiters), then
    /// immediately aborts the first woken task before it can re-poll — modelling
    /// a stream-create cancellation/timeout. The second woken task consumes one
    /// ID; one ID remains free. Without the heal-on-drop fix the third task would
    /// never be woken and awaiting it would stall the runtime; with the fix the
    /// aborted task's drop re-wakes it and it completes.
    #[test]
    fn stranded_id_lost_wakeup_bach() {
        use crate::testing::{ext::*, sim};

        let _g = crate::testing::without_snapshots();

        sim(|| {
            let list = Arc::new(FreeList::new(VarInt::from_u8(0)));

            // Spawn three waiter tasks; each parks on alloc().
            let l0 = list.clone();
            let h0 = async move {
                let _ = l0.alloc().await;
            }
            .spawn();

            let l1 = list.clone();
            let h1 = async move {
                let _ = l1.alloc().await;
            }
            .spawn();

            let l2 = list.clone();
            let done2 = Arc::new(AtomicUsize::new(0));
            let done2_task = done2.clone();
            let h2 = async move {
                let _ = l2.alloc().await;
                done2_task.fetch_add(1, Ordering::SeqCst);
            }
            .spawn();

            let _ = (&h1,);

            // Controller: free two IDs and cancel the first woken task before it
            // re-polls. Then wait for the third task to complete — which it never
            // will, because no wakeup ever reaches it.
            async move {
                // Let all three waiters park first.
                bach::time::sleep(core::time::Duration::from_millis(1)).await;

                let mut ids = IntervalSet::new();
                let _ = ids.insert_value(VarInt::from_u8(0));
                let _ = ids.insert_value(VarInt::from_u8(1));
                let res =
                    list.free(VarInt::from_u8(1), ids.inclusive_ranges().map(Ok), &mut |w| {
                        w.wake()
                    });
                assert_eq!(res.slots, 2);

                // Cancel the first woken waiter (h0) before it can re-poll and
                // collect its ID. Its Drop removes it from the wait list but does
                // not consume the ID nor re-wake any other parked waiter.
                h0.abort();

                // The third waiter (h2) was never woken, but an ID is free. Wait
                // for it: on correct behavior it completes; on current code it
                // hangs -> bach reports "Runtime stalled".
                let _ = h2.await;
                assert_eq!(done2.load(Ordering::SeqCst), 1, "W2 allocated");
            }
            .primary()
            .spawn();
        });
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

    /// A QueueFree whose payload decode-errors mid-stream latches its
    /// `free_request_id` and drops the trailing ranges — and that is the CORRECT
    /// behavior, not a leak.
    ///
    /// The earlier suspicion was that the retransmission could "complete" the
    /// dropped ranges, so latching up front would strand them. But the retry path
    /// retransmits the *byte-identical* payload (`queue/freed.rs` `RetryEntry`
    /// stores the original encoded bytes), so the retransmission decode-errors at
    /// the exact same point — there is nothing to complete. And an honest peer's
    /// payload never decode-errors at all: the server encoder emits well-formed
    /// deltas, queue IDs are bounded far below `VarInt::MAX`, and on-wire tampering
    /// fails AEAD before reaching `free()`. So a decode error only ever truncates a
    /// *malicious* peer's own message.
    ///
    /// This test pins the up-front latch: a duplicate (same request_id, same
    /// payload prefix) must be rejected without re-inserting the already-consumed
    /// IDs — re-inserting would double-allocate slots the client has since handed
    /// out, which is far worse than dropping a malicious peer's trailing garbage.
    #[test]
    fn partial_decode_dedups_identical_retransmission() {
        use s2n_codec::DecoderError;

        let list = FreeList::new(VarInt::from_u8(0));

        let r = |a: u8, b: u8| Ok(VarInt::from_u8(a)..=VarInt::from_u8(b));

        // First receipt of QueueFree request_id=1: range [10..=10] then a decode
        // error. `free()` inserts slot 10 and breaks; the request_id is latched.
        let first: Vec<Result<core::ops::RangeInclusive<VarInt>, DecoderError>> = vec![
            r(10, 10),
            Err(DecoderError::InvariantViolation("delta overflow")),
        ];
        let res = list.free(VarInt::from_u8(1), first.into_iter(), &mut |w| w.wake());
        assert_eq!(res.slots, 1, "only the pre-error range was inserted");

        // The client allocates slot 10.
        assert_eq!(list.try_alloc(), Some(VarInt::from_u8(10)));
        assert_eq!(list.try_alloc(), None);

        // The byte-identical retransmission arrives (same request_id, same payload
        // → same decode error at the same offset). It MUST be rejected as a
        // duplicate so slot 10 — now live on the client — is not re-freed under it.
        let retransmit: Vec<Result<core::ops::RangeInclusive<VarInt>, DecoderError>> = vec![
            r(10, 10),
            Err(DecoderError::InvariantViolation("delta overflow")),
        ];
        let res = list.free(VarInt::from_u8(1), retransmit.into_iter(), &mut |w| w.wake());
        assert_eq!(res.slots, 0, "identical retransmission is a duplicate");
        assert_eq!(
            list.try_alloc(),
            None,
            "slot 10 must NOT be re-freed: it is live on the client"
        );
    }
}
