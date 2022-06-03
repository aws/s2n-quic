// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::Result;
use bytes::Bytes;
use core::time::Duration;
use s2n_quic::{
    provider::event,
    stream::{ReceiveStream, SendStream},
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// Drains a receive stream
pub async fn handle_receive_stream(mut stream: ReceiveStream) -> Result<()> {
    let mut chunks = vec![Bytes::new(); 64];

    loop {
        let (len, is_open) = stream.receive_vectored(&mut chunks).await?;

        if !is_open {
            break;
        }

        for chunk in chunks[..len].iter_mut() {
            // discard chunks
            *chunk = Bytes::new();
        }
    }

    Ok(())
}

/// Sends a specified amount of data on a send stream
pub async fn handle_send_stream(mut stream: SendStream, len: u64) -> Result<()> {
    let mut chunks = vec![Bytes::new(); 64];

    //= https://tools.ietf.org/id/draft-banks-quic-performance-00#4.1
    //# Since the goal here is to measure the efficiency of the QUIC
    //# implementation and not any application protocol, the performance
    //# application layer should be as light-weight as possible.  To this
    //# end, the client and server application layer may use a single
    //# preallocated and initialized buffer that it queues to send when any
    //# payload needs to be sent out.
    let mut data = s2n_quic_core::stream::testing::Data::new(len);

    loop {
        match data.send(usize::MAX, &mut chunks) {
            Some(count) => {
                stream.send_vectored(&mut chunks[..count]).await?;
            }
            None => {
                stream.finish()?;
                break;
            }
        }
    }

    Ok(())
}

//= https://tools.ietf.org/id/draft-banks-quic-performance-00#2.3.1
//# Every stream opened by the client uses the first 8 bytes of the
//# stream data to encode a 64-bit unsigned integer in network byte order
//# to indicate the length of data the client wishes the server to
//# respond with.
pub async fn write_stream_size(stream: &mut SendStream, len: u64) -> Result<()> {
    let size = len.to_be_bytes();
    let chunk = Bytes::copy_from_slice(&size);
    stream.send(chunk).await?;
    Ok(())
}

pub async fn read_stream_size(stream: &mut ReceiveStream) -> Result<(u64, Bytes)> {
    let mut chunk = Bytes::new();
    let mut offset = 0;
    let mut id = [0u8; core::mem::size_of::<u64>()];

    while offset < id.len() {
        chunk = stream
            .receive()
            .await?
            .expect("every stream should be prefixed with the scenario ID");

        let needed_len = id.len() - offset;
        let len = chunk.len().min(needed_len);

        id[offset..offset + len].copy_from_slice(&chunk[..len]);
        offset += len;
        bytes::Buf::advance(&mut chunk, len);
    }

    let id = u64::from_be_bytes(id);

    Ok((id, chunk))
}

#[derive(Debug, structopt::StructOpt)]
pub struct Limits {
    /// The maximum bits/sec for each connection
    #[structopt(long, default_value = "150")]
    pub max_throughput: u64,

    /// The expected RTT in milliseconds
    #[structopt(long, default_value = "100")]
    pub expected_rtt: u64,
}

impl Limits {
    pub fn limits(&self) -> s2n_quic::provider::limits::Limits {
        let data_window = self.data_window();

        s2n_quic::provider::limits::Limits::default()
            .with_data_window(data_window)
            .unwrap()
            .with_max_send_buffer_size(data_window.min(u32::MAX as _) as _)
            .unwrap()
            .with_bidirectional_local_data_window(data_window)
            .unwrap()
            .with_unidirectional_data_window(data_window)
            .unwrap()
    }

    fn data_window(&self) -> u64 {
        s2n_quic_core::transport::parameters::compute_data_window(
            self.max_throughput,
            core::time::Duration::from_millis(self.expected_rtt),
            2,
        )
        .as_u64()
    }
}

#[derive(Debug, Default)]
pub struct Subscriber {
    pub counters: Arc<Counters>,
}

impl Subscriber {
    pub fn spawn(&self, frequency: Duration) {
        let counters = self.counters.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(frequency).await;
                counters.print(frequency);
            }
        });
    }
}

impl s2n_quic::provider::event::Subscriber for Subscriber {
    type ConnectionContext = ();

    #[inline]
    fn create_connection_context(
        &mut self,
        _meta: &event::ConnectionMeta,
        _info: &event::ConnectionInfo,
    ) -> Self::ConnectionContext {
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
}

impl Counters {
    pub fn print(&self, duration: Duration) {
        let send_progress = self.send_progress.swap(0, Ordering::Relaxed);
        let send_rate = rate(send_progress, duration);
        let receive_progress = self.receive_progress.swap(0, Ordering::Relaxed);
        let receive_rate = rate(receive_progress, duration);
        let max_cwnd = self.max_cwnd.swap(0, Ordering::Relaxed);
        let max_cwnd = bytes(max_cwnd);
        let max_bytes_in_flight = self.max_bytes_in_flight.swap(0, Ordering::Relaxed);
        let max_bytes_in_flight = bytes(max_bytes_in_flight);
        let max_rtt = self.max_rtt.swap(0, Ordering::Relaxed);
        let max_rtt = Duration::from_nanos(max_rtt);
        let max_smoothed_rtt = self.max_smoothed_rtt.swap(0, Ordering::Relaxed);
        let max_smoothed_rtt = Duration::from_nanos(max_smoothed_rtt);
        let lost_packets = self.lost_packets.swap(0, Ordering::Relaxed);
        let wakeups = self.event_loop_wakeup.swap(0, Ordering::Relaxed);
        let duration = self.timeout.swap(0, Ordering::Relaxed);
        let duration = Duration::from_nanos(duration);
        let pto_count = self.pto_count.swap(0, Ordering::Relaxed);
        eprintln!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{:?}\t{:?}\t{:?}\t{}",
            send_rate,
            receive_rate,
            max_cwnd,
            max_bytes_in_flight,
            lost_packets,
            wakeups,
            duration,
            max_rtt,
            max_smoothed_rtt,
            pto_count,
        );
    }
}

fn rate(bytes: u64, duration: Duration) -> String {
    use humansize::{file_size_opts as opts, FileSize};

    let opts = opts::FileSizeOpts {
        space: false,
        ..humansize::file_size_opts::DECIMAL
    };

    let bits = bytes * 8;
    let value = bits.file_size(opts).unwrap();
    let value = value.trim_end_matches('B');

    if duration == Duration::from_secs(1) {
        format!("{}bps", value)
    } else {
        format!("{}/{:?}", value, duration)
    }
}

fn bytes(value: u64) -> String {
    use humansize::{file_size_opts as opts, FileSize};

    let opts = opts::FileSizeOpts {
        space: false,
        ..humansize::file_size_opts::DECIMAL
    };

    value.file_size(opts).unwrap()
}
