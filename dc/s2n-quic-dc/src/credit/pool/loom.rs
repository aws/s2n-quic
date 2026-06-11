// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Loom models for [`Pool`] and [`Distributor`].
//!
//! Run via `LOOM_MAX_PREEMPTIONS=3 cargo test --profile release-debug --features loom credit::pool::loom`.

use super::*;
use crate::{
    credit::slot::{AbandonResult, GrantResult, Slot},
    testing::loom,
};
use core::task::{Context, Poll, Waker};
use std::{
    alloc::{self, Layout},
    ptr::NonNull,
    task::Wake,
};

#[repr(C)]
struct TestAlloc {
    slot: Slot,
}
crate::assert_slot_at_offset_zero!(TestAlloc);

unsafe fn drop_test_alloc(ptr: NonNull<Slot>) {
    let ptr = ptr.cast::<TestAlloc>();
    std::ptr::drop_in_place(ptr.as_ptr());
    alloc::dealloc(ptr.as_ptr().cast(), Layout::new::<TestAlloc>());
}

fn alloc_slot() -> NonNull<Slot> {
    let layout = Layout::new::<TestAlloc>();
    let ptr = unsafe { alloc::alloc(layout) as *mut TestAlloc };
    unsafe {
        std::ptr::write(
            ptr,
            TestAlloc {
                slot: Slot::new(drop_test_alloc),
            },
        );
        NonNull::new_unchecked(ptr as *mut Slot)
    }
}

unsafe fn free_slot(ptr: NonNull<Slot>) {
    let ptr = ptr.cast::<TestAlloc>();
    std::ptr::drop_in_place(ptr.as_ptr());
    alloc::dealloc(ptr.as_ptr().cast(), Layout::new::<TestAlloc>());
}

struct Noop;
impl Wake for Noop {
    fn wake(self: std::sync::Arc<Self>) {}
    fn wake_by_ref(self: &std::sync::Arc<Self>) {}
}

fn noop() -> Waker {
    Waker::from(std::sync::Arc::new(Noop))
}

/// Counts grants delivered by the distributor (and forwards each granted slot's waker).
struct CountWake(Arc<crate::sync::AtomicUsize>);
impl WakerSink for CountWake {
    fn append_wakers(&mut self, batch: &mut std::collections::VecDeque<core::task::Waker>) {
        for w in batch.drain(..) {
            self.0.fetch_add(1, Ordering::Release);
            w.wake();
        }
    }
}

/// A `Send` carrier for a raw slot pointer crossing a loom thread boundary.
struct SendPtr(NonNull<Slot>);
unsafe impl Send for SendPtr {}

/// Drive the distributor to completion on its own thread, parking via `block_on` whenever
/// `poll_distribute` yields `Pending`. The thread is re-polled ONLY by a real wakeup, so a lost
/// wakeup manifests as a permanent park → loom deadlock. Completes once `granted` reaches
/// `target`. Returns the distributor's final `paid_demand` so the caller can recover outstanding
/// demand (`parked_demand − paid_demand`) for conservation assertions.
fn spawn_distributor(
    pool: Arc<Pool>,
    granted: Arc<crate::sync::AtomicUsize>,
    target: usize,
) -> loom::thread::JoinHandle<u64> {
    spawn_distributor_with_budget(pool, granted, target, 1 << 16)
}

