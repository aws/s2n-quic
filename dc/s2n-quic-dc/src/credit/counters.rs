// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Observability counters and gauges for [`crate::credit::Pool`] and
//! [`crate::credit::Distributor`].
//!
//! Names follow the project convention:
//!
//! * Namespace: `credit.*`.
//! * `!`-prefixed names mark counters that should be **zero or low** in healthy operation —
//!   operators investigate when they grow. The list:
//!   - `!credit.distributor.budget_exhausted` — passes that exited via the per-poll budget. A
//!     persistent nonzero rate means the budget is tuned too tight or the pool is overdriven.
//!   - `!credit.abandon.granted_race` — abandon-vs-grant CAS losses. Correct behaviour, but a
//!     sustained nonzero value means callers are dropping `AcquireFuture`s aggressively.
//!   - `!credit.{dir}.refill.sustained_engaged` — runs of consecutive pacer ticks that injected
//!     credit with no intervening release-met-rate tick. A short burst breaks a transient wedge and
//!     is fine; sustained growth means the pool relies on injection indefinitely — an undersized
//!     pool for the workload's concurrency, or a credit leak the pacer is masking.
//!
//! All counters and gauges are constructed from a [`crate::counter::Registry`]. Use
//! [`Counters::default`] when no registry is wired up — it registers against a default `Registry`
//! that drops every emit on the floor (no overhead in the hot path beyond the Counter handle's
//! own atomic increment).

use crate::{
    counter::{Counter, Gauge, Registry},
    credit::pool::Priority,
};

/// Observability handles for a single credit pool.
///
/// One instance per [`Pool`]/[`Distributor`] pair. Cheap to clone — every field is an
/// `Arc`-backed handle into the registry.
#[derive(Clone)]
pub struct Counters {
    /// Fast-path acquires that succeeded without parking. Counts events, not bytes.
    pub acquire_fast_path: Counter,
    /// Acquires that parked (exhausted the fast path), per priority. The variant name is the
    /// `Priority` enum's debug representation (e.g. `Highest`, `Background`).
    pub acquire_parked: [Counter; Priority::LEVELS],
    /// Bytes acquired (sum of fast-path grants and parked grants).
    pub acquire_bytes: Counter,
    /// Bytes returned to the pool via [`Pool::release`].
    pub release_bytes: Counter,
    /// Number of `release` calls that did real work (not no-ops on a closed pool).
    pub release_calls: Counter,

    /// Distributor passes — one per call to `pass()`. Bounded by the per-poll budget.
    pub distributor_passes: Counter,
    /// Grants delivered. Counts slots woken, not bytes (use `acquire_bytes` for the byte volume).
    pub distributor_granted: Counter,
    /// Dead slots reaped during distribution. Healthy steady-state may be nonzero (callers can
    /// abandon mid-park); sustained spikes correlate with churn or aggressive future drops.
    pub distributor_reaped: Counter,
    /// Pull bytes the distributor carried into the next pass because writing them back would
    /// have driven `available` positive past live parked demand. A persistent nonzero value here
    /// is normal under heavy contention; persistent *growth* indicates a wedged tier or bug.
    pub distributor_carry_bytes: Gauge,
    /// Passes that exited via budget exhaustion. Should be zero or low; otherwise the budget is
    /// undersized or the pool is overdriven. (`!`-prefixed: investigate when nonzero.)
    pub distributor_budget_exhausted: Counter,

    /// Abandon-vs-grant CAS losses observed by the distributor. The slot was abandoned between
    /// the distributor's `is_dead` check and its grant CAS — correct behaviour, but a high rate
    /// signals callers dropping futures aggressively. (`!`-prefixed: investigate when nonzero.)
    pub abandon_granted_race: Counter,

