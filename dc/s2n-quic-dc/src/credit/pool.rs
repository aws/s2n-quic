// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A priority-aware shared byte-credit pool with a single-distributor reconciler.
//!
//! # Counter model
//!
//! Free credit is tracked with **two credit counters plus one monotonic demand counter**:
//!
//! - [`Pool::available`] (`AtomicI64`, init `capacity`) — the fast-path debit counter.
//!   [`Pool::poll_acquire`] does a lone `fetch_sub`. The distributor is the *only* actor that ever
//!   `fetch_add`s it.
//! - [`Pool::returned`] (`AtomicU64`) — the no-snipe staging buffer. [`Pool::release`] adds here; the
//!   fast path never reads it. The distributor swaps it to zero at the top of each pass. Routing
//!   returned credit here (instead of into `available`) is what prevents a brand-new acquirer from
//!   sniping credit a parked waiter has been waiting for.
//! - [`Pool::parked_demand`] (`AtomicU64`) — **monotonic** running total of every park's `requested`.
//!   `poll_acquire` adds on the park branch; nobody ever subtracts. The distributor maintains a
//!   private `paid_demand` accumulator (one `u64` field) and recovers outstanding demand as
//!   `parked_demand − paid_demand`. Making the shared atomic add-only halves its contention (one
//!   atomic op per park instead of one-per-park plus one-per-grant) and removes the producer/
//!   distributor RMW race on the same line.
//!
//! ## Invariants (verified by the loom tests)
//!
//! At every quiescent point, with `outstanding = parked_demand − paid_demand`:
//!
//! ```text
//! available + outstanding + returned + in_flight == capacity
//! ```
//!
//! and operationally `available <= 0` holds **whenever any waiter is parked** — so the fast path
//! (which needs `prev >= n > 0` to succeed) cannot acquire while waiters exist. That is the no-snipe
//! guarantee. `available` rises above zero only once the parked queue fully drains.
//!
//! # Distribution
//!
//! A single [`Distributor`] owns all distribution. It keeps a task-local mirror — one
//! [`List`](crate::intrusive::List) per priority — and refills an empty mirror by detaching the
//! shared tier under its lock (one O(1) splice). Unserved waiters stay in the mirror across passes
//! (never re-linked), so under a sustained backlog the tier mutex is taken only on refill. Grants
//! are **full** (`requested`), strict priority, FIFO within a tier, and the walk stops at the first
//! affordable-but-unaffordable live head (head-of-line blocking by design).
//!
//! # Lifetime contract
//!
//! The pool is owned by `Arc<Pool>`; producers and the [`Distributor`] both hold one. Dropping the
//! `Distributor` **closes the pool**: each tier mutex is taken to mark the tier closed and drain
//! its waiters (each one woken with [`crate::credit::slot::GrantResult::Closed`]), and any
//! subsequent `poll_acquire` short-circuits the park under the same mutex. This is the production
//! close path — the implicit `Arc<Pool>` drop only runs after the *last* outstanding stream
//! finishes, which is too late to recover from an unwanted distributor cancellation. See
//! [`Distributor`]'s docs for the full contract.

use super::{
    config::Config,
    counters::Counters,
    slot::{DeadSlot, DeadSlotQueue, Slot, SlotAdapter, SlotPtr},
    waker::TaskWaker,
};
use crate::{
    intrusive::List,
    socket::channel::Budget,
    sync::{lock, Arc, AtomicI64, AtomicU64, Mutex, Ordering},
    tracing::{debug, trace},
};
use core::task::{Context, Poll, Waker};
use crossbeam_utils::CachePadded;
use std::{collections::VecDeque, ptr::NonNull};

#[cfg(all(test, not(feature = "loom")))]
mod tests;

#[cfg(all(test, feature = "loom"))]
mod loom;

#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Priority {
    Highest = 0,
    High = 1,
    MediumHigh = 2,
    #[default]
    Medium = 3,
    MediumLow = 4,
    Low = 5,
    Lowest = 6,
    Background = 7,
}

impl Priority {
    pub const LEVELS: usize = 8;

    pub const ALL: [Self; Self::LEVELS] = [
        Self::Highest,
        Self::High,
        Self::MediumHigh,
        Self::Medium,
        Self::MediumLow,
        Self::Low,
        Self::Lowest,
        Self::Background,
    ];

