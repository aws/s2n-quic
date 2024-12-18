// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{event::Subscriber, sync::channel as chan};
use core::{
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
use s2n_quic_core::{packet::number::PacketNumberSpace, recovery::RttEstimator, time::Clock};
use std::sync::Arc;
use tokio::time::sleep;

pub fn channel() -> (Sender, Worker, Stats) {
    // TODO configure this queue depth?
    let (send, recv) = chan::new(1024);
    let sender = Sender(send);
    let stats = Stats::default();
    let worker = Worker {
        queue: recv,
        stats: stats.clone(),
    };
    (sender, worker, stats)
}

#[derive(Clone)]
pub struct Sender(chan::Sender<Duration>);

impl Sender {
    #[inline]
    pub fn send(&self, sojourn_time: Duration) {
        // prefer recent samples
        let _ = self.0.send_back(sojourn_time);
    }
}

impl Subscriber for Sender {
    type ConnectionContext = ();

    #[inline]
    fn create_connection_context(
        &self,
        _meta: &crate::event::api::ConnectionMeta,
        _info: &crate::event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    #[inline]
    fn on_acceptor_stream_dequeued(
        &self,
        _meta: &crate::event::api::EndpointMeta,
        event: &crate::event::api::AcceptorStreamDequeued,
    ) {
        self.send(event.sojourn_time);
    }
}

pub struct Worker {
    queue: chan::Receiver<Duration>,
    stats: Stats,
}

impl Worker {
    pub async fn run<C: Clock>(self, clock: C) {
        let mut rtt_estimator = RttEstimator::new(Duration::from_secs(30));

        let debounce = Duration::from_millis(5);
        let timeout = Duration::from_millis(5);

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
            sleep(debounce).await;

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
            sleep(timeout).await;
        }
    }
}

#[derive(Clone, Default)]
pub struct Stats(Arc<StatsState>);

impl Stats {
    #[inline]
    pub fn smoothed_sojourn_time(&self) -> Duration {
        Duration::from_nanos(self.0.smoothed_sojourn_time.load(Ordering::Relaxed))
    }

    fn update(&self, rtt_estimator: &RttEstimator) {
        let smoothed_rtt = rtt_estimator.smoothed_rtt().as_nanos().min(u64::MAX as _) as _;
        self.0
            .smoothed_sojourn_time
            .store(smoothed_rtt, Ordering::Relaxed);
    }
}

#[derive(Default)]
struct StatsState {
    smoothed_sojourn_time: AtomicU64,
}