    /// Bytes injected by the refill pacer (the liveness floor). Zero unless `Config::refill` is set
    /// and the pool actually wedged; pairs with `release.bytes` to gauge how much forward progress
    /// came from injection vs. real releases.
    pub refill_bytes_injected: Counter,
    /// Pacer ticks that injected credit (the round's real releases fell short of the rate). Each is
    /// a tick the pacer kept the parked queue moving.
    pub refill_ticks: Counter,
    /// Pacer ticks that injected nothing because real releases already met or exceeded the rate this
    /// round — the pool was healthy, so it was "as if the pacer never ran."
    pub refill_skipped: Counter,
    /// Runs of consecutive injecting ticks with no intervening skip. A short burst breaks a
    /// transient wedge; sustained growth means the pool relies on injection indefinitely (undersized
    /// pool or a masked leak). (`!`-prefixed: investigate when growing.)
    pub refill_sustained_engaged: Counter,
    /// Current run length of consecutive injecting ticks (resets to 0 on a skip). The live
    /// counterpart to `refill_sustained_engaged`: a sampled scrape of this run length answers "is
    /// the pool wedged right now, and for how long" where the monotonic counters only answer "ever".
    /// Published by the distributor once per tick.
    pub refill_consecutive_ticks: Gauge,
    /// Wedge depth in bytes: `max(0, capacity − available)` while a waiter is parked, else 0. Would
    /// have read ~2.8 MB on the repro that motivated the pacer. Published by the distributor each
    /// tick (it already reads `available` every pass).
    pub refill_deficit: Gauge,
}

impl Counters {
    /// Construct counters under the default `credit.*` namespace. Use [`new_with_prefix`] when
    /// multiple pools share a registry (e.g. one send-direction pool and one recv-direction pool
    /// on the same endpoint) so their counter names don't collide.
    ///
    /// [`new_with_prefix`]: Self::new_with_prefix
    pub fn new(registry: &Registry) -> Self {
        Self::new_with_prefix(registry, "credit")
    }

    /// Construct counters with a custom namespace prefix. The prefix replaces the leading `credit`
    /// segment of every counter name, so for example `Counters::new_with_prefix(registry, "credit.send")`
    /// produces `credit.send.acquire.fast_path`, `!credit.send.distributor.budget_exhausted`, etc.
    ///
    /// The prefix should not contain a trailing dot. The leading `!` for nominal-bad counters is
    /// preserved.
    pub fn new_with_prefix(registry: &Registry, prefix: &str) -> Self {
        let acquire_parked = std::array::from_fn(|i| {
            let priority = match i {
                0 => "Highest",
                1 => "High",
                2 => "MediumHigh",
                3 => "Medium",
                4 => "MediumLow",
                5 => "Low",
                6 => "Lowest",
                7 => "Background",
                _ => unreachable!(),
            };
            registry.register_nominal(format!("{prefix}.acquire.parked"), priority)
        });

        Self {
            acquire_fast_path: registry.register(format!("{prefix}.acquire.fast_path")),
            acquire_parked,
            acquire_bytes: registry.register_bytes(format!("{prefix}.acquire.bytes")),
            release_bytes: registry.register_bytes(format!("{prefix}.release.bytes")),
            release_calls: registry.register(format!("{prefix}.release.calls")),
            distributor_passes: registry.register(format!("{prefix}.distributor.passes")),
            distributor_granted: registry.register(format!("{prefix}.distributor.granted")),
            distributor_reaped: registry.register(format!("{prefix}.distributor.reaped")),
            distributor_carry_bytes: registry
                .register_gauge(format!("{prefix}.distributor.carry_bytes")),
            distributor_budget_exhausted: registry
                .register(format!("!{prefix}.distributor.budget_exhausted")),
            abandon_granted_race: registry.register(format!("!{prefix}.abandon.granted_race")),
            refill_bytes_injected: registry
                .register_bytes(format!("{prefix}.refill.bytes_injected")),
            refill_ticks: registry.register(format!("{prefix}.refill.ticks")),
            refill_skipped: registry.register(format!("{prefix}.refill.skipped")),
            refill_sustained_engaged: registry
                .register(format!("!{prefix}.refill.sustained_engaged")),
            refill_consecutive_ticks: registry
                .register_gauge(format!("{prefix}.refill.consecutive_ticks")),
            refill_deficit: registry.register_gauge(format!("{prefix}.refill.deficit")),
        }
    }
}

impl Default for Counters {
    /// Construct a `Counters` against a default-initialized `Registry`. The registry is local to
    /// this struct and never publishes anywhere — every emit is dropped on the floor. Use this
    /// when no observability is wired up (tests, internal benchmarks).
    fn default() -> Self {
        Self::new(&Registry::default())
    }
}
