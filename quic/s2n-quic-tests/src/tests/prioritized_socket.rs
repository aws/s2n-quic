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
/// Two sockets are bound to different ports. A real s2n-quic Server is started
/// with one as the rx socket and the other as the prioritized socket. A flood
/// thread sends packets to both ports.
///
/// The priority scheduling should cause the prioritized socket to receive
/// significantly more packets than the other.
#[tokio::test]
async fn prioritized_socket_scheduling_test() {
    let socket_low = syscall::bind_udp("127.0.0.1:0", false, false, false).unwrap();
    socket_low.set_nonblocking(true).unwrap();
    let low_addr = socket_low.local_addr().unwrap().as_socket().unwrap();

    let socket_high = syscall::bind_udp("127.0.0.1:0", false, false, false).unwrap();
    socket_high.set_nonblocking(true).unwrap();
    let high_addr = socket_high.local_addr().unwrap().as_socket().unwrap();

    let stats = StatsSubscriber::default();

    let io = s2n_quic::provider::io::tokio::Builder::default()
        .with_rx_socket(socket_low.into())
        .unwrap()
        .with_prioritized_socket(socket_high.into())
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

    // Flood both sockets from a separate thread
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
    let total_flood = flood_count.load(Ordering::Relaxed);

    eprintln!(
        "Flood packets sent: {}, Low priority rx: {}, High priority rx: {}",
        total_flood, socket_0_count, socket_1_count,
    );

    // The prioritized socket (index 1) should have received many packets
    assert!(socket_1_count > 0);

    // Even with a 1:1 send ratio, the prioritized socket should receive more
    // packets because the priority scheduling drains it first. The low-priority
    // socket only gets read when the prioritized socket momentarily has no data.
    assert!(socket_1_count > socket_0_count * 2);
}
