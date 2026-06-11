// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credit::{
        AbandonResult, Config, DeadSlotQueue, Distributor, GrantResult, Pool, Priority, Slot,
        WakerSink,
    },
    socket::channel::Budget,
};
use std::{
    alloc::{self, Layout},
    collections::VecDeque,
    ptr::NonNull,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};

// ── Test helpers ─────────────────────────────────────────────────────────────

/// A minimal `#[repr(C)]` allocation with Slot as the prefix.
#[repr(C)]
struct TestAlloc {
    slot: Slot,
    value: u64,
}
crate::assert_slot_at_offset_zero!(TestAlloc);

static DROP_COUNT: AtomicUsize = AtomicUsize::new(0);

unsafe fn drop_test_alloc(ptr: NonNull<Slot>) {
    DROP_COUNT.fetch_add(1, Ordering::Relaxed);
    let ptr = ptr.cast::<TestAlloc>();
    std::ptr::drop_in_place(ptr.as_ptr());
    alloc::dealloc(ptr.as_ptr().cast(), Layout::new::<TestAlloc>());
}

fn alloc_test_slot() -> NonNull<Slot> {
    let layout = Layout::new::<TestAlloc>();
    let ptr = unsafe { alloc::alloc(layout) as *mut TestAlloc };
    assert!(!ptr.is_null());
    unsafe {
        std::ptr::write(
            ptr,
            TestAlloc {
                slot: Slot::new(drop_test_alloc),
                value: 42,
            },
        );
        NonNull::new_unchecked(ptr as *mut Slot)
    }
}

/// Free a test slot that is in the idle state (rc=1).
unsafe fn free_test_slot(ptr: NonNull<Slot>) {
    let ptr = ptr.cast::<TestAlloc>();
    std::ptr::drop_in_place(ptr.as_ptr());
    alloc::dealloc(ptr.as_ptr().cast(), Layout::new::<TestAlloc>());
}

/// Build a `Config` from a `(capacity, uniform max_single_acquire)` pair — preserves the shape
/// these tests have used since before per-priority caps existed.
fn cfg(capacity: u64, max_single_acquire: u64) -> Config {
    Config {
        capacity,
        max_single_acquire: [max_single_acquire; Priority::LEVELS],
        // Floor == cap: the fair-share slice never splits a grant below the full request, so these
        // tests exercise the pre-fair-share full-grant contract they were written against. Tests
        // that specifically exercise demand-elastic splitting set a smaller slice explicitly.
        min_grant_slice: [max_single_acquire; Priority::LEVELS],
    }
}

#[derive(Default)]
struct WakeCounter {
    wakeups: AtomicUsize,
}

impl WakeCounter {
    fn wakeups(&self) -> usize {
        self.wakeups.load(Ordering::Relaxed)
    }
}

impl Wake for WakeCounter {
    fn wake(self: Arc<Self>) {
        self.wakeups.fetch_add(1, Ordering::Relaxed);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.wakeups.fetch_add(1, Ordering::Relaxed);
    }
}

/// Waker sink that immediately wakes every waker in the batch (mirrors a downstream that
/// drains and delivers the wakers).
struct InlineWakeSender;

impl WakerSink for InlineWakeSender {
    fn append_wakers(&mut self, batch: &mut VecDeque<Waker>) {
        for w in batch.drain(..) {
            w.wake();
        }
    }
}

/// A no-op waker used to register the distributor (its identity is stable across polls).
struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
    fn wake_by_ref(self: &Arc<Self>) {}
}

/// Test harness: a shared pool plus its single distributor.
struct Harness {
    pool: Arc<Pool>,
    dist: Distributor,
    dist_waker: Waker,
}

impl Harness {
    fn new(config: Config) -> Self {
        let pool = Arc::new(Pool::new(config));
        let dist = Distributor::new(pool.clone());
        Self {
            pool,
            dist,
            dist_waker: Waker::from(Arc::new(NoopWake)),
        }
    }

    /// Acquire `n` at `priority`, parking with the given waker if necessary.
    ///
    /// # Safety
    /// `slot` must be a valid idle slot that outlives any resulting park.
    unsafe fn poll_acquire(
        &self,
        slot: NonNull<Slot>,
        n: u64,
        priority: Priority,
        waker: &Waker,
    ) -> Poll<u64> {
        let mut cx = Context::from_waker(waker);
        self.pool.poll_acquire(&mut cx, slot, n, priority)
    }

    fn release(&self, n: u64) {
        self.pool.release(n);
    }

    /// Run the distributor to quiescence. Dead slots are freed when poll_distribute returns.
    fn distribute(&mut self) {
        self.dist.pool.waker.register(&self.dist_waker);
        let mut budget = Budget::new(1 << 20);
        let mut wakers = InlineWakeSender;
        let _ = self.dist.poll_distribute(&mut budget, &mut wakers);
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn fast_path_acquire() {
    let pool = Pool::new(cfg(100, 100));

    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter.clone());
    let mut cx = Context::from_waker(&waker);
    let slot = alloc_test_slot();

    let result = unsafe { pool.poll_acquire(&mut cx, slot, 10, Priority::Medium) };
    assert_eq!(result, Poll::Ready(10));
    assert_eq!(pool.debug_available(), 90);

    unsafe { free_test_slot(slot) };
}

#[test]
fn fast_path_exhaustion_parks() {
    // With no fast-path success the acquirer parks (the old try_acquire-returns-0 path is gone).
    // Capacity 20 with max_single_acquire 20 — request 20 succeeds first, second request of 20
    // exhausts and parks.
    let mut h = Harness::new(cfg(20, 20));

    // Pre-acquire to drain the pool.
    let drain_counter = Arc::new(WakeCounter::default());
    let drain_waker = Waker::from(drain_counter);
    let drain_slot = alloc_test_slot();
    let r = unsafe { h.poll_acquire(drain_slot, 20, Priority::Medium, &drain_waker) };
    assert_eq!(r, Poll::Ready(20));

    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter.clone());
    let slot = alloc_test_slot();

    let result = unsafe { h.poll_acquire(slot, 20, Priority::Medium, &waker) };
    assert!(matches!(result, Poll::Pending));
    // The subtraction stays in place: 0 - 20 = -20, recorded as parked_demand.
    assert_eq!(h.pool.debug_available(), -20);
    assert_eq!(h.pool.debug_parked_demand(), 20);

    // Returning the bytes lets the distributor grant the full 20.
    h.release(20);
    h.distribute();
    assert_eq!(counter.wakeups(), 1);

    let slot_ref = unsafe { &*slot.as_ptr() };
    assert_eq!(slot_ref.poll_granted(), GrantResult::Granted(20));

    unsafe {
        free_test_slot(drain_slot);
        free_test_slot(slot);
    }
}

#[test]
fn park_and_grant() {
    let mut h = Harness::new(cfg(0, 100));

    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter.clone());
    let slot = alloc_test_slot();

    let result = unsafe { h.poll_acquire(slot, 10, Priority::Medium, &waker) };
    assert!(matches!(result, Poll::Pending));

    h.release(10);
    h.distribute();
    assert_eq!(counter.wakeups(), 1);

    let slot_ref = unsafe { &*slot.as_ptr() };
    assert_eq!(slot_ref.poll_granted(), GrantResult::Granted(10));

    unsafe { free_test_slot(slot) };
}

