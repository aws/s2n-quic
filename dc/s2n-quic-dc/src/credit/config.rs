// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::pool::Priority;

#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// Total byte budget for the pool.
    pub capacity: u64,
    /// Maximum bytes a single acquisition can request, indexed by [`Priority`]. Requests are
    /// clamped to the corresponding entry, which also bounds how far `available` can go negative:
    /// at most `Σ concurrent_waiters_at_priority(p) * max_single_acquire[p]`.
    ///
    /// Two reasons this is per-priority rather than a single scalar:
    ///
    /// * **Mixed-criticality fairness.** Latency-sensitive control traffic and bulk data share the
    ///   same pool. A single global cap forces a tradeoff between "small enough that bulk can't
    ///   starve control" and "large enough that bulk doesn't fragment into a parade of small
    ///   acquires." Per-priority caps let small caps for control coexist with larger caps for
    ///   bulk on the same pool.
    /// * **Misconfiguration containment.** A single huge `max_single_acquire` plus a few
    ///   concurrent waiters can drive `available` so far negative that no-snipe breaks under the
    ///   conservation arithmetic. Bounding the cap separately per priority bounds the worst-case
    ///   negative excursion *per tier*.
    pub max_single_acquire: [u64; Priority::LEVELS],
    /// Minimum bytes a demand-elastic fair-share grant hands a parked waiter, indexed by
    /// [`Priority`]. See `Distributor::pass`: under contention the per-waiter share
    /// (`free / parked`) shrinks toward zero, so the distributor floors each grant here to keep it
    /// at least a few frames — every served waiter then makes real forward progress instead of
    /// receiving sub-frame slivers that just force another acquire round-trip. The floor never
    /// raises a grant above the credit actually free, nor above `max_single_acquire[p]`.
    ///
    /// Per-priority (like `max_single_acquire`) so latency-sensitive tiers can use a small slice
    /// (finer round-robin, lower head-of-line delay) while bulk tiers use a larger one (fewer
    /// re-acquire round-trips per unit of throughput).
    pub min_grant_slice: [u64; Priority::LEVELS],
}

/// Default minimum fair-share grant slice: a handful of MTU-sized frames. Large enough that a
/// served waiter does useful work before re-acquiring, small enough that many waiters can be served
/// per pass under contention. Tune empirically via dc-tester.
const DEFAULT_MIN_GRANT_SLICE: u64 = 64 * 1024;

impl Default for Config {
    #[inline]
    fn default() -> Self {
        // Asymmetric default: smaller caps for the latency-sensitive tiers (so a single bulky
        // control-tier acquisition can't monopolize the pool) and larger caps for the bulk tiers.
        // Tune empirically once integrated; this is a starting point.
        let cap = 256 * 1024 * 1024;
        let small = cap / 256; // 1 MiB at the default capacity
        let large = cap / 64; // 4 MiB at the default capacity
        Self {
            capacity: cap,
            max_single_acquire: [
                small, small, small, small, // Highest, High, MediumHigh, Medium
                large, large, large, large, // MediumLow, Low, Lowest, Background
            ],
            min_grant_slice: [DEFAULT_MIN_GRANT_SLICE; Priority::LEVELS],
        }
    }
}

impl Config {
    /// Construct a config with the given capacity and the default per-priority caps scaled to
    /// that capacity. See [`Default`] for the cap shape.
    #[inline]
    pub fn new(capacity: u64) -> Self {
        let cap = capacity.max(1);
        let small = (cap / 256).max(1);
        let large = (cap / 64).max(1);
        Self {
            capacity,
            max_single_acquire: [
                small, small, small, small, // Highest, High, MediumHigh, Medium
                large, large, large, large, // MediumLow, Low, Lowest, Background
            ],
            min_grant_slice: [DEFAULT_MIN_GRANT_SLICE; Priority::LEVELS],
        }
    }

    /// Override the per-priority cap with a single value applied uniformly.
    #[inline]
    pub fn with_max_single_acquire_uniform(mut self, n: u64) -> Self {
        self.max_single_acquire = [n; Priority::LEVELS];
        self
    }

    /// Override the per-priority cap explicitly, one entry per [`Priority`] level.
    #[inline]
    pub fn with_max_single_acquire_per_priority(mut self, caps: [u64; Priority::LEVELS]) -> Self {
        self.max_single_acquire = caps;
        self
    }

    /// Override the minimum fair-share grant slice with a single value applied uniformly.
    #[inline]
    pub fn with_min_grant_slice_uniform(mut self, n: u64) -> Self {
        self.min_grant_slice = [n; Priority::LEVELS];
        self
    }

    /// Override the minimum fair-share grant slice explicitly, one entry per [`Priority`] level.
    #[inline]
    pub fn with_min_grant_slice_per_priority(mut self, slices: [u64; Priority::LEVELS]) -> Self {
        self.min_grant_slice = slices;
        self
    }