    /// Decode a single byte into a `Priority`. Values 0..=7 map; otherwise `None`.
    #[inline]
    pub fn from_u8(byte: u8) -> Option<Self> {
        match byte {
            0 => Some(Self::Highest),
            1 => Some(Self::High),
            2 => Some(Self::MediumHigh),
            3 => Some(Self::Medium),
            4 => Some(Self::MediumLow),
            5 => Some(Self::Low),
            6 => Some(Self::Lowest),
            7 => Some(Self::Background),
            _ => None,
        }
    }

    #[inline]
    pub const fn as_u8(self) -> u8 {
        self as u8
    }
}

pub struct Pool {
    /// Fast-path debit counter. `poll_acquire` subtracts; the distributor is the sole adder.
    /// Padded to keep producer fast-path RMWs off the same line as `returned`/`parked_demand`.
    available: CachePadded<AtomicI64>,
    /// No-snipe staging buffer for returned credit. The fast path never reads this.
    /// Padded — releasers and the distributor's `swap` would otherwise share a line with
    /// `available`'s hot fast-path traffic.
    returned: CachePadded<AtomicU64>,
    /// Monotonic running total of `requested` over every park. Producers `fetch_add`; the
    /// distributor only loads. Outstanding demand is recovered as `parked_demand − paid_demand`
    /// (a Distributor-private `u64`), so this atomic carries no decrements.
    parked_demand: CachePadded<AtomicU64>,
    config: Config,
    waker: TaskWaker,
    /// One wait list per priority. Held under a mutex; the distributor briefly locks each tier
    /// only to refill an empty mirror. Each tier is cache-padded so that producers parking on
    /// different priorities don't share a line — without padding, a producer on tier 3
    /// invalidates the distributor's cached read of tier 4 on every walk.
    ///
    /// The per-tier `closed` flag rides under the same mutex as the list. [`Distributor::drop`]
    /// sets it while draining each tier; concurrent [`Pool::poll_acquire`] callers observe it
    /// when they take the tier lock to push and short-circuit the park, signalling closure on
    /// the slot directly. Folding `closed` into the existing critical section avoids a separate
    /// atomic load on the slow path — producers already pay the lock acquire to push.
    ///
    /// `release` does NOT check closure: it only touches `returned` (a benign atomic that no
    /// one will ever read on a closed pool) and the distributor's waker (a no-op load when the
    /// distributor is gone). Skipping the check there saves an atomic on the hot release path.
    tiers: [CachePadded<Mutex<Tier>>; Priority::LEVELS],
    /// Observability handles. Default is a no-op `Registry` (every emit is dropped on the floor).
    /// Wire in real counters via [`Pool::with_counters`].
    counters: Counters,
}

/// A single priority tier: the wait list plus a closed flag, both protected by the same mutex.
struct Tier {
    list: List<SlotAdapter>,
    closed: bool,
}

impl Tier {
    fn new() -> Self {
        Self {
            list: List::new(),
            closed: false,
        }
    }
}

impl Pool {
    pub fn new(config: Config) -> Self {
        Self::with_counters(config, Counters::default())
    }

    /// Construct a pool with externally-registered observability counters.
    pub fn with_counters(config: Config, counters: Counters) -> Self {
        let config = config.normalized();

        Self {
            available: CachePadded::new(AtomicI64::new(config.capacity as i64)),
            returned: CachePadded::new(AtomicU64::new(0)),
            parked_demand: CachePadded::new(AtomicU64::new(0)),
            config,
            waker: TaskWaker::new(),
            tiers: std::array::from_fn(|_| CachePadded::new(Mutex::new(Tier::new()))),
            counters,
        }
    }

