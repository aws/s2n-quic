// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::stream::{socket::Protocol, testing::bind_pair};
use std::sync::atomic::AtomicBool;
use tokio::net::UdpSocket;

const JUMBO_MTU: u16 = 8840;

/// Delay successful probes until a stream has opened, reproducing streams that snapshot
/// the base MTU before discovery completes. Optionally drop all oversized probes.
async fn check_stream_mtu(
    server_mtu: Option<u16>,
    client_mtu: Option<u16>,
    offload: usize,
    drop_probes: bool,
) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let map = || {
            Map::new(
                Signer::new(b"default"),
                50_000,
                false,
                StdClock::default(),
                NoopSubscriber,
            )
        };
        let server_events = MtuRecorder::default();
        let client_events = MtuRecorder::default();
        let server_map = map();
        let client_map = map();
        let mut server_builder = server::Builder::default()
            .with_thread_count(offload)
            .with_event_subscriber(server_events.clone());
        if let Some(mtu) = server_mtu {
            server_builder = server_builder.with_mtu(mtu);
        }
        let server = server_builder
            .start(
                "127.0.0.1:0".parse().unwrap(),
                TestTlsProvider {},
                NoopSubscriber,
                server_map.clone(),
            )
            .await
            .unwrap();
        let server_addr = server.local_addr();

        let mut client_builder =
            client::Builder::default().with_event_subscriber(client_events.clone());
        if let Some(mtu) = client_mtu {
            client_builder = client_builder.with_mtu(mtu);
        }
        let client = client_builder
            .start(
                "127.0.0.1:0".parse().unwrap(),
                client_map.clone(),
                TestTlsProvider {},
                NoopSubscriber,
                crate::testing::server_name(),
            )
            .unwrap();

        let relay = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let relay_addr = relay.local_addr().unwrap();
        let base = DEFAULT_BASE_MTU - 28; // IPv4 and UDP headers
        let allow_probes = Arc::new(AtomicBool::new(false));
        let allow = allow_probes.clone();
        let dropped = Arc::new(AtomicUsize::new(0));
        let dropped_probes = dropped.clone();
        let cancellation = tokio_util::sync::CancellationToken::new();
        let _relay_guard = cancellation.clone().drop_guard();
        tokio::spawn(async move {
            let mut buf = vec![0; u16::MAX as usize];
            let mut client_addr = None;
            loop {
                let (len, from) = tokio::select! {
                    _ = cancellation.cancelled() => return,
                    packet = relay.recv_from(&mut buf) => packet.unwrap(),
                };
                let to = if from == server_addr {
                    client_addr.unwrap()
                } else {
                    client_addr = Some(from);
                    server_addr
                };
                if len > base as usize && (drop_probes || !allow.load(Ordering::Relaxed)) {
                    dropped_probes.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                relay.send_to(&buf[..len], to).await.unwrap();
            }
        });

        let (client, server) = bind_pair(
            Protocol::Udp,
            "127.0.0.1:0".parse().unwrap(),
            client,
            server,
        );
        let (client_stream, server_stream) = tokio::join!(
            client.connect(
                relay_addr,
                server.acceptor_addr().unwrap(),
                crate::testing::server_name(),
            ),
            server.accept(),
        );
        let mut client_stream = client_stream.unwrap();
        let mut server_stream = server_stream.unwrap().0;
        let initial = |mtu: Option<u16>| mtu.unwrap_or(DEFAULT_BASE_MTU).min(DEFAULT_BASE_MTU) - 28;
        assert_eq!(client_stream.max_datagram_size(), initial(client_mtu));
        assert_eq!(server_stream.max_datagram_size(), initial(server_mtu));
        allow_probes.store(true, Ordering::Relaxed);

        let expected = |mtu: Option<u16>| {
            if drop_probes {
                base
            } else {
                mtu.unwrap_or(DEFAULT_BASE_MTU).min(DEFAULT_MTU) - 28
            }
        };
        // Drive both send workers while probes finish. Inspect the actual packet-sizing state,
        // not just the handshake events or the secret map.
        let payload = vec![0x5a; 32 * 1024];
        loop {
            let (written, received) = tokio::join!(
                async { client_stream.write_all_from(&mut &payload[..]).await },
                async {
                    let mut buf = vec![0; payload.len()];
                    tokio::io::AsyncReadExt::read_exact(&mut server_stream, &mut buf)
                        .await
                        .unwrap();
                    buf
                },
            );
            written.unwrap();
            assert_eq!(received, payload);
            let (written, received) = tokio::join!(
                async { server_stream.write_all_from(&mut &payload[..]).await },
                async {
                    let mut buf = vec![0; payload.len()];
                    tokio::io::AsyncReadExt::read_exact(&mut client_stream, &mut buf)
                        .await
                        .unwrap();
                    buf
                },
            );
            written.unwrap();
            assert_eq!(received, payload);
            if server_events.complete.load(Ordering::Relaxed)
                && client_events.complete.load(Ordering::Relaxed)
                && server_stream.max_datagram_size() == expected(server_mtu)
                && client_stream.max_datagram_size() == expected(client_mtu)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        for (stream, map, events, mtu) in [
            (&client_stream, client_map, client_events, client_mtu),
            (&server_stream, server_map, server_events, server_mtu),
        ] {
            assert_eq!(events.max_mtu.load(Ordering::Relaxed), expected(mtu));
            let entry = map.get_by_id_untracked(stream.path_secret_id()).unwrap();
            assert_eq!(entry.max_datagram_size(), expected(mtu));
        }
        if drop_probes {
            assert!(dropped.load(Ordering::Relaxed) > 0);
        }
    })
    .await
    .expect("MTU probing and bidirectional stream transfer should complete");
}

#[tokio::test]
async fn default_stream_mtu() {
    check_stream_mtu(None, None, 0, false).await;
}

#[tokio::test]
async fn lower_stream_mtu() {
    check_stream_mtu(Some(1280), Some(1280), 0, false).await;
}

#[tokio::test]
async fn stream_mtu_capped_to_buffer_capacity() {
    check_stream_mtu(Some(u16::MAX), Some(u16::MAX), 0, false).await;
}

#[tokio::test]
async fn jumbo_stream_mtu() {
    check_stream_mtu(Some(JUMBO_MTU), Some(JUMBO_MTU), 0, false).await;
}

#[tokio::test]
async fn jumbo_stream_mtu_with_offload() {
    check_stream_mtu(Some(JUMBO_MTU), Some(JUMBO_MTU), 2, false).await;
}

#[tokio::test]
async fn server_only_jumbo_stream_mtu() {
    check_stream_mtu(Some(JUMBO_MTU), None, 0, false).await;
}

#[tokio::test]
async fn client_only_jumbo_stream_mtu() {
    check_stream_mtu(None, Some(JUMBO_MTU), 0, false).await;
}

#[tokio::test]
async fn lost_mtu_probes_keep_streams_at_base_mtu() {
    check_stream_mtu(Some(JUMBO_MTU), Some(JUMBO_MTU), 0, true).await;
}
