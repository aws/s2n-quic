// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::inet::{SocketAddress, SocketAddressV6};
use std::{
    collections::hash_map::RandomState,
    hash::BuildHasher,
    net::SocketAddr,
    time::{Duration, Instant},
};

pub(super) struct RehandshakeState {
    queue: Vec<SocketAddressV6>,
    handshake_at: Option<Instant>,
    schedule_handshake_at: Instant,

    // Duplicated in map State for lock-free access too.
    rehandshake_period: Duration,

    hasher: RandomState,
}

impl RehandshakeState {
    pub(super) fn new(rehandshake_period: Duration) -> Self {
        Self {
            queue: Default::default(),
            handshake_at: Default::default(),
            schedule_handshake_at: Instant::now(),
            rehandshake_period,

            // Initializes the hasher with random keys, ensuring we handshake with peers in a
            // different, random order from different hosts.
            hasher: RandomState::new(),
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

    pub(super) fn next_rehandshake_batch(
        &mut self,
        peer_count: usize,
        mut request_handshake: impl FnMut(SocketAddr),
    ) {
        // Get the number of handshakes we should run during each minute.
        let mut to_select =
            (60.0 * peer_count as f64 / self.rehandshake_period.as_secs() as f64).trunc() as usize;

        // Roll a random number *once* to schedule the tail handshake. This avoids repeatedly
        // rolling false if we rolled every minute with a small probability of success. This mostly
        // matters in cases where to_select is otherwise zero (i.e., with small peer counts).
        let mut max_delay =
            (self.rehandshake_period.as_secs() as f64 / peer_count as f64).ceil() as u64;

        // Schedule when we're going to add the one handshake.
        if self.handshake_at.is_none()
            && max_delay > 0
            && self.schedule_handshake_at <= Instant::now()
        {
            max_delay = max_delay.clamp(0, self.rehandshake_period.as_secs());
            let delta = rand::random_range(0..max_delay);
            self.handshake_at = Some(Instant::now() + Duration::from_secs(delta));
            self.schedule_handshake_at = Instant::now() + Duration::from_secs(max_delay);
        }

        // If the time when we should add the single handshake, then add it.
        if self.handshake_at.is_some_and(|t| t <= Instant::now()) {
            to_select += 1;
            self.handshake_at = None;
        }

        for idx in 0..to_select {
            let Some(entry) = self.queue.pop() else {
                break;
            };

            request_handshake(entry.unmap().into());

            if idx % 5 == 0 && idx != 0 {
                // Since we handshake in bursts of 5, this still allows 60*1000/10*5 = 30k
                // handshakes/minute, which is orders of magnitude more than we should ever have.
                // At 500k peers with a 24 hour handshake period means ~348 handshakes/minute.
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    }
}