    /// Acquire `n` bytes, parking the slot if the pool cannot satisfy it immediately.
    ///
    /// The fast path is a single `fetch_sub`. If the previous value was sufficient, the subtraction
    /// holds and the acquire succeeds. Otherwise the subtraction is **left in place** — the parked
    /// slot is the record of that demand — the slot is linked into its priority tier, and
    /// `parked_demand` is bumped (monotonic, never decremented). The distributor will deliver a
    /// full grant and wake the slot once enough credit is returned.
    ///
    /// # Safety
    ///
    /// The provided `slot` pointer must be valid and the slot must be idle (refcount=1). It must
    /// remain valid until either the grant is delivered (refcount transitions back to 1) or the slot
    /// is abandoned.
    pub unsafe fn poll_acquire(
        &self,
        cx: &mut Context<'_>,
        slot: NonNull<Slot>,
        n: u64,
        priority: Priority,
    ) -> Poll<u64> {
        let n = self.config.clamp_request(n, priority);
        if n == 0 {
            return Poll::Ready(0);
        }

        // Fast path: a single debit. Success if the pool had the credit.
        //
        // Acquire pairs with the distributor's writeback at end-of-pass so a successful debit
        // observes returned credit promptly. The park branch publishes via
        // `parked_demand.fetch_add(Release)` below, so no Release is needed here.
        let prev = self.available.fetch_sub(n as i64, Ordering::Acquire);
        if prev >= n as i64 {
            self.counters.acquire_fast_path.add(1);
            self.counters.acquire_bytes.add(n);
            return Poll::Ready(n);
        }

        // Slow path: the subtraction stays in `available` (it now represents this waiter's unmet
        // demand). No refund and no distributor wake — a park adds demand, not credit.
        //
        // Ordering: the `fetch_sub` above (the `-n` to `available`) is sequenced before the
        // `fetch_add` below (the `+n` to `parked_demand`). The distributor relies on this order,
        // reading `parked_demand` before `available`, so it can never observe the `+n` without the
        // `-n` and thus never over-counts free credit. See `Distributor::pass`.
        let slot_ref = &*slot.as_ptr();
        slot_ref.prepare_park(n, cx.waker());

        let mut tier = lock(&self.tiers[priority as usize]);
        // Once the distributor has marked this tier closed, the drain walk has either run or is
        // running and there is no agent to deliver credit. Skip the park, signal closure on the
        // slot directly, and wake the caller so its next poll observes `GrantResult::Closed`.
        // The mutex itself synchronizes us with `Distributor::drop`: either the drain swapped
        // the list and set `closed` before we got the lock (we see `closed=true` and bail), or
        // we got the lock first and pushed (the drain will pick up our slot under the same lock).
        if tier.closed {
            slot_ref.cancel_park();
            // Refund the speculative debit; the slot never linked, so this credit is "as if" the
            // poll had never happened.
            self.available.fetch_add(n as i64, Ordering::Release);
            slot_ref.signal_closed_idle();
            trace!(target: "credit::pool", n, "poll_acquire on closed pool");
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        self.parked_demand.fetch_add(n, Ordering::Release);
        tier.list.push_back(SlotPtr::new(slot));
        slot_ref.transition_to_linked();

        self.counters.acquire_parked[priority as usize].add(1);
        // The bytes counter is incremented at park time (not grant time) so the metric reflects
        // *demand*, not *delivered* throughput; pair with `credit.distributor.granted` to
        // distinguish backlog from in-flight.
        self.counters.acquire_bytes.add(n);

        Poll::Pending
    }

    /// Return `n` bytes of credit to the pool and wake the distributor.
    ///
    /// Returned credit is staged in `returned`, where the fast path cannot see it, so a concurrent
    /// acquirer cannot snipe ahead of a parked waiter. The distributor pulls `returned` at the top
    /// of its next pass.
    ///
    /// `release` does not check whether the pool is closed: the only side effects are the
    /// `returned` add (a benign atomic that goes nowhere if the distributor is gone) and the
    /// `waker.wake()` call (a no-op load when the registered waker has already been dropped). A
    /// closed-pool check here would add an extra atomic load on the hot release path — which the
    /// integration plan calls per `bytes_in_flight` decrement — for no behavioural benefit.
    pub fn release(&self, n: u64) {
        if n == 0 {
            return;
        }
        self.returned.fetch_add(n, Ordering::Release);
        self.counters.release_bytes.add(n);
        self.counters.release_calls.add(1);
        self.waker.wake();
    }

    /// Per-request acquire ceiling for `priority` (the normalized `max_single_acquire`). Callers
    /// use this to bound speculative sizing — e.g. the reader caps its window-growth ratio so a
    /// grown window can never exceed what a single acquire could satisfy.
    #[inline]
    pub fn max_single_acquire(&self, priority: Priority) -> u64 {
        self.config.max_single_acquire[priority as usize]
    }

    // Used by the deterministic suite (compiled out under the loom feature, hence allow(dead_code)).
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn debug_available(&self) -> i64 {
        self.available.load(Ordering::Relaxed)
    }

    /// Staged-but-not-yet-reconciled returned credit. With no parked waiters the pool conserves
    /// `available + returned == capacity`; a recv-credit leak shows up as this sum falling short.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn debug_returned(&self) -> u64 {
        self.returned.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn debug_parked_demand(&self) -> u64 {
        self.parked_demand.load(Ordering::Relaxed)
    }
}

/// The single owner of all credit distribution.
///
/// Holds the task-local mirror (one list per priority) and an [`Arc`] to the shared [`Pool`]. Run
/// it as a task with [`Distributor::distribute`]; the future registers the distributor's waker on
/// the pool once and yields cooperatively under a budget.
///
/// # Lifetime contract
///
/// Dropping the [`Distributor`] **closes the [`Pool`]**: each tier mutex is taken to mark the
/// tier closed and drain its waiters (each woken with [`GrantResult::Closed`]); the distributor's
/// local mirror is closed implicitly when the field drops. Subsequent calls to [`Pool::release`]
/// remain functional but go nowhere — `returned` accumulates uselessly and the dead waker is a
/// no-op load — so producers don't need a closed check on the hot release path. Subsequent calls
/// to [`Pool::poll_acquire`] short-circuit the park under the same per-tier mutex used by the
/// drain: the slot's `granted` field is stamped with `GRANT_CLOSED` directly and the caller's
/// waker fires so the next poll observes [`GrantResult::Closed`].
///
/// This is the production close path. With every stream in the integration plan holding
/// `Arc<Pool>`, only [`Distributor::drop`] can guarantee the close timing — the implicit
/// `Arc<Pool>` drop happens after the *last* stream finishes, which is too late for an unwanted
/// distributor cancellation.
/// Receiver of the distributor's per-poll waker batch.
///
/// The distributor accumulates `Waker`s into a `VecDeque<Waker>` it owns across the
/// poll cycle and hands the batch to the sink at end-of-poll via [`append_wakers`].
/// The sink takes whatever delivery decision is appropriate for it (fan out under a
/// lock, fire inline, route to a drain task) and returns. The distributor's
/// `VecDeque` is left empty and reused on the next poll.
///
/// **Wake guarantee.** A granted slot has already had its `granted` field mutated
/// — the parked task MUST be woken. The sink contract is therefore total: every
/// `Waker` placed into `batch` MUST be either delivered (queued for a drain task,
/// fanned out, etc.) or invoked inline before `append_wakers` returns. The
/// distributor never inspects the `VecDeque` after the call (it only `clear()`s
/// for reuse), so a sink that leaves wakers in the batch without invoking them
/// strands granted slots.
///
/// [`append_wakers`]: WakerSink::append_wakers
pub trait WakerSink {
    /// Drain `batch` and deliver each waker. The distributor calls this once per
    /// poll cycle when at least one grant happened. After the call, `batch` should
    /// be empty so the distributor can reuse its allocation; sink implementations
    /// that cannot deliver immediately must arrange for delivery internally and
    /// still empty the input (e.g. by `append`-ing into a downstream buffer).
    fn append_wakers(&mut self, batch: &mut VecDeque<Waker>);
}

pub struct Distributor {
    pool: Arc<Pool>,
    /// One mirror list per priority. Each shares its shared tier's list id (seeded by `detach`).
    local: [List<SlotAdapter>; Priority::LEVELS],
    /// Cumulative `requested` for every park we have already granted or reaped. Outstanding demand
    /// is `pool.parked_demand − paid_demand`. Lives here so the shared atomic stays add-only —
    /// producers never contend with the distributor on the same line for a decrement.
    paid_demand: u64,
    /// Pull bytes the previous pass could not safely write back without driving `available`
    /// positive while live parked waiters still demand credit. Folded into `pull` at the top of the
    /// next pass. Holding the surplus distributor-locally (instead of staging it back through
    /// `returned`) keeps the shared `returned` line a single producer/single consumer atomic.
    carry: u64,
    /// Per-poll waker scratch buffer. Owned by the distributor so its capacity persists
    /// across polls — `VecDeque` only re-allocates when a poll grants more wakers than any
    /// previous poll. The buffer is always empty when entering and leaving `poll_distribute`:
    /// `pass` appends grants; the sink drains them via [`WakerSink::append_wakers`] before
    /// the function returns.
    pending_wakers: VecDeque<Waker>,
}

impl Distributor {
    pub fn new(pool: Arc<Pool>) -> Self {
        debug!(target: "credit::pool", "distributor created");
        Self {
            pool,
            local: std::array::from_fn(|_| List::new()),
            paid_demand: 0,
            carry: 0,
            pending_wakers: VecDeque::new(),
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn debug_paid_demand(&self) -> u64 {
        self.paid_demand
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn debug_carry(&self) -> u64 {
        self.carry
    }

    /// Drive the distributor as an async task.
    ///
    /// `budget` bounds the work per poll; the future yields when the budget is exhausted and
    /// re-arms via the standard `take_needs_wake` self-wake. `wakers_tx` receives the batch of
    /// wakers granted during each poll cycle via [`WakerSink::append_wakers`] — empty cycles
    /// do not call the sink. The future never resolves; drop the task to shut down.
    ///
    /// A grant has already mutated the slot's `granted` field and cannot be retracted, so the
    /// sink MUST either queue or invoke every waker handed to it before returning. See
    /// [`WakerSink`] for the contract.
    pub async fn distribute<W>(mut self, mut budget: Budget, mut wakers_tx: W)
    where
        W: WakerSink,
    {
        // Register exactly once on the first poll. Subsequent polls keep the registered waker —
        // the steady-state loop below never touches it.
        core::future::poll_fn(|cx| {
            self.pool.waker.register(cx.waker());
            Poll::Ready(())
        })
        .await;

        core::future::poll_fn(move |cx| {
            budget.reset();
            let _ = self.poll_distribute(&mut budget, &mut wakers_tx);
            // poll_distribute always returns Pending (it loops until quiescent); honor the
            // standard budget-exhausted self-wake.
            if budget.take_needs_wake() {
                cx.waker().wake_by_ref();
            }
            Poll::Pending
        })
        .await
    }

    /// Run bounded distribution passes until quiescent or the budget is exhausted, then flush any
    /// accumulated waker batch.
    ///
    /// Loops passes while each makes progress and budget remains. When a pass makes no progress,
    /// returns `Poll::Pending`; when budget runs out with work remaining, sets `needs_wake` so the
    /// outer drain re-polls. Work per poll is bounded by the budget, regardless of backlog size or
    /// `release`-during-pass churn. The caller is responsible for registering the distributor's
    /// waker on the pool (see [`Distributor::distribute`] for the standard wiring) before the first
    /// poll, otherwise releases will not drive a re-poll.
    ///
    /// On every `Pending` exit, the accumulated waker batch is shipped to `wakers_tx` as a
    /// single [`WakerSink::append_wakers`] call (skipped when empty). Dead slots are freed
    /// inline inside `pass`, outside any tier lock.
    pub(crate) fn poll_distribute<W>(&mut self, budget: &mut Budget, wakers_tx: &mut W) -> Poll<()>
    where
        W: WakerSink,
    {
        // Wakers accumulate during the pass and are flushed at the end of the poll cycle:
        // a `VecDeque::push_back` per grant is cheap, but invoking the sink (which typically
        // takes a downstream lock) per grant would stall the inner loop. `self.pending_wakers`
        // is a long-lived scratch buffer — its allocation persists across polls so steady-state
        // grant batches don't allocate. Empty at function entry; emptied by the sink before exit.
        debug_assert!(
            self.pending_wakers.is_empty(),
            "pending_wakers leaked across polls"
        );
        let mut dead = DeadSlotQueue::new();
        let result = loop {
            let progressed = self.pass(budget, &mut dead);
            if budget.is_exhausted() {
                budget.set_needs_wake();
                break Poll::Pending;
            }
            if !progressed {
                break Poll::Pending;
            }
        };

        if !self.pending_wakers.is_empty() {
            // The sink contract is total: every waker in the buffer is either queued or fired
            // inline before this returns, and the buffer is left empty so we can reuse it next
            // poll without re-allocating.
            wakers_tx.append_wakers(&mut self.pending_wakers);
            debug_assert!(
                self.pending_wakers.is_empty(),
                "WakerSink::append_wakers must drain the batch (its capacity is reused next poll)"
            );
        }
        // `dead` drops here at the end of the poll cycle — its drop walks the list and runs each
        // slot's `drop_fn`, freeing the outer allocations outside the work loop.
        drop(dead);

        result
    }

    /// A single distribution pass. Returns whether it made progress (granted or reaped anything,
    /// or had pulled credit — including carried credit from a previous pass). Stops early if the
    /// budget is exhausted. Granted slots' wakers are pushed into `pending_wakers`; dead slots are
    /// pushed into `dead` and freed when that queue drops at the end of the poll cycle (their
    /// drop_fn must run outside any tier lock).
    ///
    /// No-snipe across all exits: the end-of-pass writeback caps the pull portion at the
    /// after-grant live parked demand. On the unaffordability exit this is a no-op (the cap is
    /// never tighter than the natural bound). On the budget-exhaustion exit, where affordable
    /// waiters can still be linked, the cap prevents `available` from going positive while owed
    /// credit still has parkers; the surplus carries to the next pass via `self.carry`.
    fn pass(&mut self, budget: &mut Budget, dead: &mut DeadSlotQueue) -> bool {
        self.pool.counters.distributor_passes.add(1);
        // Recover true free credit `= capacity − in_flight = available + outstanding_demand`,
        // plus the newly-returned `pull`. `available` is held negative by every parked waiter's
        // subtraction; `outstanding_demand` cancels exactly that.
        //
        // `parked_demand` is monotonic (producers `fetch_add`, nobody subtracts). The distributor
        // tracks every grant/reap in its private `paid_demand`, so true outstanding demand is
        // `parked_demand − paid_demand`. Equivalent to the old `parked_demand` semantics; the only
        // change is that the subtraction lives in this thread's local instead of in a shared RMW.
        //
        // Load order matters: `poll_acquire`'s park does `available -= n` and THEN
        // `parked_demand += n` (two non-atomic steps). We read `parked_demand` FIRST, then
        // `available`, so a park landing between our two loads can only be observed as the `-n` in
        // `available` without the matching `+n` in `parked_demand` — i.e. `free` is *under*-counted
        // (conservative, that waiter is simply served next pass). The reverse order could
        // *over*-count by `n` and grant a waiter we can't afford (over-commit past capacity).
        // Acquire pairs with `release()`'s Release-add. The stored `0` has no acquiring reader.
        //
        // Fold any `carry` surplus from the previous pass back into `pull`. Carry exists when the
        // previous pass could not write the full pulled credit back to `available` without driving
        // it positive past live parked demand (which would let a fresh fast-path acquirer snipe).
        let pull = self.pool.returned.swap(0, Ordering::Acquire) + self.carry;
        self.carry = 0;
        let outstanding =
            (self.pool.parked_demand.load(Ordering::Acquire) - self.paid_demand) as i64;
        let mut free = self.pool.available.load(Ordering::Acquire) + outstanding + pull as i64;

        // Avail-debits the walk reclaimed this pass that MUST write back to `available` in full (in
        // contrast to `pull`, whose writeback is capped to preserve no-snipe). Three sources, all
        // balancing a park-time `available -= req`: a reaped dead slot's `req`, an abandon-race
        // slot's `req`, and the un-granted remainder of a partial grant (`req - grant_amount`). Kept
        // separate from `pull` precisely because it is unconditional; see the end-of-pass writeback.
        let mut reclaimed_avail: u64 = 0;
        let mut granted_any = false;

        'walk: for ((local, tier), &min_slice) in self
            .local
            .iter_mut()
            .zip(&self.pool.tiers)
            .zip(&self.pool.config.min_grant_slice)
        {
            // Pull the shared tier into the mirror up-front, before computing the slice. This both
            // refills a drained mirror and merges fresh arrivals that landed behind cached leftovers
            // — the merge is load-bearing for fairness: the slice below is `free / local.len()`, so
            // if a few cached heads shadowed a large shared-tier backlog the split would degenerate
            // toward a near-full grant for those heads and starve the hidden waiters. `append`
            // preserves FIFO (cached stay at the front, arrivals go to the back). One uncontended
            // tier-lock per pass per tier — cheap, and the slice now always reflects the true
            // backlog. Skip the per-pass overhead for empty tiers.
            {
                let mut shared = lock(tier);
                local.append(&mut shared.list);
            }
            if local.is_empty() {
                continue;
            }

            // Demand-elastic fair share, computed ONCE up-front for the whole tier.
            // Granting each head its full `req` lets the first few waiters drain the pool at
            // `max_single_acquire` and starve the rest behind the affordability `break`. Instead,
            // hand each waiter an equal slice and round-robin: a granted head leaves the queue, its
            // caller consumes and re-acquires, re-parking at the tail. Every waiter is serviced
            // within O(parked) grants — FIFO, not racy — and aggregate bandwidth stays saturated as
            // long as the slice covers a few frames.
            //
            // Computed once (not per item as `free` shrinks): a single up-front split gives every
            // waiter in the tier the same slice; recomputing would skew larger slices toward the
            // tail of the queue. Floored at the configured `min_grant_slice[priority]` so heavy
            // contention can't shrink it to sub-frame slivers — we'd rather serve fewer waiters per
            // pass with a usable slice than dribble a few KB to everyone. With no contention
            // (`parked == 1`) the slice is the full `free`, so a lone waiter still gets a full grant.
            // `min_slice` is normalized to `<= max_single_acquire[p]` and `<= capacity`, so the
            // floor can never demand more than the pool could ever free.
            //
            // KNOWN LIMITATION (skewed demand): with a fixed slice, waiters wanting less than the
            // slice leave their unused portion in `free`, which the larger waiters cannot pick up
            // this pass (they are capped at `slice`). The leftover writes back and is granted over
            // subsequent passes, so this costs a little latency under heavily mixed demand, never
            // throughput or correctness. A single-pass-optimal allocation would need a max-min /
            // water-filling split (e.g. per-tier demand-bucket counts), deferred until measured.
            let slice = if free > 0 {
                (free as u64 / local.len() as u64).max(min_slice)
            } else {
                min_slice
            };

            loop {
                if local.is_empty() {
                    break;
                }

                // Grant heads while affordable. Reap dead heads unconditionally (a dead corpse at
                // the head must not block live waiters behind it). Stop the whole walk at the first
                // live head we cannot afford — strict priority means lower tiers wait too.
                //
                // SAFETY: the slot is in our mirror, so it is linked (not idle); `requested` was set
                // at park time under the tier lock and is stable until we grant or reap it here.
                let front = local.front().unwrap();
                let req = unsafe { front.requested() };

                if front.is_dead() {
                    let ptr = local.pop_front().unwrap().take();
                    self.paid_demand += req;
                    reclaimed_avail += req;
                    free += req as i64;
                    self.pool.counters.distributor_reaped.add(1);
                    // SAFETY: a dead slot (refcount=0) has been popped from every list and is owned
                    // by us; wrapping it in `DeadSlot` transfers ownership to the dead queue, which
                    // runs the drop_fn at end-of-poll outside the work loop.
                    dead.push_back(unsafe { DeadSlot::new(ptr) });
                    continue;
                }

                if free <= 0 {
                    break 'walk;
                }

                let grant_amount = req.min(slice);

                // If we can't cover this head's slice right now, stop and let releases refill `free`.
                // A head wanting at least the slice must get the whole slice (no sub-slice dribble);
                // a head wanting less than the slice only needs its (smaller) `req`. Either way, bail
                // when `free` can't cover what this head should receive — the next pass retries once
                // more credit has been returned. This is what keeps grants at >= min_slice instead
                // of handing out whatever scrap of `free` happens to exist this instant.
                if (grant_amount as i64) > free {
                    break 'walk;
                }

                if !budget.consume() {
                    self.pool.counters.distributor_budget_exhausted.add(1);
                    break 'walk;
                }

                let ptr = local.pop_front().unwrap().take();
                // SAFETY: `ptr` was just popped from our mirror, so it is linked and exclusively
                // ours for the duration of this grant; we hold no tier lock (grant must not run
                // under one) but the slot cannot be concurrently freed — only the app's `abandon`
                // (refcount CAS) and our `grant` race, and `grant` resolves that race.
                unsafe {
                    let slot = &*ptr.as_ptr();
                    let res = slot.grant(grant_amount);

                    // Whether granted or raced, this park is now resolved — credit the FULL `req`
                    // locally so the next pass's `outstanding` reflects that the slot has left the
                    // parked list. The un-granted remainder (`req - grant_amount`) is refunded
                    // below; the slot's caller re-acquires for the rest.
                    self.paid_demand += req;

                    match res {
                        Some(waker) => {
                            // Partial (or full) grant: only `grant_amount` becomes in-flight; the
                            // rest of the park-time `available -= req` debit must return to
                            // `available`. That refund is the same kind of reclaimed avail-debit as
                            // a dead-slot reap, so route it through `reclaimed_avail` (which always
                            // writes back at end of pass). The waiter's park-time subtraction stays
                            // in `available`; we reclassify `grant_amount` parked → in-flight and
                            // hand the remainder back. Stash the waker locally; the batch ships at
                            // end of the poll cycle via `WakerSink::append_wakers`.
                            free -= grant_amount as i64;
                            let refund = req - grant_amount;
                            if refund > 0 {
                                reclaimed_avail += refund;
                            }
                            granted_any = true;
                            self.pool.counters.distributor_granted.add(1);
                            trace!(target: "credit::pool", req, grant_amount, "grant");
                            self.pending_wakers.push_back(waker);
                        }
                        None => {
                            // Raced: the app abandoned between our `is_dead` check and the CAS.
                            // `grant` returning None means refcount=0 and the slot is ours to free.
                            // Push into the dead queue; its drop at end-of-poll frees the slot.
                            reclaimed_avail += req;
                            free += req as i64;
                            self.pool.counters.abandon_granted_race.add(1);
                            trace!(target: "credit::pool", req, "abandon_granted_race");
                            dead.push_back(DeadSlot::new(ptr));
                        }
                    }
                }
            }
        }

        // End-of-pass writeback. We only ever apply a *delta* to `available` via `fetch_add`: the
        // fast path (`poll_acquire`) mutates `available` concurrently, so the distributor can never
        // store an absolute (e.g. `free`) without clobbering in-flight acquires. The delta re-
        // entering `available` is exactly the credit that left it or was never granted:
        //   * `pull` — bytes pulled from `returned` (plus any carried-over from a prior pass).
        //   * `reclaimed_avail` — park-time `available -= req` debits the walk reclaimed this pass
        //     (dead-slot reaps, abandon races, partial-grant refunds), which must return in full.
        // (`free` itself is absolute — it bakes in `available_start + outstanding`, already present
        // in `available` — so adding it would double-count; that's why we track the delta instead.)
        //
        // If we wrote back the full total, two pass exit paths are safe and one is not:
        //
        //   * The unaffordability exit (`req > free` for a live head) leaves the walk's outstanding
        //     demand unreduced relative to the credit we pulled, so `available + writeback <= 0`
        //     holds naturally and no-snipe is preserved.
        //   * Quiescent exits (all live heads granted) drain `live_parked` to zero; nobody can be
        //     sniped because nobody is parked.
        //   * The budget-exhaustion exit can leave affordable waiters still linked. Writing the
        //     full pull then drives `available` positive while owed credit still has parkers — a
        //     fresh fast-path acquirer would snipe credit destined for those parkers. To preserve
        //     no-snipe across this exit, we cap the writeback by the live remaining demand and
        //     hold the surplus in `self.carry` for the next pass to fold back into `pull`.
        //
        // `reclaimed_avail` always writes back (those debits are real reclaimed credit, not pull).
        // The pull portion is capped at `live_parked` so `available` never overruns after-grant
        // outstanding demand.
        let live_parked = self
            .pool
            .parked_demand
            .load(Ordering::Acquire)
            .saturating_sub(self.paid_demand);
        let total = pull + reclaimed_avail;
        let writeback = if live_parked == 0 {
            total
        } else {
            total.min(live_parked + reclaimed_avail)
        };
        self.carry = total - writeback;
        self.pool
            .counters
            .distributor_carry_bytes
            .set(self.carry as i64);
        if writeback > 0 {
            self.pool
                .available
                .fetch_add(writeback as i64, Ordering::Release);
        }

        // Forward progress: `pull > 0` covers the "carried credit waiting to be written back"
        // case, since carry is folded into `pull` at the top of every pass.
        granted_any || reclaimed_avail > 0 || pull > 0
    }
}

impl Drop for Distributor {
    fn drop(&mut self) {
        debug!(target: "credit::pool", "distributor dropping; closing pool");
        // Drain each shared tier under its lock, marking the tier `closed` in the same critical
        // section. Concurrent `poll_acquire` callers serialize on the tier mutex: they either
        // observe `closed=false` and push (we'll pick up their slot here), or they observe
        // `closed=true` after we've already drained and short-circuit on the slot directly.
        // No separate atomic flag — the lock itself is the synchronization point.
        //
        // Drop the drained list OUTSIDE the lock: `SlotPtr::drop` performs the LINKED→APP CAS,
        // writes `GRANT_CLOSED`, and wakes the parked task; that wake can be arbitrarily slow
        // (runtime-dependent) and would otherwise stall a hot critical section.
        for tier in self.pool.tiers.iter() {
            let mut drained: List<SlotAdapter> = List::new();
            {
                let mut shared = lock(tier);
                shared.closed = true;
                core::mem::swap(&mut drained, &mut shared.list);
            }
            drop(drained);
        }

        // The local mirror drops automatically with the field. Each entry there is also a
        // `SlotPtr`, so its drop closes the slot just as the shared-tier walk above does.
    }
}