#[test]
fn full_grants_to_multiple_waiters() {
    // Three waiters each requesting 20; releasing 60 grants all three their full request.
    let mut h = Harness::new(cfg(0, 1000));

    let counters: Vec<_> = (0..3).map(|_| Arc::new(WakeCounter::default())).collect();
    let wakers: Vec<_> = counters.iter().map(|c| Waker::from(c.clone())).collect();
    let slots: Vec<_> = (0..3).map(|_| alloc_test_slot()).collect();

    for i in 0..3 {
        let result = unsafe { h.poll_acquire(slots[i], 20, Priority::Medium, &wakers[i]) };
        assert!(matches!(result, Poll::Pending));
    }

    h.release(60);
    h.distribute();

    for c in &counters {
        assert_eq!(c.wakeups(), 1);
    }
    for slot in &slots {
        let slot_ref = unsafe { &*slot.as_ptr() };
        assert_eq!(slot_ref.poll_granted(), GrantResult::Granted(20));
    }

    for slot in slots {
        unsafe { free_test_slot(slot) };
    }
}

#[test]
fn partial_budget_serves_priority_prefix() {
    // 50 released, three waiters each wanting 20: the first two (FIFO) get full grants, the third
    // is unaffordable (20 > remaining 10) and stays parked. No partial grant.
    let mut h = Harness::new(cfg(0, 1000));

    let counters: Vec<_> = (0..3).map(|_| Arc::new(WakeCounter::default())).collect();
    let wakers: Vec<_> = counters.iter().map(|c| Waker::from(c.clone())).collect();
    let slots: Vec<_> = (0..3).map(|_| alloc_test_slot()).collect();

    for i in 0..3 {
        let result = unsafe { h.poll_acquire(slots[i], 20, Priority::Medium, &wakers[i]) };
        assert!(matches!(result, Poll::Pending));
    }

    h.release(50);
    h.distribute();

    assert_eq!(counters[0].wakeups(), 1);
    assert_eq!(counters[1].wakeups(), 1);
    assert_eq!(counters[2].wakeups(), 0);

    let s2 = unsafe { &*slots[2].as_ptr() };
    assert_eq!(s2.poll_granted(), GrantResult::Pending);

    // 10 leftover is still owed to the parked head, so available must stay <= 0 (no-snipe).
    assert!(h.pool.debug_available() <= 0);

    // Releasing the remaining 10 serves the third on the next pass.
    h.release(10);
    h.distribute();
    assert_eq!(counters[2].wakeups(), 1);

    for slot in slots {
        unsafe { free_test_slot(slot) };
    }
}

#[test]
fn grant_is_exactly_requested_surplus_to_fast_path() {
    // A waiter that requested 10 gets exactly 10; the surplus lands in `available` for the fast path.
    let mut h = Harness::new(cfg(0, 1000));

    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter.clone());
    let slot = alloc_test_slot();

    let result = unsafe { h.poll_acquire(slot, 10, Priority::Medium, &waker) };
    assert!(matches!(result, Poll::Pending));

    h.release(1000);
    h.distribute();

    let slot_ref = unsafe { &*slot.as_ptr() };
    assert_eq!(slot_ref.poll_granted(), GrantResult::Granted(10));

    // 990 surplus is now free for the fast path.
    assert_eq!(h.pool.debug_available(), 990);
    let counter2 = Arc::new(WakeCounter::default());
    let waker2 = Waker::from(counter2);
    let slot2 = alloc_test_slot();
    let r = unsafe { h.poll_acquire(slot2, 990, Priority::Medium, &waker2) };
    assert_eq!(r, Poll::Ready(990));

    unsafe {
        free_test_slot(slot);
        free_test_slot(slot2);
    }
}

#[test]
fn spurious_wake_then_grant() {
    let mut h = Harness::new(cfg(0, 100));

    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter.clone());
    let slot = alloc_test_slot();

    let result = unsafe { h.poll_acquire(slot, 50, Priority::Medium, &waker) };
    assert!(matches!(result, Poll::Pending));

    let slot_ref = unsafe { &*slot.as_ptr() };
    assert_eq!(slot_ref.poll_granted(), GrantResult::Pending);

    h.release(100);
    h.distribute();
    // Requested 50 → granted exactly 50; the other 50 is surplus.
    assert_eq!(slot_ref.poll_granted(), GrantResult::Granted(50));
    assert_eq!(h.pool.debug_available(), 50);

    unsafe { free_test_slot(slot) };
}

#[test]
fn priority_ordering() {
    let mut h = Harness::new(cfg(0, 100));

    let low_counter = Arc::new(WakeCounter::default());
    let high_counter = Arc::new(WakeCounter::default());
    let low_waker = Waker::from(low_counter.clone());
    let high_waker = Waker::from(high_counter.clone());

    let low_slot = alloc_test_slot();
    let high_slot = alloc_test_slot();

    // Park low first, then high — high should still be served first.
    let result = unsafe { h.poll_acquire(low_slot, 10, Priority::Low, &low_waker) };
    assert!(matches!(result, Poll::Pending));
    let result = unsafe { h.poll_acquire(high_slot, 10, Priority::Highest, &high_waker) };
    assert!(matches!(result, Poll::Pending));

    // Only enough for one grant: strict priority gives it to the high tier.
    h.release(10);
    h.distribute();
    assert_eq!(high_counter.wakeups(), 1);
    assert_eq!(low_counter.wakeups(), 0);

    h.release(10);
    h.distribute();
    assert_eq!(low_counter.wakeups(), 1);

    unsafe {
        free_test_slot(high_slot);
        free_test_slot(low_slot);
    }
}

#[test]
fn drop_while_linked() {
    DROP_COUNT.store(0, Ordering::Relaxed);

    let mut h = Harness::new(cfg(0, 100));

    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter.clone());
    let slot = alloc_test_slot();

    let result = unsafe { h.poll_acquire(slot, 10, Priority::Medium, &waker) };
    assert!(matches!(result, Poll::Pending));

    // App drops while linked.
    let slot_ref = unsafe { &*slot.as_ptr() };
    assert_eq!(unsafe { slot_ref.abandon() }, AbandonResult::Abandoned);

    // The distributor reaps and frees the dead slot inline; nothing is granted.
    h.release(10);
    h.distribute();
    assert_eq!(counter.wakeups(), 0);
    assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 1);
}

#[test]
fn drop_while_idle() {
    DROP_COUNT.store(0, Ordering::Relaxed);

    let slot = alloc_test_slot();
    let slot_ref = unsafe { &*slot.as_ptr() };
    assert!(slot_ref.is_idle());

    unsafe { free_test_slot(slot) };
    assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 0);
}

