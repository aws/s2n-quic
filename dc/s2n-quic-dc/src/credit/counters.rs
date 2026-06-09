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
