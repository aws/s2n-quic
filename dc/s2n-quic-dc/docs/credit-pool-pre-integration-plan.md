# Credit Pool — Pre-Integration Plan

This document tracks the work that must land on the credit-pool subsystem
(`src/credit/`) before it is wired into the stream send/receive paths. The
plan came out of a multi-lens code review of the current implementation; the
two real correctness bugs and several integration-blocking gaps below are
what survived adversarial verification.

The phases are ordered so each gate verifies the next. Each phase is a
self-contained unit of work suitable for delegation.

## Background

The credit pool is the endpoint-wide flow-control primitive described in
[endpoint-flow-control.md](endpoint-flow-control.md). The integration plan
is: every stream gets an `Arc<Pool>` handle and calls `poll_acquire` with
its priority before queueing send data; the packet layer calls `release` as
bytes leave `bytes_in_flight`; one `Distributor` task per endpoint delegates
credits FIFO-within-priority across all streams. The current code in
`src/credit/` implements that primitive; this plan is the punch list to
make it ready for that wiring.

The full review (71 raw findings, 56 confirmed after adversarial
verification) lives in the conversation history. This document captures
only the items we agreed to act on.

---

## Phase 0 — Restore loom build

**Why:** `cargo build -p s2n-quic-dc --features loom --tests` currently
fails with E0599 because `pool/loom.rs` calls a non-existent
`Distributor::register_waker`. As a result, every loom model in the credit
suite is dead code — none of the no-snipe / no-overcommit / lost-wakeup /
accumulate-on-release properties documented in `pool.rs` are actually
verified in CI today. Every subsequent phase wants to add or extend loom
models, so this gate must lift first.

**Constraint:** the original review recommended adding
`pub(crate) fn register_waker` on `Distributor`, but that exposes
distributor-task plumbing as a real API. The waker registration should
remain an implementation detail of the `distribute()` future. Inline the
registration in the loom test bodies (or add a `#[doc(hidden)]` test-only
helper on `Pool`) so production callers cannot see it.

**Tasks:**
- Replace the two `dist.register_waker(cx)` call sites in
  `src/credit/pool/loom.rs` (around lines 92 and 143) with direct
  registration via `dist.pool.waker.register(cx.waker())`, or add a
  `#[cfg(test)] #[doc(hidden)]` shim on `Pool` if that is cleaner.
- Remove the broken intra-doc link `[register_waker](Self::register_waker)`
  in the doc-comment on `poll_distribute` in `src/credit/pool.rs` (around
  line 268). Replace with prose pointing at `Distributor::distribute`.
- Verify with
  `LOOM_MAX_PREEMPTIONS=3 cargo test --profile release-debug --features loom credit::pool::loom`.
  All four existing models should run and pass.

