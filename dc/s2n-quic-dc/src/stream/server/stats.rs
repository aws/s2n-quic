// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{event::Subscriber, sync::mpmc as chan};
use core::time::Duration;
use s2n_quic_core::recovery::RttEstimator;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

mod worker;

pub use worker::Worker;

type Receiver = chan::Receiver<Duration>;

pub fn channel() -> (Sender, Worker, Stats) {
    // TODO configure this queue depth?
    let (send, recv) = chan::new(1024);
    let sender = Sender(send);
    let stats = Stats::default();
    let worker = Worker::new(recv, stats.clone());
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

#[derive(Clone, Default)]
pub struct Stats(Arc<StatsState>);

impl Stats {
    #[inline]
    pub fn smoothed_sojourn_time(&self) -> Duration {
        Duration::from_nanos(self.0.smoothed_sojourn_time.load(Ordering::Relaxed))
    }

    pub fn update(&self, rtt_estimator: &RttEstimator) {
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