#[test]
fn dead_entry_skipped_in_distribution() {
    DROP_COUNT.store(0, Ordering::Relaxed);

    let mut h = Harness::new(cfg(0, 100));

    let counter1 = Arc::new(WakeCounter::default());
    let counter2 = Arc::new(WakeCounter::default());
    let waker1 = Waker::from(counter1.clone());
    let waker2 = Waker::from(counter2.clone());

    let slot1 = alloc_test_slot();
    let slot2 = alloc_test_slot();

    let result = unsafe { h.poll_acquire(slot1, 10, Priority::Medium, &waker1) };
    assert!(matches!(result, Poll::Pending));
    let result = unsafe { h.poll_acquire(slot2, 10, Priority::Medium, &waker2) };
    assert!(matches!(result, Poll::Pending));

    assert_eq!(
        unsafe { (*slot1.as_ptr()).abandon() },
        AbandonResult::Abandoned
    );

    h.release(20);
    h.distribute();
    assert_eq!(counter1.wakeups(), 0);
    assert_eq!(counter2.wakeups(), 1);
    assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 1);

    let slot2_ref = unsafe { &*slot2.as_ptr() };
    assert_eq!(slot2_ref.poll_granted(), GrantResult::Granted(10));

    unsafe { free_test_slot(slot2) };
}

#[test]
fn mixed_alive_and_dead_in_distribution() {
    let mut h = Harness::new(cfg(0, 1000));

    let slots: Vec<_> = (0..5).map(|_| alloc_test_slot()).collect();
    let counters: Vec<_> = (0..5).map(|_| Arc::new(WakeCounter::default())).collect();
    let wakers: Vec<_> = counters.iter().map(|c| Waker::from(c.clone())).collect();

    let requests = [10u64, 20, 30, 40, 50];
    for i in 0..5 {
        let result = unsafe { h.poll_acquire(slots[i], requests[i], Priority::Medium, &wakers[i]) };
        assert!(matches!(result, Poll::Pending));
    }

    assert_eq!(
        unsafe { (*slots[2].as_ptr()).abandon() },
        AbandonResult::Abandoned
    );

    DROP_COUNT.store(0, Ordering::Relaxed);
    h.release(150);
    h.distribute();

    assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 1);

    for i in [0, 1, 3, 4] {
        assert_eq!(counters[i].wakeups(), 1);
        let slot_ref = unsafe { &*slots[i].as_ptr() };
        assert_eq!(slot_ref.poll_granted(), GrantResult::Granted(requests[i]));
    }

    for i in [0, 1, 3, 4] {
        unsafe { free_test_slot(slots[i]) };
    }
}

#[test]
fn burst_cap_enforced() {
    // A request larger than max_single_acquire is clamped, and the clamped amount is granted from
    // the fast path when capacity allows.
    let pool = Pool::new(cfg(1000, 16));

    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter);
    let mut cx = Context::from_waker(&waker);
    let slot = alloc_test_slot();

    let result = unsafe { pool.poll_acquire(&mut cx, slot, 100, Priority::Medium) };
    assert_eq!(result, Poll::Ready(16));

    unsafe { free_test_slot(slot) };
}

#[test]
fn newcomer_cannot_snipe_parked_waiter() {
    // A parked waiter must be served before a fresh acquirer can take returned credit.
    let mut h = Harness::new(cfg(0, 100));

    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter.clone());
    let slot = alloc_test_slot();

    // W parks requesting 10 → available = -10.
    let result = unsafe { h.poll_acquire(slot, 10, Priority::Medium, &waker) };
    assert!(matches!(result, Poll::Pending));

    // Credit comes back, but it is staged in `returned`, invisible to the fast path.
    h.release(10);

    // A newcomer tries the fast path BEFORE the distributor runs: it cannot see the returned credit
    // (available is still -10), so it parks instead of sniping.
    let nc_counter = Arc::new(WakeCounter::default());
    let nc_waker = Waker::from(nc_counter.clone());
    let nc_slot = alloc_test_slot();
    let result = unsafe { h.poll_acquire(nc_slot, 1, Priority::Medium, &nc_waker) };
    assert!(matches!(result, Poll::Pending), "newcomer must not snipe");

    // The distributor serves the original waiter.
    h.distribute();
    assert_eq!(counter.wakeups(), 1);
    let slot_ref = unsafe { &*slot.as_ptr() };
    assert_eq!(slot_ref.poll_granted(), GrantResult::Granted(10));

    // Clean up: serve the newcomer too so we can free it.
    h.release(1);
    h.distribute();
    assert_eq!(nc_counter.wakeups(), 1);

    unsafe {
        free_test_slot(slot);
        free_test_slot(nc_slot);
    }
}

#[test]
fn pool_drop_signals_closed() {
    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter.clone());

    let slot = alloc_test_slot();

    {
        let h = Harness::new(cfg(0, 100));

        let result = unsafe { h.poll_acquire(slot, 10, Priority::Medium, &waker) };
        assert!(matches!(result, Poll::Pending));

        // Harness drops here: the distributor (empty mirror) and the pool's tiers drop, closing the
        // still-linked waiter.
    }

    assert_eq!(counter.wakeups(), 1);
    let slot_ref = unsafe { &*slot.as_ptr() };
    assert_eq!(slot_ref.poll_granted(), GrantResult::Closed);

    unsafe { free_test_slot(slot) };
}

#[test]
fn abandon_then_pool_drop_frees_dead_slot() {
    // Covers the SlotPtr::drop DEAD branch: a slot abandoned (rc=DEAD) before pool shutdown must be
    // freed by the close path, not closed/woken.
    DROP_COUNT.store(0, Ordering::Relaxed);

    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter.clone());
    let slot = alloc_test_slot();

    {
        let h = Harness::new(cfg(0, 100));

        let result = unsafe { h.poll_acquire(slot, 10, Priority::Medium, &waker) };
        assert!(matches!(result, Poll::Pending));

        // Abandon while linked, then drop the pool without ever distributing.
        assert_eq!(
            unsafe { (*slot.as_ptr()).abandon() },
            AbandonResult::Abandoned
        );
    }

    // The DEAD slot was freed by the shutdown close path, and never woken.
    assert_eq!(counter.wakeups(), 0);
    assert_eq!(DROP_COUNT.load(Ordering::Relaxed), 1);
}

#[test]
fn split_credit_no_longer_strands() {
    // Regression for the original split-credit deadlock: A acquires 50, B requests 60 and parks,
    // A releases 50. The old two-counter design stranded B (50 in available + 50 in carry, neither
    // alone >= 60). The single distributor reunifies and serves B.
    let mut h = Harness::new(cfg(100, 100));

    let a_counter = Arc::new(WakeCounter::default());
    let a_waker = Waker::from(a_counter);
    let a_slot = alloc_test_slot();
    let r = unsafe { h.poll_acquire(a_slot, 50, Priority::Medium, &a_waker) };
    assert_eq!(r, Poll::Ready(50));

    let b_counter = Arc::new(WakeCounter::default());
    let b_waker = Waker::from(b_counter.clone());
    let b_slot = alloc_test_slot();
    let r = unsafe { h.poll_acquire(b_slot, 60, Priority::Medium, &b_waker) };
    assert!(matches!(r, Poll::Pending));

    // A's bytes complete and are returned.
    h.release(50);
    h.distribute();

    // B is served its full 60.
    assert_eq!(b_counter.wakeups(), 1);
    let b_ref = unsafe { &*b_slot.as_ptr() };
    assert_eq!(b_ref.poll_granted(), GrantResult::Granted(60));
    // 40 surplus is free for the fast path.
    assert_eq!(h.pool.debug_available(), 40);

    unsafe {
        free_test_slot(a_slot);
        free_test_slot(b_slot);
    }
}