**Done when:** the loom suite builds and passes locally on
`LOOM_MAX_PREEMPTIONS=3`. CI should be checked too if the project runs
loom under CI (it's worth confirming during this phase).

---

## Phase 1 — Budget-exhaustion no-snipe fix

**Why:** there is a real correctness bug in
`Distributor::pass`. The walk has two exit paths: unaffordability
(`req as i64 > free` at `pool.rs:383-385`) and budget exhaustion
(`!budget.consume()` at `pool.rs:387-389`). The unconditional writeback at
`pool.rs:429-434` adds `pull + dead_released` to `available` regardless of
which path was taken. On the unaffordability exit this is fine — by
construction `available` stays ≤ 0 and the no-snipe invariant holds. On
the budget-exhaustion exit, affordable waiters are still linked but the
writeback drives `available` positive, and a fresh fast-path acquirer can
snipe credit that was owed to those parked waiters.

**Reproducer (deterministic, can be added directly as the regression
test):** capacity=100, A acquires 100 fast-path, four waiters park at 10
bytes each (`available = -40`), A releases 100, run a single
`pass(Budget::new(2), ...)`. After the pass, two waiters are granted but
`available = 60` with two affordable waiters still linked. A subsequent
fast-path `poll_acquire(50, ...)` from a fresh task returns
`Poll::Ready(50)` — sniping credit owed to the parked waiters.

**Constraint discussed with author:** do not re-stage the unspent pull
into the `returned` atomic. That would add producer-side contention on a
hot cache line. Hold the unspent credit in a `Distributor`-local `u64`.

**Design:**
- Add `carry: u64` to `Distributor` next to `paid_demand` (around
  `pool.rs:213`).
- At the top of `pass()`, fold it into the pull:
  `let pull = self.pool.returned.swap(0, Ordering::Acquire) + self.carry; self.carry = 0;`.
- At end of pass, before the writeback, compute the safe writeback so
  no-snipe is preserved when live waiters remain. Let
  `live_parked = parked_demand.load() − paid_demand` (recomputed at end of
  pass — earlier waiters may have arrived during the walk, but only the
  ones present at the start were eligible for grants this pass; what we
  care about for no-snipe is "any live demand exists").
  - If `live_parked == 0` → write back the full `pull + dead_released`.
    There is no waiter to snipe from; the surplus belongs to the fast
    path.
  - If `live_parked > 0` → cap the writeback so that
    `available_post = available_pre + writeback ≤ 0`. The
    `dead_released` portion always writes back (those parked subtractions
    are real reclaimed credit, not pull). Cap the pull portion by
    `live_parked` (after-grant outstanding demand). Any pull that doesn't
    fit goes into `self.carry` to be picked up next pass.
- Sketch:
  ```rust
  let live_parked = self.pool.parked_demand.load(Ordering::Acquire) - self.paid_demand;
  let total = pull + dead_released;
  let safe = if live_parked == 0 { total } else { total.min(live_parked + dead_released) };
  self.carry = total - safe;
  if safe > 0 {
      self.pool.available.fetch_add(safe as i64, Ordering::Release);
  }
  ```
  The exact algebra should be re-derived against the conservation
  comment-block at `pool.rs:24-34` and the no-snipe loom invariant before
  committing.
- Update the `pass()` doc-comment at `pool.rs:308-311` and the
  end-of-pass comment at `pool.rs:426-428` so the no-snipe story matches
  the new code. The current comment ("if any live waiter remains, its
  `requested` exceeded `free`, which forces `available <= 0`") is only
  true on the unaffordability exit; restate it.
- Make sure `pass()` still returns "made progress" correctly when
  `carry > 0` is the only thing keeping us running — without forward
  progress signalling, `poll_distribute` could exit Pending while there's
  still work to do.

**Tests (add to `src/credit/pool/tests.rs`):**
- `budget_exhaustion_preserves_no_snipe` — the reproducer above. Assert:
  two slots granted, two still Pending, `debug_available() <= 0`,
  fast-path `poll_acquire(50, ...)` from a fresh slot returns
  `Poll::Pending`.
- `carry_accumulates_across_passes` — same setup, but run multiple passes
  with `Budget::new(1)` each. Assert all four served eventually and
  `dist.carry == 0` at the end.
- `carry_releases_when_queue_drains` — start a pass with `carry > 0`
  inherited from a prior pass, then have all linked waiters abandon
  before/during the pass. Assert the full carry is written back and the
  fast path can see it.
- `budget_exhaustion_with_dead_reaps` — mix dead and live slots so
  `dead_released > 0` and budget runs out mid-walk. Assert conservation.

**Loom (extend `src/credit/pool/loom.rs`):**
- Modify `release_wakes_parked_waiter` (or add a sibling) to use a small
  `Budget` so the pass exits via budget-exhaustion at least once on some
  interleaving. Assert the conservation invariant
  (`available + outstanding + returned + in_flight == capacity`) holds at
  every quiescent point, including the `carry` if appropriate.

**Done when:** all four new deterministic tests pass; the loom model
exercises the budget exit path; `cargo nextest` is green.

---

## Phase 2 — Distributor Drop + Pool close path

**Why:** the most plausible production failure of the current code is
"distributor task gets cancelled and nobody notices." There is no
`impl Drop for Distributor`, no `Pool::close()`, and no `closed` flag on
the pool. With every stream holding `Arc<Pool>` per the integration plan,
if the distributor task is cancelled (panic, `select!` race, runtime
shutdown), `Pool::release` keeps incrementing `returned` and waking a
dead task; new `Pool::poll_acquire` calls park into the shared tiers with
no agent to grant or close them; parked stream futures pin `Arc<Pool>` so
the pool's implicit-drop close path never fires. Restart is structurally
blocked too: `TaskWaker` is `OnceLock<Waker>` with set-once semantics — a
fresh distributor's `register` either trips the `debug_assert` or
silently keeps the dead waker.

**Design:**
- Add `closed: AtomicBool` on `Pool`, initialized `false`.
- `Pool::release(n)` short-circuits when `closed` is set: do not write
  `returned`, do not call `waker.wake()`. (Optional: track release count
  in a `closed_releases` counter for observability — a non-zero value
  means the integrator has a use-after-close bug.)
- `Pool::poll_acquire(...)` checks `closed` after the fast-path
  `fetch_sub` fails: if closed, undo the subtraction (or simply observe
  it; the pool is gone anyway) and return `Poll::Ready(0)` or some
  explicit `Closed` signal. Decide the API: probably the cleanest is for
  the slot to park as today, but the parking branch first checks
  `closed` after acquiring the tier mutex and immediately writes
  `GRANT_CLOSED` instead of linking. Match whatever semantics the
  Distributor's drop walk uses so callers see a single uniform "closed"
  outcome.
- Add `impl Drop for Distributor`:
  ```rust
  impl Drop for Distributor {
      fn drop(&mut self) {
          // 1. Mark the pool closed so concurrent release/poll_acquire
          //    callers stop adding work.
          self.pool.closed.store(true, Ordering::Release);
          // 2. The local mirror's lists drop via the field's Drop impl;
          //    each SlotPtr::drop runs the LINKED -> APP CAS with
          //    GRANT_CLOSED and wakes the parked task.
          // 3. Drain each shared tier under its lock, but drop the
          //    SlotPtrs OUTSIDE the lock (existing rule: wake() must
          //    not run under a tier mutex). Move the tier's contents
          //    into a local List<SlotAdapter>, drop the lock, then drop
          //    the local list.
          for tier in &self.pool.tiers {
              let mut local = List::new();
              {
                  let mut shared = lock(tier);
                  core::mem::swap(&mut local, &mut *shared);
              }
              drop(local); // SlotPtr::drop fires here, no lock held
          }
      }
  }
  ```
  Verify under loom that there is no race between this drop and a
  concurrent `Pool::release` / `Pool::poll_acquire`: the `closed` store
  must be `Release` and the corresponding load on the producer side
  `Acquire`, paired so a producer either sees `closed=true` and
  short-circuits, OR has already published its slot/credit in a way the
  drop walk will pick up.

**Tests (deterministic):**
- `distributor_drop_closes_parked_waiters` — park three slots across
  three different priority tiers, drop the `Distributor` while the
  `Arc<Pool>` is still held by a test handle. Assert all three slots
  observe `GrantResult::Closed` and `wake_count == 3` and the outer
  allocations are not yet freed (they belong to the test, not the pool).
- `release_after_distributor_drop_is_noop` — drop the distributor, call
  `Pool::release(100)`. Assert `returned == 0`, no panic, no wake.
- `poll_acquire_after_distributor_drop_returns_closed` — drop the
  distributor, call `poll_acquire(10, ...)` from a fresh slot. Assert
  the slot ends up in the closed/APP state with `GRANT_CLOSED` (or
  whatever the chosen API surface is) and the test can free its
  allocation.
- `existing_pool_drop_signals_closed` should still pass unchanged.

**Loom:**
- `distributor_drop_races_release` — pre-park one waiter; spawn one
  thread doing `pool.release(N)` and another doing `drop(distributor)`.
  Assert the parked waiter observes either `Granted(N)` (release won and
  was distributed before drop) or `Closed` (drop won), never a stranded
  Pending.
- `distributor_drop_races_poll_acquire` — symmetric: a thread parks while
  another drops the distributor. Assert the outcome is one of the two
  legal terminal states.

**Done when:** all new tests pass; loom is green; the public API of
`Pool` and `Distributor` clearly documents the lifetime contract
("dropping the Distributor closes the Pool; subsequent acquires/releases
are no-ops"). Update the module-level doc-comment on `pool.rs`
accordingly.

---

## Phase 3 — AutoWake migration for the wakers channel

**Why:** the closed-`wakers_tx` issue is not a hot bug — by the time the
waker channel is closed the whole endpoint is shutting down — but the
current code at `pool.rs:295-300` (`let _ = wakers_tx.send(...)`) drops
the batch silently, which leaks already-granted slots (their refcount is
back to APP with `granted=req` written, but their task waker was never
called). The fix is to use the existing `AutoWake` pattern: a token that
calls `wake()` on Drop. If the channel returns Err or the consumer
drops the batch, AutoWake's Drop fires for each entry and no slot is
stranded.

**Constraint:** `AutoWake` currently lives in `src/queue/half.rs:64-88`
and is re-exported from `src/queue.rs`. It is not actually queue-specific.
Move it to a more general home so the credit pool (and any future caller)
can reach it without depending on the queue subsystem.

**Tasks:**
- Move `pub struct AutoWake` from `src/queue/half.rs` to a new module —
  candidates: `src/wake.rs` (top-level, simplest), `src/sync/wake.rs`
  (groups with the existing `sync` shims), or extend `src/sync.rs`
  inline. Pick whichever fits the crate's existing organisation; the
  `src/sync/` directory already contains primitives like `free_list.rs`
  and `waiter.rs`, so `src/sync/wake.rs` reads naturally.
- Re-export `AutoWake` from `src/queue.rs` so existing call sites
  (`src/acceptor.rs`, `src/endpoint/waker.rs`,
  `src/endpoint/dispatch.rs`, `src/queue/half.rs` internals) keep
  working with no edits.
- In `src/credit/pool.rs`, change the `pending_wakers` type from
  `Queue<Waker>` to `Queue<AutoWake>`. Update:
  - The `pending_wakers` declaration around `pool.rs:282`.
  - The `wakers_tx: UnboundedSender<Queue<Waker>>` bound on
    `distribute()` and `poll_distribute()` to use `Queue<AutoWake>`.
  - The grant-path push at `pool.rs:404-412` to wrap the waker in
    `AutoWake::new(Some(waker))` before pushing.
  - The send site at `pool.rs:298-300` — keep the
    `let _ = wakers_tx.send(...)` (the AutoWake handles the failure
    case automatically; document why).
- Update the test sinks `InlineWakeSender` and `CountWake` in
  `src/credit/pool/tests.rs` and `src/credit/pool/loom.rs` to drain
  `Queue<AutoWake>` and call `auto_wake.take()` (or the more direct
  pattern — read the existing usage in `src/endpoint/waker.rs:118-126`
  for a reference).

**Tests:**
- `closed_wakers_tx_does_not_strand` — use a custom
  `UnboundedSender<Queue<AutoWake>>` that always returns `Err` on send.
  Park a slot, release, run distribute. Assert the parked task is woken
  (via AutoWake's Drop) and a follow-up poll observes `Granted(n)`.
- `dropped_batch_wakes_everyone` — sender that accepts the send, then
  the test drops the resulting `Queue<AutoWake>` without draining it.
  Same assertion.

**Done when:** existing call sites compile unchanged after the move; new
tests pass; the doc-comment on `poll_distribute` explains that
AutoWake's Drop handles consumer failure.

---

## Phase 4 — Per-priority `max_single_acquire` + Config hardening

**Why:** two related issues. First, the current
`Config::normalized()` only clamps `max_single_acquire <= i64::MAX`, not
`<= capacity`. With a misconfigured `max_single_acquire` and a few
concurrent waiters, `available` can wrap positive while parkers are
still linked — no-snipe broken via configuration. Second, a single
global `max_single_acquire` is the wrong shape for a pool serving control
+ bulk on one endpoint; a per-priority cap costs nothing and avoids a
deployment-time tuning spiral. Both are small, mechanical changes.

**Tasks (`src/credit/config.rs`):**
- Change `pub max_single_acquire: u64` to
  `pub max_single_acquire: [u64; Priority::LEVELS]`.
- Update `normalized()` to clamp each entry: `>= 1` and
  `<= capacity` (after capacity itself is clamped to `i64::MAX`).
- Update `clamp_request` to take a `Priority` and index into the array.
- Provide ergonomic constructors:
  - `Config::new(capacity: u64) -> Self` with a sensible uniform default.
  - `Config::with_max_single_acquire_uniform(self, n: u64) -> Self`.
  - `Config::with_max_single_acquire_per_priority(self, [u64; LEVELS]) -> Self`.
- Update `Default`: pick a starting policy that reflects
  control-vs-bulk asymmetry. Suggested default (refine when integrating):
  small caps for `Highest`/`High`/`MediumHigh`/`Medium` (e.g.
  `capacity / 256`) so latency-sensitive callers can't monopolise the
  pool, larger caps for `MediumLow`/`Low`/`Lowest`/`Background` (e.g.
  `capacity / 64`). This is a starting point; tune empirically once
  integrated.

**Tasks (`src/credit/pool.rs`):**
- `poll_acquire` passes `priority` to `clamp_request` (around `pool.rs:137`).
- Module-level doc-comment updates if it references the global cap.

**Tests:**
- `config_normalize_clamps_per_priority` — set every entry to
  `u64::MAX`, capacity = 1000; assert each entry normalizes to 1000.
- `config_normalize_minimum_one` — set every entry to 0; assert each
  normalizes to 1.
- `per_priority_clamp_request` — set distinct caps per priority, call
  `clamp_request(huge, p)` for each tier, assert the right cap applies.
- `per_priority_caps_in_pool_acquire` — pool with cap 1000 and
  per-priority `max_single_acquire = [10, 1000, ..., 1000]`. Acquire
  100 at `Highest` from a fully-funded pool, assert `Poll::Ready(10)`;
  acquire 100 at `Medium`, assert `Poll::Ready(100)`.

**Done when:** existing tests still pass after the API migration
(several tests instantiate `Config { capacity, max_single_acquire }`
directly — they need updating); new tests pass.

---

## Phase 5 — Static slot-offset assertion (deferred to integration)

**Why:** `Slot` must be the first field of any outer `#[repr(C)]`
allocation that the credit pool sees, because the pool casts
`NonNull<Slot>` back to the outer type via the stored `drop_fn`. Today
this contract is enforced only by a doc-comment at `slot.rs:54-58`. A
future field-add silently corrupts the cast.

**Tasks:**
- Add a macro to `src/credit.rs` (top of the credit module so it's
  reachable from anywhere a credit-aware allocation is defined):
  ```rust
  /// Static-asserts that `slot: Slot` lives at offset 0 of `$outer`.
  /// Required for every type whose pointer is handed to the credit pool
  /// — the pool casts `NonNull<Slot>` back to the outer type via the
  /// stored `drop_fn`.
  #[macro_export]
  macro_rules! assert_slot_at_offset_zero {
      ($outer:ty) => {
          const _: () = assert!(::core::mem::offset_of!($outer, slot) == 0);
      };
  }
  ```
- Update the doc-comment on `Slot` (`slot.rs:54-69`) to point at the
  macro and to fix the convention that the embedded field MUST be named
  `slot`.
- Apply the macro to the in-crate test allocations
  (`pool/tests.rs::TestAlloc`, `pool/loom.rs::TestAlloc`) as a usage
  example.

**No functional code change.** This phase is mostly documentation and a
one-line macro. It's listed as its own phase so it shows up in the
integration checklist when `WriterAlloc` and `ReaderAlloc` are defined.

**Done when:** the macro exists, both test allocations use it, and the
slot.rs doc-comment references it.

---

## Phase 6 — Counters and tracing

**Why:** the project's experience (per the existing investigation memos)
is that the first incident on a 1000-stream deployment will require
visibility into pool internals. Adding it after the fact means modifying
`credit/pool.rs` mid-incident — exactly the visibility-gap pattern we've
been bitten by before. Get the metrics in before integration so the
first integration test surfaces real numbers.

**Tasks (`src/credit/pool.rs`):**
- Add a `Counters` struct registered with `crate::counter::Registry` at
  `Pool::new` time. The convention in this crate is that callers pass a
  `&Registry` (or pre-registered handles) to subsystem constructors —
  see `src/endpoint.rs` for how `counter::QueueGauge` etc. are
  threaded. Plumb a `&counter::Registry` argument through `Pool::new`,
  or accept a `Counters` struct constructed externally. Match the
  surrounding crate's style.
- Counter list (namespace `credit.*`, following
  `feedback_counter_naming.md`):
  - `credit.acquire.fast_path` — counter, fast-path successes
  - `credit.acquire.parked` — counter, parks (per-tier variant via
    `register_counter_with_variant` if the registry supports it,
    otherwise one counter per tier `credit.acquire.parked.{priority}`)
  - `credit.acquire.bytes` — counter (B), total bytes acquired
  - `credit.release.bytes` — counter (B), bytes released
  - `credit.release.calls` — counter, release call count
  - `credit.parked_demand` — gauge, current
    `parked_demand − paid_demand`
  - `credit.available` — gauge of `available` (signed; if the gauge
    type is unsigned, expose two: `credit.available.positive` and
    `credit.available.deficit`)
  - `credit.distributor.passes` — counter
  - `credit.distributor.granted` — counter, grants delivered
  - `credit.distributor.reaped` — counter, dead slots reaped
  - `credit.distributor.carry_bytes` — gauge, current `carry`
  - `!credit.distributor.budget_exhausted` — counter, passes that
    exited via budget exhaustion (the `!` prefix marks "should be zero
    or low; investigate if persistent")
  - `!credit.abandon.granted_race` — counter, abandon-vs-grant CAS
    losses. This is correct behaviour but a sustained nonzero value
    means something is dropping futures aggressively, worth knowing.
  - `credit.tier.waiters.{priority}` — gauge per tier, sampled at the
    top of each pass (or via the producer-side path; the tier mutex is
    convenient but expensive — pick the cheaper site).
- Per the project memory's `feedback_counter_naming.md`, prefix any
  counter operators should investigate when nonzero with `!`.
- `tracing!` integration: `trace!` on every grant, abandon-vs-grant
  race, and closed-wakeup; `debug!` on `Distributor` lifecycle events
  (registered, dropped, closing tiers); `warn!` if `carry > 0` persists
  across N consecutive passes (signal of a wedged waiter or a misconfig).

**Tasks (`src/credit/slot.rs`):**
- Hook `trace!` into the abandon-vs-grant race outcome so we can
  correlate wake counts with race losses.

**Tests:**
- Existing tests get a `Counters` constructor that uses
  `Registry::new()` (or the test-fixture pattern the rest of the crate
  uses). Spot-check that one counter (e.g. `credit.acquire.fast_path`)
  increments by the expected amount in a representative test.

**Done when:** `cargo nextest` is green; running an existing test with
counters enabled produces non-zero values for the expected metrics.

---

## Out of scope (acknowledged, not doing)

- **Configurable bounded starvation** (design doc R5). The author has
  decided to keep strict-priority head-of-line blocking as the only
  mode. The doc should be amended at integration time to reflect that
  decision; no pool change.
- **Sharded pool counters** for cross-CPU contention. Defer until
  benchmarks show the global atomics are the bottleneck.
- **Safe `AcquireFuture` / `Permit` wrapper** on top of the unsafe
  `poll_acquire`. This belongs with the stream-side wiring (Writer /
  Reader) and is out of scope for the pool itself. It will be the first
  task of the integration PR.

---

## Execution order and review gates

Phase 0 must land first (unblocks loom). Phases 1 and 2 are real
correctness fixes and should be reviewed under loom — do not skip the
loom green light when promoting these. Phase 3 is mechanical once
`AutoWake` is relocated. Phase 4 is straightforward but touches the
public API of `Config`, so the migration of existing tests should be
checked carefully. Phase 5 is documentation. Phase 6 is the largest in
line count but introduces no correctness risk.

Each phase should land as its own commit (or PR) so a regression
bisects cleanly. Phases 1 and 2 must each include their loom additions
in the same commit so the verification doesn't lag the implementation.
