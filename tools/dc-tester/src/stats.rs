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
    /// Total stream packets transmitted (including retransmissions)
    packets_transmitted: AtomicU64,
    /// Total stream packets lost
    packets_lost: AtomicU64,
    /// Total bytes in transmitted packets
    bytes_transmitted: AtomicU64,
    /// Total payload bytes in transmitted packets
    payload_bytes_transmitted: AtomicU64,
    /// Total bytes in lost packets
    bytes_lost: AtomicU64,
    /// Total payload bytes in lost packets
    payload_bytes_lost: AtomicU64,
    /// Number of retransmissions sent
    retransmissions: AtomicU64,
    /// Number of probes transmitted
    probes_transmitted: AtomicU64,
    /// Total stream packets received
    packets_received: AtomicU64,
    /// Total bytes in received packets
    bytes_received: AtomicU64,
    /// Total payload bytes in received packets
    payload_bytes_received: AtomicU64,
    /// Received packets that were retransmissions
    received_retransmissions: AtomicU64,
    /// Packets that were spuriously retransmitted (original was actually received)
    spurious_retransmissions: AtomicU64,
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
    /// Control packets transmitted
    control_packets_transmitted: AtomicU64,
    /// Control packets received
    control_packets_received: AtomicU64,
    /// Total bytes in control packets transmitted
    control_bytes_transmitted: AtomicU64,
    /// Total bytes in control packets received
    control_bytes_received: AtomicU64,
}

impl StatsInner {
    fn new() -> Self {
        Self {
            packets_transmitted: AtomicU64::new(0),
            packets_lost: AtomicU64::new(0),
            bytes_transmitted: AtomicU64::new(0),
            payload_bytes_transmitted: AtomicU64::new(0),
            bytes_lost: AtomicU64::new(0),
            payload_bytes_lost: AtomicU64::new(0),
            retransmissions: AtomicU64::new(0),
            probes_transmitted: AtomicU64::new(0),
            packets_received: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            payload_bytes_received: AtomicU64::new(0),
            received_retransmissions: AtomicU64::new(0),
            spurious_retransmissions: AtomicU64::new(0),
            requests_in_progress: AtomicU64::new(0),
            requests_completed: AtomicU64::new(0),
            app_bytes_sent: AtomicU64::new(0),
            app_bytes_received: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            control_packets_transmitted: AtomicU64::new(0),
            control_packets_received: AtomicU64::new(0),
            control_bytes_transmitted: AtomicU64::new(0),
            control_bytes_received: AtomicU64::new(0),
        }
    }

    fn report(&self) {
        let packets_tx = self.packets_transmitted.swap(0, Ordering::Relaxed);
        let packets_lost = self.packets_lost.swap(0, Ordering::Relaxed);
        let bytes_tx = self.bytes_transmitted.swap(0, Ordering::Relaxed);
        let payload_bytes_tx = self.payload_bytes_transmitted.swap(0, Ordering::Relaxed);
        let bytes_lost = self.bytes_lost.swap(0, Ordering::Relaxed);
        let payload_bytes_lost = self.payload_bytes_lost.swap(0, Ordering::Relaxed);
        let retransmissions = self.retransmissions.swap(0, Ordering::Relaxed);
        let probes_tx = self.probes_transmitted.swap(0, Ordering::Relaxed);
        let packets_rx = self.packets_received.swap(0, Ordering::Relaxed);
        let bytes_rx = self.bytes_received.swap(0, Ordering::Relaxed);
        let payload_bytes_rx = self.payload_bytes_received.swap(0, Ordering::Relaxed);
        let rx_retransmissions = self.received_retransmissions.swap(0, Ordering::Relaxed);
        let spurious = self.spurious_retransmissions.swap(0, Ordering::Relaxed);
        let requests_in_progress = self.requests_in_progress.load(Ordering::Relaxed);
        let requests_completed = self.requests_completed.swap(0, Ordering::Relaxed);
        let app_bytes_sent = self.app_bytes_sent.swap(0, Ordering::Relaxed);
        let app_bytes_received = self.app_bytes_received.swap(0, Ordering::Relaxed);
        let errors = self.errors.swap(0, Ordering::Relaxed);
        let control_packets_tx = self.control_packets_transmitted.swap(0, Ordering::Relaxed);
        let control_packets_rx = self.control_packets_received.swap(0, Ordering::Relaxed);
        let control_bytes_tx = self.control_bytes_transmitted.swap(0, Ordering::Relaxed);
        let control_bytes_rx = self.control_bytes_received.swap(0, Ordering::Relaxed);

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

        let loss_rate = if packets_tx > 0 {
            packets_lost as f64 / packets_tx as f64
        } else {
            0.0
        };

        let success = requests_completed.saturating_sub(errors);
        let success_rate = if requests_completed > 0 {
            success as f64 / requests_completed as f64
        } else {
            0.0
        };

        info!(
            packets_transmitted = packets_tx,
            packets_lost,
            loss_rate = %format_args!("{:.4}%", loss_rate * 100.0),
            bytes_transmitted = bytes_tx,
            payload_bytes_transmitted = payload_bytes_tx,
            bytes_lost,
            payload_bytes_lost,
            retransmissions,
            probes_transmitted = probes_tx,
            packets_received = packets_rx,
            bytes_received = bytes_rx,
            payload_bytes_received = payload_bytes_rx,
            received_retransmissions = rx_retransmissions,
            spurious_retransmissions = spurious,
            requests_in_progress,
            requests_completed,
            app_bytes_sent,
            app_bytes_received,
            errors,
            success_rate = %format_args!("{:.2}%", success_rate * 100.0),
            control_packets_transmitted = control_packets_tx,
            control_packets_received = control_packets_rx,
            control_bytes_transmitted = control_bytes_tx,
            control_bytes_received = control_bytes_rx,
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
    pub fn spawn(interval: std::time::Duration) -> Self {
        let subscriber = Self {
            inner: Arc::new(StatsInner::new()),
        };

        let inner = subscriber.inner.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tick.tick().await;
                inner.report();
            }
        });

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