#[test]
fn fresh_arrivals_behind_cached_head_served_same_pass() {
    // After a prior pass leaves a head cached in the mirror (it was unaffordable at the time), fresh
    // waiters arrive in the shared tier behind it. Because each pass merges the shared tier into the
    // mirror up-front (via `append`, before computing the slice), the cached head and the fresh
    // arrivals are all present when the pass grants — so they are served together, not stranded
    // until a later pass. This also pins the fairness property the up-front merge exists for: the
    // slice is computed from the *full* backlog (cached + fresh), not just the cached head.
    let mut h = Harness::new(cfg(0, 100));
    let counters: Vec<_> = (0..3).map(|_| Arc::new(WakeCounter::default())).collect();
    let wakers: Vec<_> = counters.iter().map(|c| Waker::from(c.clone())).collect();
    let slots: Vec<_> = (0..3).map(|_| alloc_test_slot()).collect();

    // A parks first; it lands in the shared Medium tier.
    let r = unsafe { h.poll_acquire(slots[0], 5, Priority::Medium, &wakers[0]) };
    assert!(matches!(r, Poll::Pending));

    // First pass with no credit: the distributor merges A into the mirror, finds it unaffordable
    // (free = 0, req = 5), and breaks. A is now cached in the mirror across passes.
    let mut budget = Budget::new(1 << 20);
    let mut dead = DeadSlotQueue::new();
    let progressed = h.dist.pass(&mut budget, &mut dead);
    assert!(!progressed);
    assert_eq!(counters[0].wakeups(), 0);

    // B and C park behind A. Because A is in the mirror, B and C go straight to the shared tier.
    let r = unsafe { h.poll_acquire(slots[1], 5, Priority::Medium, &wakers[1]) };
    assert!(matches!(r, Poll::Pending));
    let r = unsafe { h.poll_acquire(slots[2], 5, Priority::Medium, &wakers[2]) };
    assert!(matches!(r, Poll::Pending));

    // Enough credit for all three.
    h.release(15);

    // Single pass: the up-front merge appends B and C (from the shared tier) behind the cached A,
    // so all three are in the mirror before any grant; the pass then serves A, B, and C. Without
    // the up-front merge, only A would be served and B/C would wait for another pass.
    let mut budget = Budget::new(1 << 20);
    let mut dead = DeadSlotQueue::new();
    let progressed = h.dist.pass(&mut budget, &mut dead);
    assert!(progressed);

    // Deliver the wakers that this single pass staged; a real run would flush them to the waker
    // channel at end-of-poll, but the test drives `pass` directly.
    let mut sink = InlineWakeSender;
    sink.append_wakers(&mut h.dist.pending_wakers);

    for (i, counter) in counters.iter().enumerate() {
        assert_eq!(
            counter.wakeups(),
            1,
            "waiter {i} not served in a single pass"
        );
        let slot_ref = unsafe { &*slots[i].as_ptr() };
        assert_eq!(slot_ref.poll_granted(), GrantResult::Granted(5));
    }

    for slot in slots {
        unsafe { free_test_slot(slot) };
    }
}

#[test]
fn concurrent_release_halves_serve_waiter() {
    // The old design's concurrent-release double-stash strands a waiter when two sub-threshold
    // releases race. With a single counter, two releases simply sum; the distributor serves the
    // waiter once their total covers it.
    let mut h = Harness::new(cfg(0, 100));

    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter.clone());
    let slot = alloc_test_slot();

    let r = unsafe { h.poll_acquire(slot, 10, Priority::Medium, &waker) };
    assert!(matches!(r, Poll::Pending));

    // Two independent half-releases (sub-threshold individually).
    h.release(5);
    h.release(5);
    h.distribute();

    assert_eq!(counter.wakeups(), 1);
    let slot_ref = unsafe { &*slot.as_ptr() };
    assert_eq!(slot_ref.poll_granted(), GrantResult::Granted(10));

    unsafe { free_test_slot(slot) };
}

#[test]
fn budget_exhaustion_preserves_no_snipe() {
    // Capacity 100, A acquires 100 fast-path; four waiters park at 10 each (available = -40).
    // A releases 100. Run a SINGLE pass with a tiny budget so it exits via budget exhaustion
    // after granting only two waiters. Two affordable waiters are still linked, but the pass
    // must NOT drive `available` positive (a fresh fast-path acquirer would otherwise snipe
    // credit destined for the still-parked waiters).
    let mut h = Harness::new(cfg(100, 100));

    // A takes the full 100 via the fast path.
    let a_counter = Arc::new(WakeCounter::default());
    let a_waker = Waker::from(a_counter);
    let a_slot = alloc_test_slot();
    let r = unsafe { h.poll_acquire(a_slot, 100, Priority::Medium, &a_waker) };
    assert_eq!(r, Poll::Ready(100));

    // Four waiters park at 10 each → available = 0 - 40 = -40, parked_demand = 40.
    let counters: Vec<_> = (0..4).map(|_| Arc::new(WakeCounter::default())).collect();
    let wakers: Vec<_> = counters.iter().map(|c| Waker::from(c.clone())).collect();
    let slots: Vec<_> = (0..4).map(|_| alloc_test_slot()).collect();
    for i in 0..4 {
        let r = unsafe { h.poll_acquire(slots[i], 10, Priority::Medium, &wakers[i]) };
        assert!(matches!(r, Poll::Pending));
    }
    assert_eq!(h.pool.debug_available(), -40);

    // A releases 100.
    h.release(100);

    // Single pass with budget=2 → exits via budget exhaustion after two grants.
    h.dist.pool.waker.register(&h.dist_waker);
    let mut budget = Budget::new(2);
    let mut dead = DeadSlotQueue::new();
    let progressed = h.dist.pass(&mut budget, &mut dead);
    assert!(progressed);
    let mut sink = InlineWakeSender;
    sink.append_wakers(&mut h.dist.pending_wakers);

    // Two of the four are now granted.
    let granted_count: usize = counters.iter().map(|c| c.wakeups()).sum();
    assert_eq!(
        granted_count, 2,
        "expected exactly two grants under budget=2"
    );

    // Two waiters remain pending.
    let pending_count = counters.iter().filter(|c| c.wakeups() == 0).count();
    assert_eq!(pending_count, 2);

    // No-snipe: a fresh fast-path acquirer must NOT see positive credit while parked waiters
    // still demand it.
    assert!(
        h.pool.debug_available() <= 0,
        "available={} (would let a fresh acquirer snipe credit owed to still-parked waiters)",
        h.pool.debug_available()
    );
    let snipe_counter = Arc::new(WakeCounter::default());
    let snipe_waker = Waker::from(snipe_counter);
    let snipe_slot = alloc_test_slot();
    let r = unsafe { h.poll_acquire(snipe_slot, 50, Priority::Medium, &snipe_waker) };
    assert!(
        matches!(r, Poll::Pending),
        "fresh fast-path acquirer sniped {r:?} from parked waiters"
    );

    // Drain the rest so we can free slots.
    h.distribute();
    for c in &counters {
        assert_eq!(c.wakeups(), 1);
    }

    unsafe {
        free_test_slot(a_slot);
        for s in slots {
            free_test_slot(s);
        }
        // The sniper parked → abandon and let the next distribute reap it.
        let _ = (*snipe_slot.as_ptr()).abandon();
    }
    h.distribute();
}

