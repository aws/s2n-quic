// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests that the prioritized socket is drained before the other socket
//! under concurrent load.

use s2n_quic::Server;
use s2n_quic_core::{
    crypto::tls::testing::certificates,
    event::{self, api},
};
use s2n_quic_platform::syscall;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};

/// A subscriber that tracks per-socket rx packet counts via PlatformRxSocketStats.
#[derive(Debug, Default, Clone)]
struct StatsSubscriber {
    socket_counts: Arc<[AtomicU64; 2]>,
}

impl event::Subscriber for StatsSubscriber {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &api::ConnectionMeta,
        _info: &api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_platform_rx_socket_stats(
        &mut self,
        _meta: &api::EndpointMeta,
        event: &api::PlatformRxSocketStats,
    ) {
        let idx = if event.is_prioritized { 1 } else { 0 };
        self.socket_counts[idx].fetch_add(event.count as u64, Ordering::Relaxed);
    }
}

/// Verifies that the prioritized socket is drained before the other socket
/// under concurrent load on both sockets.
///
/// A small internal receive buffer is used so the ring buffer becomes the
/// bottleneck. When both sockets have data, the scheduling determines which
/// socket fills the limited ring space. Since the high-priority socket is
/// always drained first, it should receive significantly more packets.
#[tokio::test]
async fn prioritized_socket_scheduling_test() {
    let socket_low = syscall::bind_udp("127.0.0.1:0", false, false, false).unwrap();
    socket_low.set_nonblocking(true).unwrap();
    let low_addr = socket_low.local_addr().unwrap().as_socket().unwrap();

    let socket_high = syscall::bind_udp("127.0.0.1:0", false, false, false).unwrap();
    socket_high.set_nonblocking(true).unwrap();
    let high_addr = socket_high.local_addr().unwrap().as_socket().unwrap();

    let stats = StatsSubscriber::default();

    // Use a small internal receive buffer (ring buffer) so it becomes the
    // bottleneck. This forces contention between the two sockets for ring
    // space, making the priority scheduling observable.
    let io = s2n_quic::provider::io::tokio::Builder::default()
        .with_rx_socket(socket_low.into())
        .unwrap()
        .with_prioritized_socket(socket_high.into())
        .unwrap()
        .with_internal_recv_buffer_size(4096)
        .unwrap()
        .build()
        .unwrap();

    let server = Server::builder()
        .with_io(io)
        .unwrap()
        .with_tls((certificates::CERT_PEM, certificates::KEY_PEM))
        .unwrap()
        .with_event(stats.clone())
        .unwrap()
        .start()
        .unwrap();

    // Flood both sockets equally from a separate thread
    let cancel = Arc::new(AtomicBool::new(false));
    let flood_count = Arc::new(AtomicU64::new(0));

    let flood_thread = {
        let cancel = cancel.clone();
        let count = flood_count.clone();
        std::thread::spawn(move || {
            let sender = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            let packet = s2n_quic_core::crypto::initial::EXAMPLE_CLIENT_INITIAL_PROTECTED_PACKET;
            while !cancel.load(Ordering::Relaxed) {
                // Send 1:1 ratio to both sockets
                let _ = sender.send_to(&packet, high_addr);
                let _ = sender.send_to(&packet, low_addr);
                count.fetch_add(2, Ordering::Relaxed);
            }
        })
    };

    // Let the flood run for 3 seconds
    tokio::time::sleep(Duration::from_secs(3)).await;

    // Stop the flood and shut down the server
    cancel.store(true, Ordering::Relaxed);
    flood_thread.join().expect("flood thread panicked");

    // Wait briefly for events to propagate
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Shut down the server by dropping it
    drop(server);

    let socket_0_count = stats.socket_counts[0].load(Ordering::Relaxed);
    let socket_1_count = stats.socket_counts[1].load(Ordering::Relaxed);
    let total_flood_count = flood_count.load(Ordering::Relaxed);

    eprintln!(
        "Flood packets sent: {}, Low priority rx: {}, High priority rx: {}",
        total_flood_count, socket_0_count, socket_1_count,
    );

    // The flood thread must have sent packets for the test to be meaningful.
    assert!(total_flood_count > 0);

    // Both sockets should have received some packets, proving that the flood
    // reached both and that real contention occurred.
    assert!(socket_0_count > 0);
    assert!(socket_1_count > 0);

    // The high-priority socket should receive the majority of packets.
    // With a 1:1 send ratio and a small ring buffer, priority scheduling
    // biases toward the high-priority socket.
    let total_rx = socket_0_count + socket_1_count;
    let socket_1_count_pct = (socket_1_count * 100) / total_rx;
    // High priority socket should receive more than 60% of packets that were received in total.
    assert!(socket_1_count_pct >= 60);
}