    fn on_stream_packet_transmitted(
        &self,
        _context: &Self::ConnectionContext,
        _meta: &api::ConnectionMeta,
        event: &api::StreamPacketTransmitted,
    ) {
        self.inner
            .packets_transmitted
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .bytes_transmitted
            .fetch_add(event.packet_len as u64, Ordering::Relaxed);
        self.inner
            .payload_bytes_transmitted
            .fetch_add(event.payload_len as u64, Ordering::Relaxed);
        if event.is_retransmission {
            self.inner.retransmissions.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn on_stream_packet_lost(
        &self,
        _context: &Self::ConnectionContext,
        _meta: &api::ConnectionMeta,
        event: &api::StreamPacketLost,
    ) {
        self.inner.packets_lost.fetch_add(1, Ordering::Relaxed);
        self.inner
            .bytes_lost
            .fetch_add(event.packet_len as u64, Ordering::Relaxed);
        self.inner
            .payload_bytes_lost
            .fetch_add(event.payload_len as u64, Ordering::Relaxed);
    }

    fn on_stream_probe_transmitted(
        &self,
        _context: &Self::ConnectionContext,
        _meta: &api::ConnectionMeta,
        _event: &api::StreamProbeTransmitted,
    ) {
        self.inner
            .probes_transmitted
            .fetch_add(1, Ordering::Relaxed);
    }

    fn on_stream_packet_received(
        &self,
        _context: &Self::ConnectionContext,
        _meta: &api::ConnectionMeta,
        event: &api::StreamPacketReceived,
    ) {
        self.inner.packets_received.fetch_add(1, Ordering::Relaxed);
        self.inner
            .bytes_received
            .fetch_add(event.packet_len as u64, Ordering::Relaxed);
        self.inner
            .payload_bytes_received
            .fetch_add(event.payload_len as u64, Ordering::Relaxed);
        if event.is_retransmission {
            self.inner
                .received_retransmissions
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    fn on_stream_packet_spuriously_retransmitted(
        &self,
        _context: &Self::ConnectionContext,
        _meta: &api::ConnectionMeta,
        _event: &api::StreamPacketSpuriouslyRetransmitted,
    ) {
        self.inner
            .spurious_retransmissions
            .fetch_add(1, Ordering::Relaxed);
    }

    fn on_stream_control_packet_transmitted(
        &self,
        _context: &Self::ConnectionContext,
        _meta: &api::ConnectionMeta,
        event: &api::StreamControlPacketTransmitted,
    ) {
        self.inner
            .control_packets_transmitted
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .control_bytes_transmitted
            .fetch_add(event.packet_len as u64, Ordering::Relaxed);
    }

    fn on_stream_control_packet_received(
        &self,
        _context: &Self::ConnectionContext,
        _meta: &api::ConnectionMeta,
        event: &api::StreamControlPacketReceived,
    ) {
        self.inner
            .control_packets_received
            .fetch_add(1, Ordering::Relaxed);
        self.inner
            .control_bytes_received
            .fetch_add(event.packet_len as u64, Ordering::Relaxed);
    }
}
