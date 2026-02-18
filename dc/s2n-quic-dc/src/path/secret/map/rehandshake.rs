// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::inet::{SocketAddress, SocketAddressV6};
use std::{
    collections::hash_map::RandomState, hash::BuildHasher, mem::ManuallyDrop, net::SocketAddr,
    sync::Arc, time::Duration,
};
use tokio::{runtime::Runtime, sync::Semaphore, task::JoinHandle, time::Instant};

pub(super) struct RehandshakeState {
    queue: Vec<SocketAddressV6>,
    handshake_at: Option<Instant>,
    schedule_handshake_at: Instant,

    // Duplicated in map State for lock-free access too.
    rehandshake_period: Duration,

    hasher: RandomState,
    runtime: ManuallyDrop<Runtime>,
    semaphore: Arc<Semaphore>,
}

impl RehandshakeState {
    pub(super) fn new(rehandshake_period: Duration) -> Self {
        Self::new_with_runtime(rehandshake_period, false)
    }

    #[cfg(test)]
    fn new_with_paused_time(rehandshake_period: Duration) -> Self {
        Self::new_with_runtime(rehandshake_period, true)
    }

    fn new_with_runtime(rehandshake_period: Duration, start_paused: bool) -> Self {
        let mut builder = tokio::runtime::Builder::new_current_thread();
        builder.enable_all();
        if start_paused {
            #[cfg(test)]
            builder.start_paused(true);
        }
        let runtime = builder.build().unwrap();
        let _guard = runtime.enter();
        let now = Instant::now();
        Self {
            queue: Default::default(),
            handshake_at: Default::default(),
            schedule_handshake_at: now,
            rehandshake_period,

            // Initializes the hasher with random keys, ensuring we handshake with peers in a
            // different, random order from different hosts.
            hasher: RandomState::new(),
            // This is arbitrarily chosen, and should be less than the permits granted to
            // handshakes in the handshake client (currently limited to 5).
            semaphore: Arc::new(Semaphore::new(2)),
            runtime: ManuallyDrop::new(runtime),
        }
    }

    pub(super) fn needs_refill(&mut self) -> bool {
        self.queue.is_empty()
    }

    pub(super) fn push(&mut self, peer: SocketAddr) {
        self.queue.push(SocketAddress::from(peer).to_ipv6_mapped());
    }

    pub(super) fn adjust_post_refill(&mut self) {
        // Sort by hash, and if hashes are the same, by the SocketAddr. We need to include both in
        // the comparison key to ensure that we find duplicate entries correctly (just by hash
        // might have different addresses interleave).
        self.queue
            .sort_unstable_by_key(|peer| (self.hasher.hash_one(peer), *peer));
        self.queue.dedup();
    }

    pub(super) fn reserve(&mut self, capacity: usize) {
        self.queue.reserve(capacity);
    }

    /// Returns the number of handshake requests skipped due to handshakes taking too long.
    pub(super) fn next_rehandshake_batch(
        &mut self,
        peer_count: usize,
        mut request_handshake: impl FnMut(SocketAddr) -> Option<JoinHandle<()>>,
    ) -> usize {
        let _guard = self.runtime.enter();
        let start = Instant::now();

        // Reduce the batch deadline to give time for our p100 handshake duration, which we expect
        // to be approximately 10 seconds.
        let batch_deadline = start + Duration::from_secs(50);

        // Get the number of handshakes we should run during each minute.
        let mut to_select =
            (60.0 * peer_count as f64 / self.rehandshake_period.as_secs() as f64).trunc() as usize;

        // Roll a random number *once* to schedule the tail handshake. This avoids repeatedly
        // rolling false if we rolled every minute with a small probability of success. This mostly
        // matters in cases where to_select is otherwise zero (i.e., with small peer counts).
        let mut max_delay =
            (self.rehandshake_period.as_secs() as f64 / peer_count as f64).ceil() as u64;

        // Schedule when we're going to add the one handshake.
        if self.handshake_at.is_none() && max_delay > 0 && self.schedule_handshake_at <= start {
            max_delay = max_delay.clamp(0, self.rehandshake_period.as_secs());
            let delta = rand::random_range(0..max_delay);
            self.handshake_at = Some(start + Duration::from_secs(delta));
            self.schedule_handshake_at = start + Duration::from_secs(max_delay);
        }

        // If the time when we should add the single handshake, then add it.
        if self.handshake_at.is_some_and(|t| t <= start) {
            to_select += 1;
            self.handshake_at = None;
        }

        let mut handles = Vec::new();
        let mut last_spawn = start;

        while to_select > 0 {
            // Check if we've exceeded the batch deadline
            let now = Instant::now();
            if now >= batch_deadline {
                break;
            }

            to_select -= 1;

            let Some(entry) = self.queue.pop() else {
                to_select = 0;
                break;
            };

            // Pace handshakes by waiting 100ms since the last spawn. If a handshake is slow this
            // is a no-op but otherwise this effectively limits concurrency to 1.
            let pace_until = last_spawn + Duration::from_millis(100);
            self.runtime.block_on(tokio::time::sleep_until(pace_until));
            last_spawn = Instant::now();

            // Wait for a slot if we're at capacity
            let permit = self
                .runtime
                .block_on(self.semaphore.clone().acquire_owned())
                .unwrap();

            if let Some(handle) = request_handshake(entry.unmap().into()) {
                let wrapped = self.runtime.spawn(async move {
                    handle.await.expect("propagate panic");
                    drop(permit);
                });
                handles.push(wrapped);
            } else {
                drop(permit);
            }
        }

        // Wait for all tasks to complete
        for handle in handles {
            self.runtime.block_on(handle).expect("propagate panic");
        }

        to_select
    }
}

impl Drop for RehandshakeState {
    fn drop(&mut self) {
        // SAFETY: This runs in Drop and no further usage of the ManuallyDrop occurs.
        unsafe {
            ManuallyDrop::take(&mut self.runtime).shutdown_background();
        }
    }
}

#[cfg(test)]
mod test;