    #[inline]
    pub(crate) fn normalized(self) -> Self {
        let capacity = self.capacity.min(i64::MAX as u64);
        // Cap per-priority max_single_acquire at the *larger* of `capacity` and 1. This bounds
        // how far a single parker can drive `available` negative (a misconfigured `u64::MAX`
        // entry would let one waiter break no-snipe). The floor of 1 keeps the cap above the
        // `n == 0` short-circuit in `poll_acquire`. Tests deliberately use `capacity = 0` to
        // force the park branch; clamping to `capacity.max(1)` would degenerate the cap to 1
        // and silently shrink test requests, so the clamp uses `capacity` only when capacity is
        // non-zero.
        let cap_bound = if capacity == 0 {
            i64::MAX as u64
        } else {
            capacity
        };
        let mut max_single_acquire = self.max_single_acquire;
        for entry in max_single_acquire.iter_mut() {
            *entry = (*entry).max(1).min(cap_bound);
        }
        // The fair-share floor must be a usable grant size: at least 1 (so the distributor always
        // makes progress) and never above this tier's per-acquire cap (a grant can't exceed what a
        // single request could be). The cap is already clamped to the capacity bound above, so
        // clamping to it also keeps the slice within capacity for real pools — while preserving the
        // `capacity == 0` test carve-out (unbounded), which clamping directly to capacity would
        // wrongly collapse to 1. A pool smaller than the configured slice thus still grants: the
        // slice lands at capacity, not above it. See `Distributor::pass`.
        let mut min_grant_slice = self.min_grant_slice;
        for (slice, cap) in min_grant_slice.iter_mut().zip(max_single_acquire.iter()) {
            *slice = (*slice).max(1).min(*cap);
        }
        Self {
            capacity,
            max_single_acquire,
            min_grant_slice,
        }
    }

    #[inline]
    pub(crate) fn clamp_request(&self, n: u64, priority: Priority) -> u64 {
        n.min(self.max_single_acquire[priority as usize])
    }
}

#[cfg(all(test, not(feature = "loom")))]
mod tests {
    use super::*;

    #[test]
    fn config_normalize_clamps_per_priority() {
        let c = Config {
            capacity: 1000,
            max_single_acquire: [u64::MAX; Priority::LEVELS],
            min_grant_slice: [DEFAULT_MIN_GRANT_SLICE; Priority::LEVELS],
        }
        .normalized();
        for entry in c.max_single_acquire.iter() {
            assert_eq!(*entry, 1000);
        }
    }

    #[test]
    fn config_normalize_minimum_one() {
        let c = Config {
            capacity: 1000,
            max_single_acquire: [0; Priority::LEVELS],
            min_grant_slice: [DEFAULT_MIN_GRANT_SLICE; Priority::LEVELS],
        }
        .normalized();
        for entry in c.max_single_acquire.iter() {
            assert_eq!(*entry, 1);
        }
    }

    #[test]
    fn per_priority_clamp_request() {
        let caps = [10, 20, 30, 40, 50, 60, 70, 80];
        let c = Config {
            capacity: 1000,
            max_single_acquire: caps,
            min_grant_slice: [DEFAULT_MIN_GRANT_SLICE; Priority::LEVELS],
        }
        .normalized();
        let priorities = [
            Priority::Highest,
            Priority::High,
            Priority::MediumHigh,
            Priority::Medium,
            Priority::MediumLow,
            Priority::Low,
            Priority::Lowest,
            Priority::Background,
        ];
        for (i, p) in priorities.iter().enumerate() {
            assert_eq!(c.clamp_request(u64::MAX, *p), caps[i]);
        }
    }

    #[test]
    fn config_new_scales_caps_to_capacity() {
        // Default helper produces (capacity/256, capacity/64) caps.
        let c = Config::new(8192).normalized();
        // Highest tiers
        assert_eq!(c.max_single_acquire[0], 32); // 8192/256
                                                 // Bulk tiers
        assert_eq!(c.max_single_acquire[7], 128); // 8192/64
    }

    #[test]
    fn with_max_single_acquire_uniform_overrides_defaults() {
        let c = Config::new(8192).with_max_single_acquire_uniform(99);
        for entry in c.max_single_acquire.iter() {
            assert_eq!(*entry, 99);
        }
    }

    #[test]
    fn min_grant_slice_clamped_to_cap_and_capacity() {
        // Slice requested far above both the per-acquire cap and the capacity: normalize must clamp
        // it to the per-priority `max_single_acquire` (which itself is clamped to capacity).
        let c = Config::new(8192)
            .with_max_single_acquire_uniform(1024)
            .with_min_grant_slice_uniform(u64::MAX)
            .normalized();
        for slice in c.min_grant_slice.iter() {
            assert_eq!(*slice, 1024, "slice must clamp to max_single_acquire");
        }

        // A pool smaller than the configured slice must still grant: slice clamps to capacity.
        let c = Config::new(4096)
            .with_max_single_acquire_uniform(4096)
            .with_min_grant_slice_uniform(64 * 1024)
            .normalized();
        for slice in c.min_grant_slice.iter() {
            assert_eq!(
                *slice, 4096,
                "slice must clamp to capacity when capacity < slice"
            );
        }
    }

    #[test]
    fn min_grant_slice_minimum_one() {
        let c = Config::new(8192)
            .with_min_grant_slice_uniform(0)
            .normalized();
        for slice in c.min_grant_slice.iter() {
            assert_eq!(*slice, 1);
        }
    }
}