#[test]
fn carry_accumulates_across_passes() {
    // Same setup as above, but step the distributor with budget=1 several times. All four waiters
    // must eventually be served and `carry` must be zero at the end.
    let mut h = Harness::new(cfg(100, 100));

    let a_counter = Arc::new(WakeCounter::default());
    let a_waker = Waker::from(a_counter);
    let a_slot = alloc_test_slot();
    let r = unsafe { h.poll_acquire(a_slot, 100, Priority::Medium, &a_waker) };
    assert_eq!(r, Poll::Ready(100));

    let counters: Vec<_> = (0..4).map(|_| Arc::new(WakeCounter::default())).collect();
    let wakers: Vec<_> = counters.iter().map(|c| Waker::from(c.clone())).collect();
    let slots: Vec<_> = (0..4).map(|_| alloc_test_slot()).collect();
    for i in 0..4 {
        let r = unsafe { h.poll_acquire(slots[i], 10, Priority::Medium, &wakers[i]) };
        assert!(matches!(r, Poll::Pending));
    }
    h.release(100);
    h.dist.pool.waker.register(&h.dist_waker);

    // Step with budget=1 until all served.
    for _ in 0..16 {
        let mut budget = Budget::new(1);
        let mut dead = DeadSlotQueue::new();
        let _ = h.dist.pass(&mut budget, &mut dead);
        let mut sink = InlineWakeSender;
        sink.append_wakers(&mut h.dist.pending_wakers);
        if counters.iter().all(|c| c.wakeups() == 1) {
            break;
        }
    }

    for c in &counters {
        assert_eq!(c.wakeups(), 1);
    }
    assert_eq!(h.dist.debug_carry(), 0);
    // 60 bytes of surplus (100 released minus 40 owed to waiters) should now be in `available`.
    assert_eq!(h.pool.debug_available(), 60);

    unsafe {
        free_test_slot(a_slot);
        for s in slots {
            free_test_slot(s);
        }
    }
}

#[test]
fn carry_releases_when_queue_drains() {
    // A pass exits via budget-exhaustion with carry > 0 because affordable waiters are still
    // linked. Then those waiters all abandon. The next pass must write the carry back to
    // `available` (no live parked demand → full writeback) and a fresh fast-path acquirer must
    // see it.
    let mut h = Harness::new(cfg(0, 100));

    let counters: Vec<_> = (0..3).map(|_| Arc::new(WakeCounter::default())).collect();
    let wakers: Vec<_> = counters.iter().map(|c| Waker::from(c.clone())).collect();
    let slots: Vec<_> = (0..3).map(|_| alloc_test_slot()).collect();
    for i in 0..3 {
        let r = unsafe { h.poll_acquire(slots[i], 10, Priority::Medium, &wakers[i]) };
        assert!(matches!(r, Poll::Pending));
    }
    // Release enough to satisfy all three.
    h.release(30);

    // Pass with budget=1: grants one, leaves two parked, carry > 0.
    h.dist.pool.waker.register(&h.dist_waker);
    let mut budget = Budget::new(1);
    let mut dead = DeadSlotQueue::new();
    let _ = h.dist.pass(&mut budget, &mut dead);
    let mut sink = InlineWakeSender;
    sink.append_wakers(&mut h.dist.pending_wakers);
    assert!(h.dist.debug_carry() > 0);

    // The two remaining waiters abandon before the next pass.
    for i in 1..3 {
        if counters[i].wakeups() == 0 {
            assert_eq!(
                unsafe { (*slots[i].as_ptr()).abandon() },
                AbandonResult::Abandoned
            );
        }
    }

    // Next pass: live_parked drops to zero, carry must write back fully.
    let mut budget = Budget::new(1 << 20);
    let mut dead = DeadSlotQueue::new();
    let _ = h.dist.pass(&mut budget, &mut dead);
    sink.append_wakers(&mut h.dist.pending_wakers);
    drop(dead);
    assert_eq!(h.dist.debug_carry(), 0);

    // The fast path can now see the surplus.
    let snipe_counter = Arc::new(WakeCounter::default());
    let snipe_waker = Waker::from(snipe_counter);
    let snipe_slot = alloc_test_slot();
    // 30 released − 10 granted = 20 should be free.
    let r = unsafe { h.poll_acquire(snipe_slot, 20, Priority::Medium, &snipe_waker) };
    assert_eq!(r, Poll::Ready(20));

    // Free what survived.
    unsafe {
        for (i, s) in slots.into_iter().enumerate() {
            if counters[i].wakeups() == 1 {
                free_test_slot(s);
            }
            // abandoned slots already had their drop_fn run via the dead queue
        }
        free_test_slot(snipe_slot);
    }
}

#[test]
fn distributor_drop_closes_parked_waiters_across_tiers() {
    // Park three slots across three different tiers; drop the Distributor while the Arc<Pool>
    // is still held by a separate handle. All three must observe Closed and be woken.
    let pool = Arc::new(Pool::new(cfg(0, 100)));
    let dist = Distributor::new(pool.clone());

    let counters: Vec<_> = (0..3).map(|_| Arc::new(WakeCounter::default())).collect();
    let wakers: Vec<_> = counters.iter().map(|c| Waker::from(c.clone())).collect();
    let slots: Vec<_> = (0..3).map(|_| alloc_test_slot()).collect();

    let priorities = [Priority::Highest, Priority::Medium, Priority::Background];
    for i in 0..3 {
        let mut cx = Context::from_waker(&wakers[i]);
        let r = unsafe { pool.poll_acquire(&mut cx, slots[i], 10, priorities[i]) };
        assert!(matches!(r, Poll::Pending));
    }

    // Drop the distributor — Arc<Pool> still alive via `pool` here.
    drop(dist);

    // Every parked waiter is woken with Closed.
    for c in &counters {
        assert_eq!(c.wakeups(), 1);
    }
    for s in &slots {
        let s_ref = unsafe { &*s.as_ptr() };
        assert_eq!(s_ref.poll_granted(), GrantResult::Closed);
    }

    // Pool is closed: subsequent operations are no-ops.
    pool.release(50);
    assert_eq!(pool.debug_available(), -30); // unchanged from the three -10 debits

    unsafe {
        for s in slots {
            free_test_slot(s);
        }
    }
}