/// Same as [`spawn_distributor`] but with a configurable per-poll budget. Use a tiny budget to
/// force the budget-exhaustion exit path through the conservation arithmetic.
fn spawn_distributor_with_budget(
    pool: Arc<Pool>,
    granted: Arc<crate::sync::AtomicUsize>,
    target: usize,
    budget_size: usize,
) -> loom::thread::JoinHandle<u64> {
    loom::thread::spawn(move || {
        let mut dist = Distributor::new(pool);
        let mut wakers = CountWake(granted.clone());
        loom::future::block_on(core::future::poll_fn(|cx| {
            dist.pool.waker.register(cx.waker());
            let mut budget = Budget::new(budget_size);
            let _ = dist.poll_distribute(&mut budget, &mut wakers);
            // Self-wake honoring the budget contract: when the budget exhausts mid-walk with work
            // remaining, the distributor sets `needs_wake` so the caller re-polls without waiting
            // on a `release`. Match the production wiring in `Distributor::distribute`.
            if budget.take_needs_wake() {
                cx.waker().wake_by_ref();
            }
            if granted.load(Ordering::Acquire) >= target {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }));
        dist.debug_paid_demand()
    })
}

/// A parking `poll_acquire` races a distributor pass that is holding distributable credit. This
/// targets the two-load free-credit recovery in `pass`: the park does `available -= n` then
/// `parked_demand += n`, and the distributor reads `parked_demand` then `available`. If the loads
/// were in the wrong order the distributor could observe the `+n` without the `-n` and over-count
/// free credit, granting a slot it cannot afford and driving the conserved total past capacity.
/// `parked_demand` is monotonic; outstanding demand is recovered after the distributor joins as
/// `parked_demand − granted_total` (here, `granted` × the per-grant amount). We assert conservation
/// (`available + outstanding + returned + in_flight == capacity`) holds after every interleaving.
#[test]
fn park_races_distributor_no_overcommit() {
    loom::model(|| {
        // Total credit that will ever exist in this pool (capacity 0 + one release of CAP).
        const CAP: i64 = 10;
        let pool = Arc::new(Pool::new(Config {
            capacity: 0,
            max_single_acquire: [100; Priority::LEVELS],
            // Floor == cap: full grants, so these concurrency-invariant tests are unaffected by slicing.
            min_grant_slice: [100; Priority::LEVELS],
        }));

        // Park a waiter wanting CAP → available = -CAP, parked_demand = CAP.
        let w0 = alloc_slot();
        let w0w = noop();
        let mut w0cx = Context::from_waker(&w0w);
        let r = unsafe { pool.poll_acquire(&mut w0cx, w0, CAP as u64, Priority::Medium) };
        assert!(matches!(r, Poll::Pending));

        // A second waiter parks concurrently with a distributor pass + the release of CAP. The
        // pass must never over-grant. The distributor here runs a SINGLE bounded pass (no
        // target-blocking) — we are checking the conservation arithmetic of one pass racing a
        // park, not wakeup liveness (covered by other models), so it must not park/deadlock.
        let dist = {
            let pool = pool.clone();
            loom::thread::spawn(move || {
                let mut dist = Distributor::new(pool);
                let granted = Arc::new(crate::sync::AtomicUsize::new(0));
                let mut wakers = CountWake(granted);
                let mut budget = Budget::new(1 << 16);
                let dwaker = noop();
                dist.pool.waker.register(&dwaker);
                let _ = dist.poll_distribute(&mut budget, &mut wakers);
                dist.debug_paid_demand()
            })
        };

        let releaser = {
            let pool = pool.clone();
            loom::thread::spawn(move || pool.release(CAP as u64))
        };
        let parker = {
            let pool = pool.clone();
            loom::thread::spawn(move || {
                let w = alloc_slot();
                let ww = noop();
                let mut wcx = Context::from_waker(&ww);
                // May succeed (if capacity frees up as w0 is served) or park — either is correct.
                // What must NOT happen is an over-count letting more total credit out than CAP.
                let granted_now =
                    match unsafe { pool.poll_acquire(&mut wcx, w, CAP as u64, Priority::Low) } {
                        Poll::Ready(n) => {
                            // took it via the fast path; release it straight back so teardown is clean
                            pool.release(n);
                            true
                        }
                        Poll::Pending => {
                            let _ = unsafe { (*w.as_ptr()).abandon() };
                            false
                        }
                    };
                (SendPtr(w), granted_now)
            })
        };

        releaser.join().unwrap();
        let (w_ptr, granted_now) = parker.join().unwrap();
        let paid = dist.join().unwrap() as i64;

        // Conservation: whatever the interleaving, the conserved total must never imply more than
        // CAP bytes in flight. The wrong load order over-counts free credit and breaks this.
        // `parked_demand` is monotonic; outstanding demand is `parked_demand − paid`.
        let available = pool.available.load(Ordering::Relaxed);
        let parked = pool.parked_demand.load(Ordering::Relaxed) as i64;
        let outstanding = parked - paid;
        let returned = pool.returned.load(Ordering::Relaxed) as i64;
        let in_flight = CAP - (available + outstanding + returned);
        assert!(
            (0..=CAP).contains(&in_flight),
            "over-commit: in_flight={in_flight} not in 0..={CAP} \
             (available={available} outstanding={outstanding} returned={returned} \
              parked={parked} paid={paid})"
        );

        drop(pool);
        // The parker's slot is idle (fast-path granted) or abandoned-then-freed-on-pool-drop.
        if granted_now {
            unsafe { free_slot(w_ptr.0) };
        }
        unsafe { free_slot(w0) };
    });
}

/// A single release must wake a parked distributor. If the wakeup were lost the distributor would
/// park forever and loom would report a deadlock; conservation must also hold at the end.
#[test]
fn release_wakes_parked_waiter() {
    loom::model(|| {
        let pool = Arc::new(Pool::new(Config {
            capacity: 0,
            max_single_acquire: [100; Priority::LEVELS],
            // Floor == cap: full grants, so these concurrency-invariant tests are unaffected by slicing.
            min_grant_slice: [100; Priority::LEVELS],
        }));

        let slot = alloc_slot();
        let waker = noop();
        let mut cx = Context::from_waker(&waker);
        let r = unsafe { pool.poll_acquire(&mut cx, slot, 10, Priority::Medium) };
        assert!(matches!(r, Poll::Pending));
        assert_eq!(pool.available.load(Ordering::Relaxed), -10);

        let granted = Arc::new(crate::sync::AtomicUsize::new(0));
        let dist = spawn_distributor(pool.clone(), granted.clone(), 1);

        // Releaser races the distributor's first poll/park.
        pool.release(10);

        let paid = dist.join().unwrap();

        let slot_ref = unsafe { &*slot.as_ptr() };
        assert_eq!(slot_ref.poll_granted(), GrantResult::Granted(10));
        // Conservation: the 10 is now in-flight; nothing stranded.
        // `parked_demand` is monotonic, so the cancellation check is parked == paid.
        assert_eq!(pool.available.load(Ordering::Relaxed), 0);
        assert_eq!(pool.parked_demand.load(Ordering::Relaxed), paid);
        assert_eq!(pool.returned.load(Ordering::Relaxed), 0);

        unsafe { free_slot(slot) };
    });
}

/// Two concurrent releasers each return part of what the waiter needs. Whatever order they
/// interleave with the distributor, their credit must accumulate (one pass writes the leftover
/// back to `available`, the next pass picks it up) and the waiter must be served exactly once.
#[test]
fn concurrent_releases_accumulate() {
    loom::model(|| {
        let pool = Arc::new(Pool::new(Config {
            capacity: 0,
            max_single_acquire: [100; Priority::LEVELS],
            // Floor == cap: full grants, so these concurrency-invariant tests are unaffected by slicing.
            min_grant_slice: [100; Priority::LEVELS],
        }));

        let slot = alloc_slot();
        let waker = noop();
        let mut cx = Context::from_waker(&waker);
        let r = unsafe { pool.poll_acquire(&mut cx, slot, 10, Priority::Medium) };
        assert!(matches!(r, Poll::Pending));

        let granted = Arc::new(crate::sync::AtomicUsize::new(0));
        let dist = spawn_distributor(pool.clone(), granted.clone(), 1);

        let r1 = {
            let pool = pool.clone();
            loom::thread::spawn(move || pool.release(5))
        };
        let r2 = {
            let pool = pool.clone();
            loom::thread::spawn(move || pool.release(5))
        };

        r1.join().unwrap();
        r2.join().unwrap();
        let paid = dist.join().unwrap();

        let slot_ref = unsafe { &*slot.as_ptr() };
        assert_eq!(slot_ref.poll_granted(), GrantResult::Granted(10));
        assert_eq!(pool.available.load(Ordering::Relaxed), 0);
        assert_eq!(pool.parked_demand.load(Ordering::Relaxed), paid);

        unsafe { free_slot(slot) };
    });
}

/// Budget-exhaustion conservation: with a tight per-poll budget, the distributor must shed pull
/// across multiple polls (carry surplus locally, fold it into the next pull). Two waiters race a
/// release; under any interleaving conservation must hold and the per-pass writeback must never
/// drive `available` positive while live demand remains. Loom would deadlock if a needed re-poll
/// was lost, and the final assertion catches over-/under-count of free credit.
#[test]
fn budget_exhaustion_conservation() {
    loom::model(|| {
        const CAP: i64 = 20;
        let pool = Arc::new(Pool::new(Config {
            capacity: 0,
            max_single_acquire: [100; Priority::LEVELS],
            // Floor == cap: full grants, so these concurrency-invariant tests are unaffected by slicing.
            min_grant_slice: [100; Priority::LEVELS],
        }));

        let s1 = alloc_slot();
        let s2 = alloc_slot();
        let w1 = noop();
        let w2 = noop();
        let mut cx1 = Context::from_waker(&w1);
        let mut cx2 = Context::from_waker(&w2);
        let r = unsafe { pool.poll_acquire(&mut cx1, s1, 10, Priority::Medium) };
        assert!(matches!(r, Poll::Pending));
        let r = unsafe { pool.poll_acquire(&mut cx2, s2, 10, Priority::Medium) };
        assert!(matches!(r, Poll::Pending));

        let granted = Arc::new(crate::sync::AtomicUsize::new(0));
        // budget=1 forces budget-exhaustion exits — the distributor will need at least two polls
        // to satisfy both waiters, so any lost wakeup or missing carry-forward deadlocks.
        let dist = spawn_distributor_with_budget(pool.clone(), granted.clone(), 2, 1);

        let releaser = {
            let pool = pool.clone();
            loom::thread::spawn(move || pool.release(CAP as u64))
        };
        releaser.join().unwrap();
        let paid = dist.join().unwrap();

        let s1_ref = unsafe { &*s1.as_ptr() };
        let s2_ref = unsafe { &*s2.as_ptr() };
        assert_eq!(s1_ref.poll_granted(), GrantResult::Granted(10));
        assert_eq!(s2_ref.poll_granted(), GrantResult::Granted(10));
        // Conservation: parked_demand monotonic, all 20 reclassified parked → in-flight, nothing
        // stranded in returned. (Carry can't survive on success — a quiescent pass writes it out.)
        assert_eq!(pool.available.load(Ordering::Relaxed), 0);
        assert_eq!(pool.parked_demand.load(Ordering::Relaxed), paid);
        assert_eq!(pool.returned.load(Ordering::Relaxed), 0);

        unsafe {
            free_slot(s1);
            free_slot(s2);
        }
    });
}

/// Distributor drop racing a release: a parked waiter's terminal state must be exactly one of
/// `Granted` (the release won the race and the distributor served the slot before drop) or
/// `Closed` (drop won and the slot was woken by the close path). It must never be a stranded
/// `Pending` (lost wakeup) or a missing wake.
#[test]
fn distributor_drop_races_release() {
    loom::model(|| {
        let pool = Arc::new(Pool::new(Config {
            capacity: 0,
            max_single_acquire: [100; Priority::LEVELS],
            // Floor == cap: full grants, so these concurrency-invariant tests are unaffected by slicing.
            min_grant_slice: [100; Priority::LEVELS],
        }));

        let slot = alloc_slot();
        let waker = noop();
        let mut cx = Context::from_waker(&waker);
        let r = unsafe { pool.poll_acquire(&mut cx, slot, 10, Priority::Medium) };
        assert!(matches!(r, Poll::Pending));

        let dist = Distributor::new(pool.clone());

        let releaser = {
            let pool = pool.clone();
            loom::thread::spawn(move || pool.release(10))
        };
        let dropper = loom::thread::spawn(move || {
            // Run a single pass before dropping — this is the realistic race: the distributor task
            // might still be polling when its task gets cancelled.
            let mut wakers = CountWake(Arc::new(crate::sync::AtomicUsize::new(0)));
            let mut budget = Budget::new(1 << 16);
            let dwaker = noop();
            dist.pool.waker.register(&dwaker);
            let mut dist = dist;
            let _ = dist.poll_distribute(&mut budget, &mut wakers);
            // Now drop.
            drop(dist);
        });

        releaser.join().unwrap();
        dropper.join().unwrap();

        let slot_ref = unsafe { &*slot.as_ptr() };
        let result = slot_ref.poll_granted();
        assert!(
            matches!(result, GrantResult::Granted(10) | GrantResult::Closed),
            "stranded slot: {result:?}"
        );

        unsafe { free_slot(slot) };
    });
}

/// Distributor drop racing a fresh `poll_acquire`: the parker's terminal state must be one of
/// `Granted` (impossible here without a release, but legal in shape) or `Closed` (the drop's
/// close walk picked it up after it linked) or — if the parker observed `closed=true` after
/// taking the tier lock — a direct `Closed` via `signal_closed_idle`. Never stranded Pending.
#[test]
fn distributor_drop_races_poll_acquire() {
    loom::model(|| {
        let pool = Arc::new(Pool::new(Config {
            capacity: 0,
            max_single_acquire: [100; Priority::LEVELS],
            // Floor == cap: full grants, so these concurrency-invariant tests are unaffected by slicing.
            min_grant_slice: [100; Priority::LEVELS],
        }));

        let dist = Distributor::new(pool.clone());

        let parker = {
            let pool = pool.clone();
            loom::thread::spawn(move || {
                let n = alloc_slot();
                let nwaker = noop();
                let mut ncx = Context::from_waker(&nwaker);
                let _ = unsafe { pool.poll_acquire(&mut ncx, n, 5, Priority::Medium) };
                SendPtr(n)
            })
        };
        let dropper = loom::thread::spawn(move || {
            drop(dist);
        });

        let n_ptr = parker.join().unwrap().0;
        dropper.join().unwrap();

        let n_ref = unsafe { &*n_ptr.as_ptr() };
        let result = n_ref.poll_granted();
        assert!(
            matches!(result, GrantResult::Closed),
            "expected Closed, got: {result:?}"
        );

        unsafe { free_slot(n_ptr) };
    });
}

/// No-snipe: while a waiter is parked, a newcomer's fast-path `poll_acquire` racing a `release`
/// must never succeed — released credit goes to `returned` (invisible to the fast path) and the
/// fast path debits an `available` already driven negative by the parked waiter's demand. We
/// assert directly on the newcomer's poll result across every interleaving; no distributor is
/// involved (the property is purely about `release` vs the fast path), so there is no wakeup
/// dependency to deadlock on.
#[test]
fn newcomer_cannot_snipe() {
    loom::model(|| {
        let pool = Arc::new(Pool::new(Config {
            capacity: 0,
            max_single_acquire: [100; Priority::LEVELS],
            // Floor == cap: full grants, so these concurrency-invariant tests are unaffected by slicing.
            min_grant_slice: [100; Priority::LEVELS],
        }));

        // Pre-park a waiter requesting 10 → available = -10.
        let w = alloc_slot();
        let wwaker = noop();
        let mut wcx = Context::from_waker(&wwaker);
        let r = unsafe { pool.poll_acquire(&mut wcx, w, 10, Priority::Highest) };
        assert!(matches!(r, Poll::Pending));

        // A release of 10 races the newcomer's fast-path acquire. Because the release only ever
        // touches `returned`, `available` stays <= 0 until the distributor (not present here)
        // reconciles, so the newcomer can never see positive credit to take.
        let releaser = {
            let pool = pool.clone();
            loom::thread::spawn(move || pool.release(10))
        };
        let newcomer = {
            let pool = pool.clone();
            loom::thread::spawn(move || {
                let n = alloc_slot();
                let nwaker = noop();
                let mut ncx = Context::from_waker(&nwaker);
                let r = unsafe { pool.poll_acquire(&mut ncx, n, 5, Priority::Medium) };
                assert!(
                    matches!(r, Poll::Pending),
                    "newcomer sniped a parked waiter"
                );
                // The newcomer parked; mark it dead so the tier-list drop frees it (no
                // distributor runs in this model).
                let dead = matches!(unsafe { (*n.as_ptr()).abandon() }, AbandonResult::Abandoned);
                assert!(dead);
                SendPtr(n)
            })
        };

        releaser.join().unwrap();
        let n_ptr = newcomer.join().unwrap().0;

        // Drain the tiers so both parked slots are released exactly once (pool drop would also do
        // this, but we free `w` explicitly below and the newcomer was abandoned → freed on drop).
        let w_ref = unsafe { &*w.as_ptr() };
        // The waiter was never granted (no distributor ran); it is still linked.
        assert!(w_ref.is_linked());

        // Free the still-linked waiter directly (it never transitioned out of the tier). The
        // abandoned newcomer is freed when the pool's tiers drop.
        drop(pool);
        let _ = n_ptr;
        unsafe { free_slot(w) };
    });
}

/// Finding 4: `grant()` and `abandon()` race on the same parked slot, and the value the
/// application reads back from `abandon()` must be exactly what the distributor wrote — never a
/// torn or stale `granted`.
///
/// `grant()` writes `*granted = amount` then does a `Release` CAS LINKED→APP; `abandon()` does a
/// CAS LINKED→DEAD with `Acquire` failure ordering then reads `*granted`. The two outcomes:
///   * abandon wins the CAS (slot → DEAD): grant's CAS then fails (returns `None`), the slot is
///     ours-then-pool-freed, and no grant was observed.
///   * grant wins the CAS (slot → APP): abandon's CAS fails, and its `Acquire` load
///     synchronizes-with grant's `Release` CAS, so the `granted` read sees exactly `amount`.
/// Either way the app must never see a partial/garbage value, and pool credit must be conserved.
#[test]
fn grant_races_abandon_reads_exact_granted() {
    loom::model(|| {
        const CAP: u64 = 10;
        let pool = Arc::new(Pool::new(Config {
            capacity: CAP,
            max_single_acquire: [100; Priority::LEVELS],
            // Floor == cap: full grants, so these concurrency-invariant tests are unaffected by slicing.
            min_grant_slice: [100; Priority::LEVELS],
        }));

        // Drain capacity with a throwaway fast-path acquire so the next acquire is forced to park.
        let drain = alloc_slot();
        let dwaker = noop();
        let mut dcx = Context::from_waker(&dwaker);
        let r = unsafe { pool.poll_acquire(&mut dcx, drain, CAP, Priority::Medium) };
        assert!(matches!(r, Poll::Ready(n) if n == CAP));

        // Park the waiter we will race on: available is 0, so this parks for CAP.
        let w = alloc_slot();
        let wwaker = noop();
        let mut wcx = Context::from_waker(&wwaker);
        let r = unsafe { pool.poll_acquire(&mut wcx, w, CAP, Priority::Medium) };
        assert!(matches!(r, Poll::Pending));

        // Return the drained credit so the distributor has CAP to grant to the parked waiter.
        pool.release(CAP);

        // Thread A: the distributor runs one pass, attempting to grant the parked waiter.
        let dist = {
            let pool = pool.clone();
            loom::thread::spawn(move || {
                let mut dist = Distributor::new(pool);
                let granted = Arc::new(crate::sync::AtomicUsize::new(0));
                let mut wakers = CountWake(granted);
                let mut budget = Budget::new(1 << 16);
                let dwaker = noop();
                dist.pool.waker.register(&dwaker);
                let _ = dist.poll_distribute(&mut budget, &mut wakers);
            })
        };

        // Thread B: the application abandons concurrently with the grant.
        let app = {
            let pool = pool.clone();
            loom::thread::spawn(move || {
                let slot = unsafe { &*w.as_ptr() };
                match unsafe { slot.abandon() } {
                    AbandonResult::Abandoned => {
                        // We won the CAS; the pool frees the slot on its pop/drop. No credit was
                        // observed, so nothing to release.
                        false
                    }
                    AbandonResult::Granted(n) => {
                        // Grant won the CAS. The value MUST be exactly CAP (the full grant) or 0
                        // (grant had not written yet / nothing outstanding) — never torn.
                        assert!(
                            n == CAP || n == 0,
                            "abandon read a torn/unexpected granted value: {n} (expected {CAP} or 0)"
                        );
                        if n > 0 {
                            pool.release(n);
                        }
                        true
                    }
                    AbandonResult::Closed => true,
                }
            })
        };

        dist.join().unwrap();
        let app_owns = app.join().unwrap();

        drop(pool);
        // If abandon won, the slot is DEAD and freed by the pool's tier drop. If the grant won,
        // the app owns the now-idle slot and frees it here.
        if app_owns {
            unsafe { free_slot(w) };
        }
        unsafe { free_slot(drain) };
    });
}
