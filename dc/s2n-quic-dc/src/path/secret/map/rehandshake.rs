// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use rand::seq::SliceRandom;
use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

pub(super) struct RehandshakeState {
    // FIXME: This is larger than it needs to be because SocketAddr is 32 bytes. We should consider
    // some other storage form, since IPv4 is 6 bytes and IPv6 is 28 bytes (18 bytes if we ignore
    // scope ID), almost anything would be smaller than this. But this is cheaper to implement and
    // we can revisit memory impact separately.
    //
    // Splitting IPv4 and IPv6 would help, but it would also mean that we scan one and then the
    // other, which is probably bad idea long-term -- we want to visit peers randomly.
    queue: Vec<SocketAddr>,
    handshake_at: Option<Instant>,
    schedule_handshake_at: Instant,

    // Duplicated in map State for lock-free access too.
    rehandshake_period: Duration,
}

impl RehandshakeState {
    pub(super) fn new(rehandshake_period: Duration) -> Self {
        Self {
            queue: Default::default(),
            handshake_at: Default::default(),
            schedule_handshake_at: Instant::now(),
            rehandshake_period,
        }
    }

    pub(super) fn needs_refill(&mut self) -> bool {
        self.queue.is_empty()
    }

    pub(super) fn push(&mut self, peer: SocketAddr) {
        self.queue.push(peer);
    }

    pub(super) fn adjust_post_refill(&mut self) {
        self.queue.sort_unstable();
        self.queue.dedup();

        // Shuffling each time we pull a new queue means that we have p100 re-handshake time
        // double the expected handshake period, because the entry handshaked at p0 on the
        // first pass might end up at p100 on the second pass. We're OK with that tradeoff --
        // the randomization avoids thundering herds against the same host, and while we could
        // remember an order it's harder to get diffing that order with new entries right.
        let mut rng = rand::rng();
        self.queue.shuffle(&mut rng);
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

            request_handshake(entry);

            if idx % 25 == 0 && idx != 0 {
                // Since we handshake in bursts of 25, this still allows 60*1000/50*25 = 30k
                // handshakes/minute, which is orders of magnitude more than we should ever have. At
                // 500k peers with a 24 hour handshake period means ~348 handshakes/minute.
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}