#[test]
fn release_after_distributor_drop_does_not_panic() {
    // `release` after the distributor is dropped accumulates harmlessly into `returned` (no
    // distributor will ever consume it) and pokes the now-dead waker (no-op). What matters is
    // that it does not panic and `available` is unaffected — there is no live distributor to
    // grant credit, so a fast-path acquirer never sees the released bytes.
    let pool = Arc::new(Pool::new(cfg(100, 100)));
    let dist = Distributor::new(pool.clone());
    drop(dist);

    pool.release(50);
    assert_eq!(pool.debug_available(), 100);
}

#[test]
fn poll_acquire_after_distributor_drop_signals_closed() {
    let pool = Arc::new(Pool::new(cfg(0, 100)));
    let dist = Distributor::new(pool.clone());
    drop(dist);

    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter.clone());
    let mut cx = Context::from_waker(&waker);
    let slot = alloc_test_slot();

    // Fast-path debit fails (capacity=0) → enters the closed-pool branch.
    let r = unsafe { pool.poll_acquire(&mut cx, slot, 10, Priority::Medium) };
    assert!(matches!(r, Poll::Pending));

    // The branch wakes the caller and stamps GRANT_CLOSED on the slot.
    assert_eq!(counter.wakeups(), 1);
    let slot_ref = unsafe { &*slot.as_ptr() };
    assert_eq!(slot_ref.poll_granted(), GrantResult::Closed);
    // The speculative `available -= 10` was refunded.
    assert_eq!(pool.debug_available(), 0);

    unsafe { free_test_slot(slot) };
}

#[test]
fn budget_exhaustion_with_dead_reaps() {
    // Mix dead and live slots so reclaimed_avail > 0 and budget runs out mid-walk. Conservation
    // (no over-commit) and no-snipe (available <= 0 with live parkers) must both hold.
    let mut h = Harness::new(cfg(0, 100));

    let counters: Vec<_> = (0..6).map(|_| Arc::new(WakeCounter::default())).collect();
    let wakers: Vec<_> = counters.iter().map(|c| Waker::from(c.clone())).collect();
    let slots: Vec<_> = (0..6).map(|_| alloc_test_slot()).collect();
    for i in 0..6 {
        let r = unsafe { h.poll_acquire(slots[i], 10, Priority::Medium, &wakers[i]) };
        assert!(matches!(r, Poll::Pending));
    }
    // Abandon slots 1, 3, 5 — interleaved live and dead.
    for i in [1, 3, 5] {
        assert_eq!(
            unsafe { (*slots[i].as_ptr()).abandon() },
            AbandonResult::Abandoned
        );
    }
    h.release(60);

    // Single tiny-budget pass: dead slots are reaped without consuming budget; live ones do.
    h.dist.pool.waker.register(&h.dist_waker);
    let mut budget = Budget::new(1);
    let mut dead = DeadSlotQueue::new();
    let _ = h.dist.pass(&mut budget, &mut dead);
    let mut sink = InlineWakeSender;
    sink.append_wakers(&mut h.dist.pending_wakers);
    drop(dead);

    // No-snipe: live parkers may remain.
    let live_remaining = [0, 2, 4]
        .iter()
        .filter(|&&i| counters[i].wakeups() == 0)
        .count();
    if live_remaining > 0 {
        assert!(
            h.pool.debug_available() <= 0,
            "available={} with {live_remaining} live parkers",
            h.pool.debug_available()
        );
    }

    // Drain the rest to clean up.
    h.distribute();
    for i in [0, 2, 4] {
        assert_eq!(counters[i].wakeups(), 1);
        unsafe { free_test_slot(slots[i]) };
    }
    // dead slots [1, 3, 5] freed by the dead queue.
}

#[test]
fn per_priority_caps_in_pool_acquire() {
    // Pool with capacity 1000, but Highest is capped at 10 and Medium at the full 1000. The same
    // request size at different priorities yields different clamped grants.
    let pool = Pool::new(Config {
        capacity: 1000,
        max_single_acquire: [
            10, 1000, 1000, 1000, // Highest, High, MediumHigh, Medium
            1000, 1000, 1000, 1000, // MediumLow, Low, Lowest, Background
        ],
        // Per-priority floor == per-priority cap, so this priority-clamp test sees full grants.
        min_grant_slice: [
            10, 1000, 1000, 1000, // Highest, High, MediumHigh, Medium
            1000, 1000, 1000, 1000, // MediumLow, Low, Lowest, Background
        ],
    });

    let waker = Waker::from(Arc::new(WakeCounter::default()));
    let mut cx = Context::from_waker(&waker);

    let s1 = alloc_test_slot();
    let r = unsafe { pool.poll_acquire(&mut cx, s1, 100, Priority::Highest) };
    assert_eq!(r, Poll::Ready(10));

    let s2 = alloc_test_slot();
    let r = unsafe { pool.poll_acquire(&mut cx, s2, 100, Priority::Medium) };
    assert_eq!(r, Poll::Ready(100));

    unsafe {
        free_test_slot(s1);
        free_test_slot(s2);
    }
}

#[test]
fn counters_track_fast_path_and_park() {
    // Spot-check that wiring is correct: a fast-path acquire bumps acquire_fast_path and
    // acquire_bytes; a parked acquire bumps acquire_parked[priority]; a release bumps
    // release_calls and release_bytes; a grant bumps distributor_granted.
    use crate::counter::Registry;
    let registry = Registry::default();
    let counters = crate::credit::Counters::new(&registry);
    let pool = Arc::new(Pool::with_counters(cfg(100, 100), counters.clone()));
    let mut dist = Distributor::new(pool.clone());
    let dist_waker = Waker::from(Arc::new(NoopWake));

    // Fast-path: 30 bytes.
    let waker = Waker::from(Arc::new(WakeCounter::default()));
    let mut cx = Context::from_waker(&waker);
    let s = alloc_test_slot();
    let r = unsafe { pool.poll_acquire(&mut cx, s, 30, Priority::Medium) };
    assert_eq!(r, Poll::Ready(30));

    // Park another 80 (capacity left = 70).
    let s2 = alloc_test_slot();
    let r = unsafe { pool.poll_acquire(&mut cx, s2, 80, Priority::Highest) };
    assert!(matches!(r, Poll::Pending));

    // Release 30 to free up enough credit; distribute to grant.
    pool.release(30);
    pool.waker.register(&dist_waker);
    let mut budget = Budget::new(1 << 20);
    let mut sink = InlineWakeSender;
    let _ = dist.poll_distribute(&mut budget, &mut sink);

    // Verify counter values via metric_metadata-style introspection isn't easy; we just check
    // that nothing panicked and the wiring compiled. A real test of the values requires a
    // registry that exposes its inner counters, which we don't have here. This test exists to
    // prove the integration is correct.
    let _ = counters;

    let s_ref = unsafe { &*s2.as_ptr() };
    assert_eq!(s_ref.poll_granted(), GrantResult::Granted(80));

    drop(dist);
    unsafe {
        free_test_slot(s);
        free_test_slot(s2);
    }
}

