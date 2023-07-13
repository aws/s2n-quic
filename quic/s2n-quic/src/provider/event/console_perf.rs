// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::provider::event;
use core::time::Duration;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// An event subscriber that prints performance metrics to the console
///
/// # Examples
///
/// Enables the console perf event subscriber for the server,
/// configured to print metrics every 10 seconds
///
/// ```rust,ignore
/// # use std::{error::Error, time::Duration};
/// use s2n_quic::Server;
/// #
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn Error>> {
/// let server = Server::builder()
///     .with_event(event::console_perf::Subscriber::new(core::time::Duration::from_secs(10)))?
///     .start()?;
/// #
/// #    Ok(())
/// # }
/// ```
#[derive(Debug, Default)]
pub struct Subscriber {
    pub counters: Arc<Counters>,
    pub frequency: Duration,
    pub spawned: bool,
}

impl Subscriber {
    /// Create a new `console_perf::Subscriber` that
    /// prints metrics at the given `frequency`
    pub fn new(frequency: Duration) -> Self {
        Self {
            counters: Default::default(),
            frequency,
            spawned: false,
        }
    }

    /// Create a new `console_perf::Subscriber` that
    /// does not print to the console
    pub fn disabled() -> Self {
        Self {
            counters: Default::default(),
            frequency: Duration::MAX,
            spawned: true,
        }
    }

    fn spawn(&mut self) {
        if !self.spawned {
            self.spawned = true;
            let counters = self.counters.clone();
            let frequency = self.frequency;
            counters.print_header();
            tokio::spawn(async move {
                loop {
                    tokio::time::sleep(frequency).await;
                    counters.print(frequency);
                }
            });
        }
    }
}

impl event::Subscriber for Subscriber {
    type ConnectionContext = ();

    #[inline]
    fn create_connection_context(
        &mut self,
        _meta: &event::ConnectionMeta,
        _info: &event::ConnectionInfo,
    ) -> Self::ConnectionContext {
        self.spawn();
    }

    #[inline]
    fn on_tx_stream_progress(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &event::ConnectionMeta,
        event: &event::events::TxStreamProgress,
    ) {
        self.counters
            .send_progress
            .fetch_add(event.bytes as _, Ordering::Relaxed);
    }

    #[inline]
    fn on_rx_stream_progress(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &event::ConnectionMeta,
        event: &event::events::RxStreamProgress,
    ) {
        self.counters
            .receive_progress
            .fetch_add(event.bytes as _, Ordering::Relaxed);
    }

    #[inline]
    fn on_recovery_metrics(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &event::ConnectionMeta,
        event: &event::events::RecoveryMetrics,
    ) {
        self.counters
            .max_cwnd
            .fetch_max(event.congestion_window as _, Ordering::Relaxed);
        self.counters
            .max_bytes_in_flight
            .fetch_max(event.bytes_in_flight as _, Ordering::Relaxed);
        self.counters
            .max_rtt
            .fetch_max(event.latest_rtt.as_nanos() as _, Ordering::Relaxed);
        self.counters
            .max_smoothed_rtt
            .fetch_max(event.smoothed_rtt.as_nanos() as _, Ordering::Relaxed);
        self.counters
            .pto_count
            .fetch_max(event.pto_count as _, Ordering::Relaxed);
    }

    #[inline]
    fn on_packet_lost(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &event::ConnectionMeta,
        _event: &event::events::PacketLost,
    ) {
        self.counters.lost_packets.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn on_platform_event_loop_wakeup(
        &mut self,
        _meta: &event::events::EndpointMeta,
        _event: &event::events::PlatformEventLoopWakeup,
    ) {
        self.counters
            .event_loop_wakeup
            .fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn on_platform_event_loop_sleep(
        &mut self,
        _meta: &event::events::EndpointMeta,
        event: &event::events::PlatformEventLoopSleep,
    ) {
        self.counters.timeout.store(
            event.timeout.unwrap_or_default().as_nanos() as _,
            Ordering::Relaxed,
        );
    }

    #[inline]
    fn on_pacing_rate_updated(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &event::events::ConnectionMeta,
        event: &event::events::PacingRateUpdated,
    ) {
        self.counters
            .max_pacing_rate
            .fetch_max(event.bytes_per_second, Ordering::Relaxed);
    }

    #[inline]
    fn on_delivery_rate_sampled(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &event::events::ConnectionMeta,
        event: &event::events::DeliveryRateSampled,
    ) {
        self.counters.max_delivery_rate.fetch_max(
            event.rate_sample.delivery_rate_bytes_per_second,
            Ordering::Relaxed,
        );
    }
}

#[derive(Debug, Default)]
pub struct Counters {
    send_progress: AtomicU64,
    receive_progress: AtomicU64,
    max_cwnd: AtomicU64,
    max_bytes_in_flight: AtomicU64,
    max_rtt: AtomicU64,
    max_smoothed_rtt: AtomicU64,
    lost_packets: AtomicU64,
    event_loop_wakeup: AtomicU64,
    timeout: AtomicU64,
    pto_count: AtomicU64,
    max_pacing_rate: AtomicU64,
    max_delivery_rate: AtomicU64,
}

impl Counters {
    pub fn print_header(&self) {
        println!(
            "Tx Rate\t\
            Rx Rate\t\
            Max Cwnd\t\
            Max Inflight\t\
            Lost Packets\t\
            Wakeups\t\
            Duration\t\
            Max RTT\t\
            Max SRTT\t\
            PTO Count\t\
            Max Pacing Rate\t\
            Max Delivery Rate"
        );
    }

    pub fn print(&self, duration: Duration) {
        // The goodput of data transmitted to the peer
        let send_progress = self.send_progress.swap(0, Ordering::Relaxed);
        let send_rate = rate(send_progress, duration);
        // The goodput of data received from the peer
        let receive_progress = self.receive_progress.swap(0, Ordering::Relaxed);
        let receive_rate = rate(receive_progress, duration);
        // The maximum congestion window observed during the interval
        let max_cwnd = self.max_cwnd.swap(0, Ordering::Relaxed);
        let max_cwnd = bytes(max_cwnd);
        // The maximum amount of unacknowledged data in flight during the interval
        let max_bytes_in_flight = self.max_bytes_in_flight.swap(0, Ordering::Relaxed);
        let max_bytes_in_flight = bytes(max_bytes_in_flight);
        // The maximum round trip time observed during the interval
        let max_rtt = self.max_rtt.swap(0, Ordering::Relaxed);
        let max_rtt = Duration::from_nanos(max_rtt);
        // The maximum smoothed (weighted average) round trip time observed during the interval
        let max_smoothed_rtt = self.max_smoothed_rtt.swap(0, Ordering::Relaxed);
        let max_smoothed_rtt = Duration::from_nanos(max_smoothed_rtt);
        // The number of packets recorded lost during the interval
        let lost_packets = self.lost_packets.swap(0, Ordering::Relaxed);
        // The number of event loop wakeups recorded during the interval
        let wakeups = self.event_loop_wakeup.swap(0, Ordering::Relaxed);
        // The duration of the latest event loop wakeup
        let duration = self.timeout.swap(0, Ordering::Relaxed);
        let duration = Duration::from_nanos(duration);
        // The number of packet time out events observed during the interval
        let pto_count = self.pto_count.swap(0, Ordering::Relaxed);
        // The maximum rate at which packets are paced out observed during the interval
        let max_pacing_rate = self.max_pacing_rate.swap(0, Ordering::Relaxed);
        let max_pacing_rate = rate(max_pacing_rate, Duration::from_secs(1));
        // The maximum estimate of bandwidth observed during the interval. Only output for BBRv2
        let max_delivery_rate = self.max_delivery_rate.swap(0, Ordering::Relaxed);
        let max_delivery_rate = rate(max_delivery_rate, Duration::from_secs(1));
        println!(
            "{send_rate}\t\
            {receive_rate}\t\
            {max_cwnd}\t\
            {max_bytes_in_flight}\t\
            {lost_packets}\t\
            {wakeups}\t\
            {duration:?}\t\
            {max_rtt:?}\t\
            {max_smoothed_rtt:?}\t\
            {pto_count}\t\
            {max_pacing_rate}\t\
            {max_delivery_rate}",
        );
    }
}

fn rate(bytes: u64, duration: Duration) -> String {
    use humansize::{format_size, DECIMAL};

    let bits = bytes * 8;
    let value = format_size(bits, DECIMAL.space_after_value(false));
    let value = value.trim_end_matches('B');

    if duration == Duration::from_secs(1) {
        format!("{value}bps")
    } else {
        format!("{value}/{duration:?}")
    }
}

fn bytes(value: u64) -> String {
    use humansize::{format_size, DECIMAL};
    format_size(value, DECIMAL.space_after_value(false))
}
