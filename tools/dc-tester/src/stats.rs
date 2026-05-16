// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Packet loss statistics subscriber for dc-tester.
//!
//! Implements the `s2n_quic_dc::event::Subscriber` trait to track
//! packet transmission and loss events, providing periodic reporting
//! of loss rates and related metrics.

use s2n_quic_dc::event::api;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tracing::info;

/// Shared packet statistics counters.
struct StatsInner {
    /// Application-level requests currently in progress
    requests_in_progress: AtomicU64,
    /// Application-level requests completed
    requests_completed: AtomicU64,
    /// Application-level bytes sent
    app_bytes_sent: AtomicU64,
    /// Application-level bytes received
    app_bytes_received: AtomicU64,
    /// Application-level errors
    errors: AtomicU64,
}

impl StatsInner {
    fn new() -> Self {
        Self {
            requests_in_progress: AtomicU64::new(0),
            requests_completed: AtomicU64::new(0),
            app_bytes_sent: AtomicU64::new(0),
            app_bytes_received: AtomicU64::new(0),
            errors: AtomicU64::new(0),
        }
    }

    #[expect(dead_code)]
    fn report(&self) {
        let requests_in_progress = self.requests_in_progress.load(Ordering::Relaxed);
        let requests_completed = self.requests_completed.swap(0, Ordering::Relaxed);
        let app_bytes_sent = self.app_bytes_sent.swap(0, Ordering::Relaxed);
        let app_bytes_received = self.app_bytes_received.swap(0, Ordering::Relaxed);
        let errors = self.errors.swap(0, Ordering::Relaxed);

        // Get jemalloc memory stats - need to advance epoch first
        let _ = tikv_jemalloc_ctl::epoch::advance();
        let allocated_mb = tikv_jemalloc_ctl::stats::allocated::read()
            .map(|b| b as f64 / 1024.0 / 1024.0)
            .unwrap_or(0.0);
        let resident_mb = tikv_jemalloc_ctl::stats::resident::read()
            .map(|b| b as f64 / 1024.0 / 1024.0)
            .unwrap_or(0.0);
        let active_mb = tikv_jemalloc_ctl::stats::active::read()
            .map(|b| b as f64 / 1024.0 / 1024.0)
            .unwrap_or(0.0);
        let retained_mb = tikv_jemalloc_ctl::stats::retained::read()
            .map(|b| b as f64 / 1024.0 / 1024.0)
            .unwrap_or(0.0);

        let success = requests_completed.saturating_sub(errors);
        let success_rate = if requests_completed > 0 {
            success as f64 / requests_completed as f64
        } else {
            0.0
        };

        info!(
            requests_in_progress,
            requests_completed,
            app_bytes_sent,
            app_bytes_received,
            errors,
            success_rate = %format_args!("{:.2}%", success_rate * 100.0),
            allocated_mb = %format_args!("{:.1}", allocated_mb),
            resident_mb = %format_args!("{:.1}", resident_mb),
            active_mb = %format_args!("{:.1}", active_mb),
            retained_mb = %format_args!("{:.1}", retained_mb),
            fragmentation = %format_args!("{:.1}%", ((resident_mb - allocated_mb) / resident_mb * 100.0).max(0.0)),
            "Stats"
        );
    }
}

/// A subscriber that tracks packet transmission and loss statistics.
///
/// Clone this to share the same underlying counters across multiple uses.
#[derive(Clone)]
pub struct Subscriber {
    inner: Arc<StatsInner>,
}

impl Subscriber {
    /// Create a new stats subscriber and spawn a background reporting task.
    ///
    /// Reports statistics every `interval` to the tracing log.
    pub fn spawn(_interval: std::time::Duration) -> Self {
        let subscriber = Self {
            inner: Arc::new(StatsInner::new()),
        };

        // let inner = subscriber.inner.clone();
        // tokio::spawn(async move {
        //     let mut tick = tokio::time::interval(interval);
        //     loop {
        //         tick.tick().await;
        //         inner.report();
        //     }
        // });

        subscriber
    }

    /// Mark the start of an application-level request.
    pub fn start_request(&self) {
        self.inner
            .requests_in_progress
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Mark the completion of an application-level request.
    pub fn finish_request(&self, bytes_sent: u64, bytes_received: u64, is_error: bool) {
        self.inner
            .requests_in_progress
            .fetch_sub(1, Ordering::Relaxed);
        self.inner
            .requests_completed
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .app_bytes_sent
            .fetch_add(bytes_sent, Ordering::Relaxed);
        self.inner
            .app_bytes_received
            .fetch_add(bytes_received, Ordering::Relaxed);
        if is_error {
            self.inner.errors.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl s2n_quic_dc::event::Subscriber for Subscriber {
    type ConnectionContext = ();

    fn create_connection_context(
        &self,
        _meta: &api::ConnectionMeta,
        _info: &api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }
}
