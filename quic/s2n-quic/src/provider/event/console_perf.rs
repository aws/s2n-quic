// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::provider::event;
use core::time::Duration;
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// An event subscriber that prints performance metrics to the console via STDOUT
///
/// NOTE: The ordering and format of the output is subject to change and
/// should not be relied on to remain consistent over time.
///
/// # Examples
///
/// Enables the console perf event subscriber for the server,
/// spawning a tokio task to print the metrics every second.
///
/// ```rust,ignore
/// use std::{error::Error, time::Duration};
/// use s2n_quic::Server;
///
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn Error>> {
///     let subscriber = event::console_perf::Builder::default()
///                        .with_format(event::console_perf::Format::TSV)
///                        .build();
///
///     let frequency = Duration::from_secs(1);
///     let subscriber = subscriber.clone();
///     tokio::spawn(async move {
///         loop {
///               tokio::time::sleep(frequency).await;
///               subscriber.print();
///         }
///      });
///
///     let server = Server::builder()
///       .with_event(subscriber)?
///       .start()?;
///
///     Ok(())
/// }
/// ```
#[derive(Clone, Debug)]
pub struct Subscriber {
    counters: Arc<Counters>,
    format: Format,
    print_header: bool,
    init: bool,
}

#[non_exhaustive]
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Format {
    /// Tab separated values with suffixes indicating the data units
    TSV,
    /// Tab separated values with no suffixes
    ///
    /// Rates are output as bits per second, sizes are output as number of bytes
    /// and durations are output as microseconds
    TSV_RAW,
}

impl Format {
    fn print_suffix(&self) -> bool {
        match self {
            Self::TSV => true,
            Self::TSV_RAW => false,
        }
    }
}

pub struct Builder {
    format: Format,
    print_header: bool,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            format: Format::TSV,
            print_header: true,
        }
    }
}

impl Builder {
    /// Sets the format that performance metrics will be printed in
    pub fn with_format(mut self, format: Format) -> Self {
        self.format = format;
        self
    }

    /// Enables or disables printing of a header containing the name of each
    /// performance metric. By default the header is printed.
    pub fn with_header(mut self, print_header: bool) -> Self {
        self.print_header = print_header;
        self
    }

    /// Builds the [`Subscriber`]
    pub fn build(self) -> Subscriber {
        Subscriber {
            counters: Default::default(),
            print_header: self.print_header,
            format: self.format,
            init: false,
        }
    }
}

impl Subscriber {
    /// Prints the current performance metrics to the console via STDOUT
    pub fn print(&mut self) {
        if self.print_header {
            self.counters.print_header(self.format);
            self.print_header = false;
        }

        self.counters.print(self.format);
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
        if !self.init {
            // Initialize the counters.last_updated_micros with the current
            // time if it has not already been initialized
            self.init = true;
            self.counters.last_updated_micros.store(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_micros() as u64,
                Ordering::Relaxed,
            );
        }
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
struct Counters {
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
    last_updated_micros: AtomicU64,
}

impl Counters {
    fn print_header(&self, format: Format) {
        match format {
            Format::TSV | Format::TSV_RAW => {
                // NOTE: this format is subject to change
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
        }
    }

    fn print(&self, format: Format) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Clock may have gone backwards");
        let last_updated_micros = self
            .last_updated_micros
            .swap(now.as_micros() as u64, Ordering::Relaxed);
        let duration = now - Duration::from_micros(last_updated_micros);
        // The goodput of data transmitted to the peer
        let send_progress = self.send_progress.swap(0, Ordering::Relaxed);
        let send_rate = rate(send_progress, duration, format);
        // The goodput of data received from the peer
        let receive_progress = self.receive_progress.swap(0, Ordering::Relaxed);
        let receive_rate = rate(receive_progress, duration, format);
        // The maximum congestion window observed during the interval
        let max_cwnd = self.max_cwnd.swap(0, Ordering::Relaxed);
        let max_cwnd = bytes(max_cwnd, format);
        // The maximum amount of unacknowledged data in flight during the interval
        let max_bytes_in_flight = self.max_bytes_in_flight.swap(0, Ordering::Relaxed);
        let max_bytes_in_flight = bytes(max_bytes_in_flight, format);
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
        let max_pacing_rate = rate(max_pacing_rate, Duration::from_secs(1), format);
        // The maximum estimate of bandwidth observed during the interval. Only output for BBRv2
        let max_delivery_rate = self.max_delivery_rate.swap(0, Ordering::Relaxed);
        let max_delivery_rate = rate(max_delivery_rate, Duration::from_secs(1), format);

        match format {
            Format::TSV => {
                // NOTE: this format is subject to change
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
            Format::TSV_RAW => {
                // NOTE: this format is subject to change
                println!(
                    "{send_rate}\t\
                    {receive_rate}\t\
                    {max_cwnd}\t\
                    {max_bytes_in_flight}\t\
                    {lost_packets}\t\
                    {wakeups}\t\
                    {duration_micros}\t\
                    {max_rtt_micros}\t\
                    {max_smoothed_rtt_micros}\t\
                    {pto_count}\t\
                    {max_pacing_rate}\t\
                    {max_delivery_rate}",
                    duration_micros = duration.as_micros(),
                    max_rtt_micros = max_rtt.as_micros(),
                    max_smoothed_rtt_micros = max_smoothed_rtt.as_micros()
                );
            }
        }
    }
}

fn rate(bytes: u64, duration: Duration, format: Format) -> String {
    let per_second = Duration::from_secs(1).as_nanos() as f64 / duration.as_nanos() as f64;
    let bits_per_second = (bytes as f64 * 8.0 * per_second) as u64;

    if format.print_suffix() {
        use humansize::{format_size, DECIMAL};
        let value = format_size(bits_per_second, DECIMAL.space_after_value(false));
        let value = value.trim_end_matches('B');

        format!("{value}bps")
    } else {
        format!("{bits_per_second}")
    }
}

fn bytes(value: u64, format: Format) -> String {
    if format.print_suffix() {
        use humansize::{format_size, DECIMAL};
        format_size(value, DECIMAL.space_after_value(false))
    } else {
        value.to_string()
    }
}
