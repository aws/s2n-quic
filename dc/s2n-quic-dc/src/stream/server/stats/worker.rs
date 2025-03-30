// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{Receiver, Stats};
use crate::clock::Clock;
use core::time::Duration;
use s2n_quic_core::{packet::number::PacketNumberSpace, recovery::RttEstimator};

pub struct Worker {
    queue: Receiver,
    stats: Stats,
}

impl Worker {
    pub fn new(queue: Receiver, stats: Stats) -> Self {
        Self { queue, stats }
    }

    pub async fn run<C: Clock>(self, clock: C) {
        let mut rtt_estimator = RttEstimator::new(Duration::from_secs(30));

        let debounce = Duration::from_millis(5);
        let timeout = Duration::from_millis(5);

        let mut timer = clock.timer();

        loop {
            let Ok(sample) = self.queue.recv_back().await else {
                break;
            };

            let now = clock.get_time();
            rtt_estimator.update_rtt(
                Duration::ZERO,
                sample,
                now,
                true,
                PacketNumberSpace::ApplicationData,
            );

            // allow some more samples to come through
            timer.sleep_until(clock.get_time() + debounce).await;

            while let Ok(Some(sample)) = self.queue.try_recv_back() {
                rtt_estimator.update_rtt(
                    Duration::ZERO,
                    sample,
                    now,
                    true,
                    PacketNumberSpace::ApplicationData,
                );
            }

            self.stats.update(&rtt_estimator);

            // wait before taking a new sample to avoid spinning
            timer.sleep_until(clock.get_time() + timeout).await;
        }
    }
}