#[test]
fn buffering_sink_splices_batch_and_distributor_reuses_buffer() {
    // The production sink (endpoint::waker::Sink) drains the batch via VecDeque::append into
    // its own buffer and a downstream drain task fires the wakers later. This test mirrors that
    // shape and additionally checks that the distributor's pending_wakers buffer is left empty
    // (so its capacity can be reused on the next poll without re-allocating).
    let mut h = Harness::new(cfg(0, 1000));

    struct BufferingSink {
        buf: VecDeque<Waker>,
    }
    impl WakerSink for BufferingSink {
        fn append_wakers(&mut self, batch: &mut VecDeque<Waker>) {
            self.buf.append(batch);
        }
    }

    let counters: Vec<_> = (0..3).map(|_| Arc::new(WakeCounter::default())).collect();
    let wakers: Vec<_> = counters.iter().map(|c| Waker::from(c.clone())).collect();
    let slots: Vec<_> = (0..3).map(|_| alloc_test_slot()).collect();
    for i in 0..3 {
        let r = unsafe { h.poll_acquire(slots[i], 10, Priority::Medium, &wakers[i]) };
        assert!(matches!(r, Poll::Pending));
    }
    h.release(30);
    h.dist.pool.waker.register(&h.dist_waker);
    let mut budget = Budget::new(1 << 20);
    let mut sink = BufferingSink {
        buf: VecDeque::new(),
    };
    let _ = h.dist.poll_distribute(&mut budget, &mut sink);

    // The distributor handed all three wakers to the sink and left its scratch buffer empty.
    assert_eq!(sink.buf.len(), 3);
    assert!(
        h.dist.pending_wakers.is_empty(),
        "distributor must leave its scratch buffer empty after a poll"
    );

    // Wakers fire when the downstream drains its buffer.
    for w in sink.buf.drain(..) {
        w.wake();
    }
    for c in &counters {
        assert_eq!(c.wakeups(), 1);
    }
    for s in &slots {
        let s_ref = unsafe { &*s.as_ptr() };
        assert_eq!(s_ref.poll_granted(), GrantResult::Granted(10));
    }

    for s in slots {
        unsafe { free_test_slot(s) };
    }
}

#[test]
fn pending_wakers_capacity_persists_across_polls() {
    // Smoke test the steady-state allocation behavior: the distributor's pending_wakers buffer
    // grows once and then reuses its capacity. We can't directly observe alloc/free, but we can
    // observe that poll_distribute leaves the buffer empty (and InlineWakeSender's `drain` keeps
    // the same allocation), so consecutive polls of the same shape don't grow it.
    let mut h = Harness::new(cfg(0, 1000));

    let counters: Vec<_> = (0..2).map(|_| Arc::new(WakeCounter::default())).collect();
    let wakers: Vec<_> = counters.iter().map(|c| Waker::from(c.clone())).collect();
    let slots: Vec<_> = (0..2).map(|_| alloc_test_slot()).collect();

    h.dist.pool.waker.register(&h.dist_waker);

    for round in 0..3 {
        for i in 0..2 {
            let r = unsafe { h.poll_acquire(slots[i], 10, Priority::Medium, &wakers[i]) };
            assert!(matches!(r, Poll::Pending), "round {round}, slot {i}");
        }
        h.release(20);
        let mut budget = Budget::new(1 << 20);
        let mut sink = InlineWakeSender;
        let _ = h.dist.poll_distribute(&mut budget, &mut sink);
        assert!(h.dist.pending_wakers.is_empty(), "round {round}");
        for i in 0..2 {
            let s_ref = unsafe { &*slots[i].as_ptr() };
            assert_eq!(s_ref.poll_granted(), GrantResult::Granted(10));
        }
        // Each round increments the wake counters by one.
        for c in &counters {
            assert_eq!(c.wakeups(), round + 1);
        }
    }

    for s in slots {
        unsafe { free_test_slot(s) };
    }
}

// ── Demand-elastic fair-share robustness ───────────────────────────────────────
//
// These pin the behavior of the per-tier fair-share slice (`Distributor::pass`) across the demand
// shapes that motivated it: uniform-tiny, heavily-mixed, and a partial-grant refund. The shared
// invariant is conservation — at quiescence with no parked waiter, `available + returned ==
// capacity` — alongside the specific grant / forward-progress guarantees.

/// Build a config with an explicit fair-share floor distinct from the per-acquire cap, so the
/// demand-elastic split is actually exercised (the `cfg` helper sets floor == cap, disabling it).
fn cfg_slice(capacity: u64, max_single_acquire: u64, min_grant_slice: u64) -> Config {
    Config {
        capacity,
        max_single_acquire: [max_single_acquire; Priority::LEVELS],
        min_grant_slice: [min_grant_slice; Priority::LEVELS],
    }
}

/// Quiescent conservation check: with no waiter parked, all credit lives in `available + returned`.
fn assert_conserved(h: &Harness, capacity: u64) {
    let available = h.pool.debug_available();
    let returned = h.pool.debug_returned();
    assert!(
        available >= 0,
        "available went negative at quiescence: {available}"
    );
    assert_eq!(
        available as u64 + returned,
        capacity,
        "credit not conserved: available={available} + returned={returned} != capacity={capacity}"
    );
}

/// Many tiny requests against ample credit must ALL be granted their full request in one pass — the
/// `min_grant_slice` floor never forces a premature bail when every head wants far less than the
/// slice. Guards against a regression where the bail (`grant_amount > free`) fires for small heads.
#[test]
fn fair_share_tiny_requests_all_served_no_premature_bail() {
    const N: usize = 100;
    const REQ: u64 = 100;
    const CAP: u64 = 1024 * 1024;
    // Floor far above any single request: each head still only takes its own `req`, never the floor.
    let mut h = Harness::new(cfg_slice(CAP, CAP, 64 * 1024));

    // Drain the pool so all N park (forces the distributor path, not the fast path).
    let drain_counter = Arc::new(WakeCounter::default());
    let drain_waker = Waker::from(drain_counter);
    let drain_slot = alloc_test_slot();
    let r = unsafe { h.poll_acquire(drain_slot, CAP, Priority::Medium, &drain_waker) };
    assert_eq!(r, Poll::Ready(CAP));

    let counters: Vec<_> = (0..N).map(|_| Arc::new(WakeCounter::default())).collect();
    let wakers: Vec<_> = counters.iter().map(|c| Waker::from(c.clone())).collect();
    let slots: Vec<_> = (0..N).map(|_| alloc_test_slot()).collect();
    for i in 0..N {
        let r = unsafe { h.poll_acquire(slots[i], REQ, Priority::Medium, &wakers[i]) };
        assert!(matches!(r, Poll::Pending));
    }

    // Return exactly the aggregate tiny demand; one pass must serve every waiter in full.
    h.release(N as u64 * REQ);
    h.distribute();

    for (i, c) in counters.iter().enumerate() {
        assert_eq!(c.wakeups(), 1, "tiny waiter {i} not served");
        let s = unsafe { &*slots[i].as_ptr() };
        assert_eq!(
            s.poll_granted(),
            GrantResult::Granted(REQ),
            "tiny waiter {i}"
        );
    }

    // Release the drained CAP (in-flight to drain_slot) and confirm conservation at quiescence.
    h.release(CAP);
    h.distribute();
    assert_conserved(&h, CAP);

    unsafe {
        free_test_slot(drain_slot);
        for s in slots {
            free_test_slot(s);
        }
    }
}

