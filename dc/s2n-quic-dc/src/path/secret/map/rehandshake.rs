// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::clock::{Clock, Timer};
use s2n_quic_core::{
    inet::{SocketAddress, SocketAddressV6},
    time::Timestamp,
};
use std::{
    collections::hash_map::RandomState, hash::BuildHasher, marker::PhantomData, net::SocketAddr,
    time::Duration,
};

pub(super) struct RehandshakeState<Clk> {
    queue: Vec<SocketAddressV6>,
    handshake_at: Option<Timestamp>,
    schedule_handshake_at: Timestamp,
    burst_timer: Timer,

    // Duplicated in map State for lock-free access too.
    rehandshake_period: Duration,

    hasher: RandomState,
    clock: PhantomData<Clk>,
}

impl<Clk: Clock> RehandshakeState<Clk> {
    pub(super) fn new(rehandshake_period: Duration, clock: &Clk) -> Self {
        Self {
            queue: Default::default(),
            handshake_at: None,
            schedule_handshake_at: clock.get_time(),
            burst_timer: clock.timer(),
            rehandshake_period,

            // Initializes the hasher with random keys, ensuring we handshake with peers in a
            // different, random order from different hosts.
            hasher: RandomState::new(),
            clock: PhantomData,
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

    pub(super) async fn next_rehandshake_batch(
        &mut self,
        peer_count: usize,
        clock: &Clk,
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

        {
            let now = clock.get_time();

            // Schedule when we're going to add the one handshake.
            if self.handshake_at.is_none() && max_delay > 0 && self.schedule_handshake_at <= now {
                max_delay = max_delay.clamp(0, self.rehandshake_period.as_secs());
                let delta = rand::random_range(0..max_delay);
                self.handshake_at = Some(now + Duration::from_secs(delta));
                self.schedule_handshake_at = now + Duration::from_secs(max_delay);
            }

            // If the time when we should add the single handshake, then add it.
            if self.handshake_at.is_some_and(|t| t <= now) {
                to_select += 1;
                self.handshake_at = None;
            }
        }

        for idx in 0..to_select {
            let Some(entry) = self.queue.pop() else {
                break;
            };

            request_handshake(entry.unmap().into());

            if idx % 25 == 0 && idx != 0 {
                // Since we handshake in bursts of 25, this still allows 60*1000/50*25 = 30k
                // handshakes/minute, which is orders of magnitude more than we should ever have. At
                // 500k peers with a 24 hour handshake period means ~348 handshakes/minute.
                let target = clock.get_time();
                self.burst_timer
                    .sleep_until(target + Duration::from_millis(50))
                    .await;
            }
        }
    }
}