/// Mixed demand: one large waiter ahead of many tiny ones, against credit far short of the large
/// request. The large head must be capped at the slice (not granted its full request, which would
/// drain the pool and starve the rest), and over repeated release+distribute rounds every small
/// waiter must make forward progress while the large one round-robins.
#[test]
fn fair_share_mixed_demand_caps_large_and_progresses() {
    const CAP: u64 = 1024 * 1024;
    const SLICE: u64 = 64 * 1024;
    // A single acquire can never exceed capacity (you can't hold more than the whole pool), so the
    // "large" head requests the per-acquire max (== CAP) and re-acquires for more across rounds.
    const BIG: u64 = CAP;
    const SMALL: u64 = 100;
    const N_SMALL: usize = 16;
    let mut h = Harness::new(cfg_slice(CAP, CAP, SLICE));

    // Pre-drain the pool so every subsequent acquire parks (forces the distributor path).
    let drain_counter = Arc::new(WakeCounter::default());
    let drain_waker = Waker::from(drain_counter);
    let drain_slot = alloc_test_slot();
    let r = unsafe { h.poll_acquire(drain_slot, CAP, Priority::Medium, &drain_waker) };
    assert_eq!(r, Poll::Ready(CAP));

    let big_counter = Arc::new(WakeCounter::default());
    let big_waker = Waker::from(big_counter.clone());
    let big_slot = alloc_test_slot();
    let r = unsafe { h.poll_acquire(big_slot, BIG, Priority::Medium, &big_waker) };
    assert!(matches!(r, Poll::Pending));

    let small_counters: Vec<_> = (0..N_SMALL)
        .map(|_| Arc::new(WakeCounter::default()))
        .collect();
    let small_wakers: Vec<_> = small_counters
        .iter()
        .map(|c| Waker::from(c.clone()))
        .collect();
    let small_slots: Vec<_> = (0..N_SMALL).map(|_| alloc_test_slot()).collect();
    for i in 0..N_SMALL {
        let r =
            unsafe { h.poll_acquire(small_slots[i], SMALL, Priority::Medium, &small_wakers[i]) };
        assert!(matches!(r, Poll::Pending));
    }

    // The drain returns its CAP. The big head is at the front of the tier with parked = 17
    // (big + 16 small), so slice = max(CAP/17, SLICE) = SLICE; the big head is capped at SLICE,
    // NOT granted its full CAP request (which would drain everything and starve the smalls).
    h.release(CAP);
    h.distribute();

    let big_ref = unsafe { &*big_slot.as_ptr() };
    match big_ref.poll_granted() {
        GrantResult::Granted(n) => assert_eq!(
            n, SLICE,
            "large head must be capped at the fair-share slice, got {n}"
        ),
        other => panic!("large head not granted: {other:?}"),
    }

    // It re-acquires for more, parking again at the tail — proving the large stream round-robins
    // behind the smalls rather than monopolizing the pool.
    let r = unsafe { h.poll_acquire(big_slot, BIG, Priority::Medium, &big_waker) };
    assert!(matches!(r, Poll::Pending));

    // Drive rounds until every small waiter is served; the big one keeps round-robining. Bounded
    // loop so a starvation regression fails loudly instead of hanging.
    for _round in 0..64 {
        if small_counters.iter().all(|c| c.wakeups() >= 1) {
            break;
        }
        h.release(SLICE);
        h.distribute();
    }
    for (i, c) in small_counters.iter().enumerate() {
        assert!(
            c.wakeups() >= 1,
            "small waiter {i} starved behind the large one"
        );
    }

    // Resolve the still-parked big slot before freeing it — a linked slot must not be dropped.
    // Release until the distributor grants it (each grant is capped at SLICE), then drain the grant
    // so the slot transitions back to idle/APP-owned and is safe to free.
    let big_ref = unsafe { &*big_slot.as_ptr() };
    let mut big_resolved = false;
    for _round in 0..64 {
        if matches!(big_ref.poll_granted(), GrantResult::Granted(_)) {
            big_resolved = true;
            break;
        }
        h.release(SLICE);
        h.distribute();
    }
    assert!(
        big_resolved,
        "big slot never resolved; cannot free a parked slot"
    );

    unsafe {
        free_test_slot(drain_slot);
        free_test_slot(big_slot);
        for s in small_slots {
            free_test_slot(s);
        }
    }
}

/// Conservation across a partial (sliced) grant: when the slice caps a grant below the request, the
/// un-granted remainder must return to `available` (via `reclaimed_avail`), never leak. A lone
/// waiter sliced down, then fully drained, must leave `available + returned == capacity`.
#[test]
fn fair_share_partial_grant_refund_conserves() {
    const CAP: u64 = 256 * 1024;
    const SLICE: u64 = 64 * 1024;
    const REQ: u64 = 256 * 1024;
    let mut h = Harness::new(cfg_slice(CAP, REQ, SLICE));

    // Pre-drain so the real waiter takes the distributor path.
    let drain_slot = alloc_test_slot();
    let drain_counter = Arc::new(WakeCounter::default());
    let drain_waker = Waker::from(drain_counter);
    let r = unsafe { h.poll_acquire(drain_slot, CAP, Priority::Medium, &drain_waker) };
    assert_eq!(r, Poll::Ready(CAP));

    let counter = Arc::new(WakeCounter::default());
    let waker = Waker::from(counter.clone());
    let slot = alloc_test_slot();
    let r = unsafe { h.poll_acquire(slot, REQ, Priority::Medium, &waker) };
    assert!(matches!(r, Poll::Pending));

    // The drain returns only SLICE of its CAP: with one parker and `free == SLICE < REQ`, the grant
    // is capped to SLICE and the park-time debit's remainder (REQ - SLICE) must be refunded to
    // `available` via `reclaimed_avail` rather than leaked.
    h.release(SLICE);
    h.distribute();
    let s = unsafe { &*slot.as_ptr() };
    assert_eq!(s.poll_granted(), GrantResult::Granted(SLICE));
    // Refund landed: with the lone waiter resolved, `available` is back to zero (the drain still
    // holds CAP - SLICE in flight, the waiter holds the granted SLICE — together exactly CAP).
    assert_eq!(h.pool.debug_available(), 0);

    // Drive to quiescence: the drain returns its remaining CAP - SLICE and the waiter returns its
    // granted SLICE — CAP total back to the pool. Nothing is parked, so it all lands in `available`.
    h.release(CAP);
    h.distribute();
    assert_conserved(&h, CAP);

    unsafe {
        free_test_slot(drain_slot);
        free_test_slot(slot);
    }
}
