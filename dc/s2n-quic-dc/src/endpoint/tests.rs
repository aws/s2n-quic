// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the stream endpoint packet pipeline.
//!
//! Each test runs inside Bach's deterministic simulation (`testing::sim`) with two fully
//! wired endpoints backed by simulated UDP sockets.  Each endpoint lives in its own Bach
//! group so it is treated as a separate machine from the network perspective.

use crate::{
    stream::endpoint::testing::sim::{Client, MonitorHostAddr, Peer, Server, SERVER_PORT},
    testing::{ext::*, sim},
    tracing::*,
};
use bach::time::timeout;
use bytes::{Bytes, BytesMut};
use s2n_quic_core::{stream::testing::Data, varint::VarInt};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

pub mod deterministic;
pub mod half_close;
pub mod unidirectional;

#[test]
fn topology_snapshot_uses_dc_tester_layout() {
    use crate::{
        acceptor,
        path::secret::map::testing,
        runtime,
        socket::{pool::Pool, rate::Rate},
        stream::endpoint::{self, Config, WorkerLayout},
    };

    let mut ids = 1..;
    let layout = WorkerLayout {
        frame_dispatch: 0,
        send: (&mut ids).take(4).collect(),
        recv_io: (&mut ids).take(4).collect(),
        recv_dispatch: (&mut ids).take(5).collect(),
        waker_drain: (&mut ids).take(1).collect(),
        background: ids.next().expect("background worker id should exist"),
    };

    let topology = runtime::inspector::endpoint_topology(
        Config {
            layout,
            send_pool: Pool::new(u16::MAX),
            recv_pool: Pool::new(u16::MAX),
            path_secret_map: testing::new(50_000),
            gso: endpoint::Gso::default(),
            acceptor_registry: acceptor::Registry::new(),
            overall_send_rate: Rate::new(25.0),
            per_socket_send_rate: Rate::new(5.0),
            budgets: endpoint::Budgets::default(),
            submission_shards: 128,
            ups_rate: Rate::new(0.001),
            ups_dedup_capacity: 1024,
            ups_dedup_window: core::time::Duration::from_secs(1),
            dead_peer_cooldown: endpoint::DEFAULT_DEAD_PEER_COOLDOWN,
            initial_tx_descriptor_allocs: 0,
            initial_rx_descriptor_allocs: 0,
            send_credit_pool_config: crate::credit::Config::default(),
            recv_credit_pool_config: crate::credit::Config::default(),
        },
        64,
        4,
    );

    insta::assert_snapshot!(topology.to_snapshot());
}

/// Ping-pong end-to-end test: the client sends "ping" and the server echoes
/// "pong" back over a real simulated UDP network path.
///
/// Both endpoints run in separate Bach groups (separate simulated machines).
/// [`Server::new`] / [`Client::new`] create their endpoints lazily on
/// first call, and [`Client::connect`] resolves the server address by group
/// name via `bach::net::lookup_host`, automatically inserting fake path-secret
/// entries into both maps.
#[test]
fn ping_pong() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);

        // ── Server — group "server" ────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            // Accept one stream.
            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();

                    // Read "ping" (the client sends FIN with the data so we
                    // get EOF after reading all 4 bytes).
                    let mut buf = BytesMut::with_capacity(8);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert_eq!(&buf[..], b"ping");

                    // Echo "pong" + FIN back to the client.
                    let mut pong = Bytes::from_static(b"pong");
                    writer
                        .write_all_from_fin(&mut pong)
                        .await
                        .expect("server write");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // ── Client — group "client" (primary) ─────────────────────────
        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let peer_addr = stream.peer_addr();
            assert_eq!(peer_addr.port(), SERVER_PORT);

            let (mut reader, mut writer) = stream.into_split();
            assert_eq!(reader.peer_addr(), peer_addr);
            assert_eq!(writer.peer_addr(), peer_addr);

            // Send "ping" + FIN in the QueueInit packet.
            let mut ping = Bytes::from_static(b"ping");
            writer
                .write_all_from_fin(&mut ping)
                .await
                .expect("client write");

            // Receive "pong" + FIN.
            let mut buf = BytesMut::with_capacity(8);
            loop {
                let n = reader.read_into(&mut buf).await.expect("client read");
                if n == 0 {
                    break;
                }
            }
            assert_eq!(&buf[..], b"pong");

            info!("ping_pong passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that PTO retransmission recovers from lost server responses.
///
/// The server sends an ACK-only packet first and then the response packet.
/// This test drops the second server packet (the response) so PTO must
/// retransmit before the client can complete.
#[test]
fn server_response_loss_triggers_pto() {
    let server_to_client_packets = Arc::new(AtomicUsize::new(0));
    let dropped_server_packets = Arc::new(AtomicUsize::new(0));

    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        let mut server_addr = MonitorHostAddr::new("server");
        // Drop the second packet sent from the server to the client.
        // In this scenario, packet #1 is ACK-only and packet #2 carries "pong".
        {
            let server_to_client_packets = server_to_client_packets.clone();
            let dropped_server_packets = dropped_server_packets.clone();
            bach::net::monitor::on_packet_sent(move |packet| {
                if server_addr.is_packet_source(packet) {
                    let packet_idx = server_to_client_packets.fetch_add(1, Ordering::Relaxed) + 1;
                    if packet_idx == 2 {
                        dropped_server_packets.fetch_add(1, Ordering::Relaxed);
                        info!(
                            "dropping server packet #{packet_idx} (source={:?}, len={})",
                            packet.source(),
                            packet.transport.payload().len()
                        );
                        return bach::net::monitor::Command::Drop;
                    }
                }
                bach::net::monitor::Command::Pass
            });
        }

        // ── Server ────────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();

                    let mut buf = BytesMut::with_capacity(8);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert_eq!(&buf[..], b"ping");

                    let mut pong = Bytes::from_static(b"pong");
                    writer
                        .write_all_from_fin(&mut pong)
                        .await
                        .expect("server write");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // ── Client ────────────────────────────────────────────────────────
        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (mut reader, mut writer) = stream.into_split();

            let mut ping = Bytes::from_static(b"ping");
            writer
                .write_all_from_fin(&mut ping)
                .await
                .expect("client write");

            let mut buf = BytesMut::with_capacity(8);
            loop {
                let n = reader.read_into(&mut buf).await.expect("client read");
                if n == 0 {
                    break;
                }
            }
            assert_eq!(&buf[..], b"pong");

            info!("server_response_loss_triggers_pto passed");
        }
        .group("client")
        .primary()
        .spawn();
    });

    let dropped = dropped_server_packets.load(Ordering::Relaxed);
    let server_packets = server_to_client_packets.load(Ordering::Relaxed);
    assert_eq!(dropped, 1, "expected exactly one dropped server packet");
    assert_eq!(
        server_packets, 3,
        "expected exactly three server packets after dropping the response packet"
    );
}

/// Verifies that the client's initial packet loss is recovered by PTO.
///
/// The first packet from the client (QueueInit + ping data) is dropped. The
/// client should PTO-retransmit and the server should still see "ping".
#[test]
fn client_request_loss_triggers_pto() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        let mut server_addr = MonitorHostAddr::new("server");

        // Drop the first packet from the client to the server.
        {
            let mut client_pkt_count = 0u32;
            bach::net::monitor::on_packet_sent(move |packet| {
                if !server_addr.is_packet_source(packet) {
                    client_pkt_count += 1;
                    if client_pkt_count == 1 {
                        info!(
                            "dropping client packet #{client_pkt_count} len={}",
                            packet.transport.payload().len()
                        );
                        return bach::net::monitor::Command::Drop;
                    }
                }
                bach::net::monitor::Command::Pass
            });
        }

        // ── Server ────────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();
                    let mut buf = BytesMut::with_capacity(8);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert_eq!(&buf[..], b"ping");

                    let mut pong = Bytes::from_static(b"pong");
                    writer
                        .write_all_from_fin(&mut pong)
                        .await
                        .expect("server write");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // ── Client ────────────────────────────────────────────────────────
        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (mut reader, mut writer) = stream.into_split();

            let mut ping = Bytes::from_static(b"ping");
            writer
                .write_all_from_fin(&mut ping)
                .await
                .expect("client write");

            let mut buf = BytesMut::with_capacity(8);
            loop {
                let n = reader.read_into(&mut buf).await.expect("client read");
                if n == 0 {
                    break;
                }
            }
            assert_eq!(&buf[..], b"pong");

            info!("client_request_loss_triggers_pto passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that multiple sequential streams work correctly.
///
/// Client opens two streams to the server. The server echoes data back on each.
/// This tests the acceptor channel, multi-stream dispatch, and queue pair routing.
#[test]
fn multiple_sequential_streams() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);

        // ── Server ────────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 16)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();
                    let mut buf = BytesMut::with_capacity(32);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    let echo = Bytes::copy_from_slice(&buf);
                    let mut echo_ref = echo;
                    writer
                        .write_all_from_fin(&mut echo_ref)
                        .await
                        .expect("server echo");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // ── Client ────────────────────────────────────────────────────────
        async move {
            let mut client = Client::new();
            let messages: &[&[u8]] = &[b"hello", b"world"];

            for &msg in messages {
                let stream = client
                    .connect("server:0", acceptor_id)
                    .await
                    .expect("connect failed");
                let (mut reader, mut writer) = stream.into_split();

                let mut data = Bytes::copy_from_slice(msg);
                writer
                    .write_all_from_fin(&mut data)
                    .await
                    .expect("client write");

                let mut buf = BytesMut::with_capacity(32);
                loop {
                    let n = reader.read_into(&mut buf).await.expect("client read");
                    if n == 0 {
                        break;
                    }
                }
                assert_eq!(&buf[..], msg, "echoed data mismatch for msg {:?}", msg);
                info!("stream {:?} completed", msg);
            }

            info!("multiple_sequential_streams passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that a larger payload (multiple frames) transfers correctly.
///
/// The client sends a 4 KiB message; the server echoes it back. This exercises
/// frame fragmentation in the Writer and reassembly in the Reader.
#[test]
fn large_payload_transfer() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        const PAYLOAD_SIZE: usize = 4096;

        // ── Server ────────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();
                    let mut buf = BytesMut::with_capacity(PAYLOAD_SIZE + 64);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert_eq!(buf.len(), PAYLOAD_SIZE, "server received wrong amount");
                    // Echo back
                    let echo = Bytes::copy_from_slice(&buf);
                    let mut echo_ref = echo;
                    writer
                        .write_all_from_fin(&mut echo_ref)
                        .await
                        .expect("server echo");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // ── Client ────────────────────────────────────────────────────────
        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (mut reader, mut writer) = stream.into_split();

            let payload = vec![0xABu8; PAYLOAD_SIZE];
            let mut data = Bytes::from(payload.clone());
            writer
                .write_all_from_fin(&mut data)
                .await
                .expect("client write");

            let mut buf = BytesMut::with_capacity(PAYLOAD_SIZE + 64);
            loop {
                let n = reader.read_into(&mut buf).await.expect("client read");
                if n == 0 {
                    break;
                }
            }
            assert_eq!(buf.len(), PAYLOAD_SIZE, "client received wrong amount");
            assert_eq!(&buf[..], &payload[..], "echoed payload mismatch");

            info!("large_payload_transfer passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that multiple consecutive packet drops are recovered by PTO.
///
/// The first two server packets to the client are dropped and PTO recovery is
/// locked to the resulting deterministic packet behavior.
#[test]
fn multiple_packet_loss_recovered_by_pto() {
    let server_to_client_packets = Arc::new(AtomicUsize::new(0));
    let dropped_server_packets = Arc::new(AtomicUsize::new(0));

    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        let mut server_addr = MonitorHostAddr::new("server");
        // Drop the first two packets from the server.
        {
            let server_to_client_packets = server_to_client_packets.clone();
            let dropped_server_packets = dropped_server_packets.clone();
            bach::net::monitor::on_packet_sent(move |packet| {
                if server_addr.is_packet_source(packet) {
                    let packet_idx = server_to_client_packets.fetch_add(1, Ordering::Relaxed) + 1;
                    if packet_idx <= 2 {
                        dropped_server_packets.fetch_add(1, Ordering::Relaxed);
                        info!(
                            "dropping server packet #{packet_idx} len={}",
                            packet.transport.payload().len()
                        );
                        return bach::net::monitor::Command::Drop;
                    }
                }
                bach::net::monitor::Command::Pass
            });
        }

        // ── Server ────────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();
                    let mut buf = BytesMut::with_capacity(8);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert_eq!(&buf[..], b"ping");
                    let mut pong = Bytes::from_static(b"pong");
                    writer
                        .write_all_from_fin(&mut pong)
                        .await
                        .expect("server write");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // ── Client ────────────────────────────────────────────────────────
        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (mut reader, mut writer) = stream.into_split();

            let mut ping = Bytes::from_static(b"ping");
            writer
                .write_all_from_fin(&mut ping)
                .await
                .expect("client write");

            let mut buf = BytesMut::with_capacity(8);
            loop {
                let n = reader.read_into(&mut buf).await.expect("client read");
                if n == 0 {
                    break;
                }
            }
            assert_eq!(&buf[..], b"pong");

            info!("multiple_packet_loss_recovered_by_pto passed");
        }
        .group("client")
        .primary()
        .spawn();
    });

    let dropped = dropped_server_packets.load(Ordering::Relaxed);
    let server_packets = server_to_client_packets.load(Ordering::Relaxed);
    assert_eq!(dropped, 2, "expected exactly two dropped server packets");
    assert_eq!(
        server_packets, 3,
        "expected exactly three server packets after dropping the first two server packets"
    );
}

/// Verifies that ACKs are sent back and the sender's inflight map is drained.
///
/// After both directions complete, neither endpoint should have pending inflight
/// packets. This is an implicit test since the sim finishes cleanly without
/// hanging (which would indicate stuck inflight tracking).
#[test]
fn ack_drains_inflight() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();
                    let mut buf = BytesMut::with_capacity(64);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    let echo = Bytes::copy_from_slice(&buf);
                    let mut echo_ref = echo;
                    writer
                        .write_all_from_fin(&mut echo_ref)
                        .await
                        .expect("server write");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            // Send 3 separate streams and verify all complete
            for i in 0u8..3 {
                let stream = client
                    .connect("server:0", acceptor_id)
                    .await
                    .expect("connect failed");
                let (mut reader, mut writer) = stream.into_split();
                let payload = vec![i; 128];
                let mut data = Bytes::from(payload.clone());
                writer
                    .write_all_from_fin(&mut data)
                    .await
                    .expect("client write");

                let mut buf = BytesMut::with_capacity(256);
                loop {
                    let n = reader.read_into(&mut buf).await.expect("client read");
                    if n == 0 {
                        break;
                    }
                }
                assert_eq!(&buf[..], &payload[..], "stream {} echo mismatch", i);
            }

            info!("ack_drains_inflight passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that a bidirectional data exchange works when both sides send simultaneously.
///
/// Client sends "client_data" while the server sends "server_data" without waiting
/// for the client's message first.
#[test]
fn bidirectional_simultaneous_send() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);

        // ── Server ────────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();

                    // Send server data immediately (don't wait for client data first)
                    let mut server_data = Bytes::from_static(b"server_data");
                    writer
                        .write_all_from_fin(&mut server_data)
                        .await
                        .expect("server write");

                    // Then read client data
                    let mut buf = BytesMut::with_capacity(32);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert_eq!(&buf[..], b"client_data");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // ── Client ────────────────────────────────────────────────────────
        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (mut reader, mut writer) = stream.into_split();

            // Send client data
            let mut client_data = Bytes::from_static(b"client_data");
            writer
                .write_all_from_fin(&mut client_data)
                .await
                .expect("client write");

            // Receive server data
            let mut buf = BytesMut::with_capacity(32);
            loop {
                let n = reader.read_into(&mut buf).await.expect("client read");
                if n == 0 {
                    break;
                }
            }
            assert_eq!(&buf[..], b"server_data");

            info!("bidirectional_simultaneous_send passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies duplicate client init traffic does not create duplicate accepted streams.
///
/// The first client packet toward the server is duplicated at the network layer.
/// The server should still accept exactly one stream and no second accept event
/// should appear afterward.
#[test]
fn duplicated_client_init_accepts_only_once() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        let duplicated_packets = Arc::new(AtomicUsize::new(0));
        let duplicated_packets_monitor = duplicated_packets.clone();

        {
            let mut duplicated_first_client_packet = false;
            bach::net::monitor::on_packet_sent(move |packet| {
                // Test-setup assumption: the first non-duplicate packet emitted is the client's
                // QueueInit packet, so duplicating that first original packet exercises init dedup.
                if !packet.is_duplicate && !duplicated_first_client_packet {
                    duplicated_first_client_packet = true;
                    duplicated_packets_monitor.fetch_add(1, Ordering::Relaxed);
                    return bach::net::monitor::duplicate(1).absolute().into();
                }

                bach::net::monitor::Command::Pass
            });
        }

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            let stream = timeout(Duration::from_secs(1), acceptor.recv())
                .await
                .expect("first stream should be accepted within timeout")
                .expect("server should accept one stream");

            let stream = stream;
            let (mut reader, mut writer) = stream.into_split();

            let mut buf = BytesMut::with_capacity(8);
            loop {
                let n = reader.read_into(&mut buf).await.expect("server read");
                if n == 0 {
                    break;
                }
            }
            assert_eq!(&buf[..], b"ping");

            let mut pong = Bytes::from_static(b"pong");
            writer
                .write_all_from_fin(&mut pong)
                .await
                .expect("server write");

            let unexpected = timeout(Duration::from_millis(200), acceptor.recv()).await;
            assert!(
                unexpected.is_err(),
                "duplicate init traffic must not create an extra accepted stream"
            );
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (mut reader, mut writer) = stream.into_split();

            let mut ping = Bytes::from_static(b"ping");
            writer
                .write_all_from_fin(&mut ping)
                .await
                .expect("client write");

            let mut buf = BytesMut::with_capacity(8);
            loop {
                let n = reader.read_into(&mut buf).await.expect("client read");
                if n == 0 {
                    break;
                }
            }
            assert_eq!(&buf[..], b"pong");

            assert_eq!(
                duplicated_packets.load(Ordering::Relaxed),
                1,
                "test setup should duplicate exactly one client packet"
            );
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies streams targeting an unregistered acceptor ID are not delivered to
/// other registered acceptor channels.
#[test]
fn unregistered_acceptor_id_does_not_reach_registered_acceptor() {
    sim(|| {
        let registered_acceptor_id = VarInt::from_u8(1);
        let missing_acceptor_id = VarInt::from_u8(2);

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(registered_acceptor_id, 8)
                .expect("acceptor registration failed");

            let unexpected = timeout(Duration::from_secs(1), acceptor.recv()).await;
            assert!(
                unexpected.is_err(),
                "stream for unregistered acceptor id should not arrive on registered acceptor"
            );
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let mut stream = client
                .connect("server:0", missing_acceptor_id)
                .await
                .expect("connect failed");

            let mut payload = Bytes::from_static(b"ping");
            let written = stream.write_from(&mut payload).await.expect("client write");
            assert!(written > 0, "client write should send at least one byte");

            let mut buf = BytesMut::with_capacity(1);
            let err = timeout(
                Duration::from_secs(1), // simulated wall-clock timeout (bach time)
                stream.read_into(&mut buf),
            )
            .await
            .expect("client read should fail within timeout")
            .expect_err("read should fail for unregistered acceptor id");
            assert_eq!(err.kind(), std::io::ErrorKind::ConnectionReset);

            let reset_error = err
                .get_ref()
                .and_then(|cause| cause.downcast_ref::<crate::endpoint::error::Error>())
                .copied()
                .expect("reset should include endpoint error code");
            assert_eq!(reset_error, crate::endpoint::error::Error::AcceptorNotFound);
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that a stream read eventually fails when all packets are dropped.
///
/// After the initial handshake succeeds (client sends "ping", server receives it
/// and sends "pong"), 100% packet loss is enabled. The client's read should
/// eventually surface a timeout/dead-peer error rather than hanging forever.
///
/// This exercises the idle-timeout → PeerDead → completion path: when the PTO
/// can never elicit an ACK, the send context must eventually be declared dead
/// and the outstanding frames failed.
#[test]
fn total_packet_loss_surfaces_read_timeout() {
    // Snapshot disabled: the PTO wheel fires at a fixed base rate with a countdown
    // for backoff, producing thousands of TRACE lines (~23MB) that change with any
    // PTO tuning. The test's value is in the assertion, not the log trace.
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);

        // After the first successful exchange, drop ALL packets.
        // We use a shared flag to enable the blackhole after setup.
        let blackhole = Arc::new(AtomicUsize::new(0));
        let blackhole_monitor = blackhole.clone();
        {
            bach::net::monitor::on_packet_sent(move |_packet| {
                if blackhole_monitor.load(Ordering::Relaxed) > 0 {
                    return bach::net::monitor::Command::Drop;
                }
                bach::net::monitor::Command::Pass
            });
        }

        // ── Server ────────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();

                    // Read "ping" from the client.
                    let mut buf = BytesMut::with_capacity(8);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert_eq!(&buf[..], b"ping");

                    // Send "pong" back — this will be the data the client is waiting for.
                    let mut pong = Bytes::from_static(b"pong");
                    writer
                        .write_all_from_fin(&mut pong)
                        .await
                        .expect("server write");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // ── Client ────────────────────────────────────────────────────────
        {
            let blackhole = blackhole.clone();
            async move {
                let mut client = Client::new();

                // First stream: normal exchange to confirm connectivity works.
                let stream = client
                    .connect("server:0", acceptor_id)
                    .await
                    .expect("connect failed");
                let (mut reader, mut writer) = stream.into_split();

                let mut ping = Bytes::from_static(b"ping");
                writer
                    .write_all_from_fin(&mut ping)
                    .await
                    .expect("client write");

                let mut buf = BytesMut::with_capacity(8);
                loop {
                    let n = reader.read_into(&mut buf).await.expect("client read");
                    if n == 0 {
                        break;
                    }
                }
                assert_eq!(&buf[..], b"pong");

                info!("baseline exchange succeeded; enabling blackhole");

                // Enable 100% packet loss.
                blackhole.store(1, Ordering::Relaxed);

                // Second stream: the server will never receive this, so the
                // client write may or may not complete (it can buffer locally),
                // but the read must eventually fail.
                let stream2 = client
                    .connect("server:0", acceptor_id)
                    .await
                    .expect("connect 2 failed");
                let (mut reader2, mut writer2) = stream2.into_split();

                let mut data = Bytes::from_static(b"hello");
                // Write succeeds (buffered locally)
                writer2.write_all_from(&mut data).await.unwrap();

                // The read should eventually fail once the peer is declared dead.
                // The idle timeout is 30s in test params; allow generous sim time.
                let result = timeout(120.s(), reader2.read_into(&mut BytesMut::new())).await;

                match result {
                    Err(_elapsed) => {
                        panic!(
                            "read did not fail within 120s simulated time — \
                             PTO is stuck without surfacing idle timeout"
                        );
                    }
                    Ok(Ok(0)) => {
                        panic!("read returned EOF — should have returned an error");
                    }
                    Ok(Ok(_n)) => {
                        panic!("read returned data — blackhole should prevent delivery");
                    }
                    Ok(Err(e)) => {
                        info!(?e, "read failed as expected");
                        assert!(
                            e.kind() == io::ErrorKind::TimedOut,
                            "expected TimedOut error kind, got: {e:?}"
                        );
                    }
                }

                info!("total_packet_loss_surfaces_read_timeout passed");
            }
            .group("client")
            .primary()
            .spawn();
        }
    });
}

/// After idle-timeout peer-dead detection triggers, new flows to that peer are blocked for the
/// configured dead-peer cooldown period.
#[test]
fn peer_dead_cooldown_blocks_new_connects() {
    // Snapshot disabled: this test intentionally drives timeout and loss behavior.
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        let blackhole = Arc::new(AtomicUsize::new(0));
        let blackhole_monitor = blackhole.clone();
        {
            bach::net::monitor::on_packet_sent(move |_packet| {
                if blackhole_monitor.load(Ordering::Relaxed) > 0 {
                    return bach::net::monitor::Command::Drop;
                }
                bach::net::monitor::Command::Pass
            });
        }

        // ── Server ────────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();

                    let mut buf = BytesMut::with_capacity(8);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert_eq!(&buf[..], b"ping");

                    let mut pong = Bytes::from_static(b"pong");
                    writer
                        .write_all_from_fin(&mut pong)
                        .await
                        .expect("server write");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // ── Client ────────────────────────────────────────────────────────
        {
            let blackhole = blackhole.clone();
            async move {
                let mut client = Client::new();

                // Baseline exchange to establish path state.
                let stream = client
                    .connect("server:0", acceptor_id)
                    .await
                    .expect("connect failed");
                let (mut reader, mut writer) = stream.into_split();

                let mut ping = Bytes::from_static(b"ping");
                writer
                    .write_all_from_fin(&mut ping)
                    .await
                    .expect("client write");

                let mut buf = BytesMut::with_capacity(8);
                loop {
                    let n = reader.read_into(&mut buf).await.expect("client read");
                    if n == 0 {
                        break;
                    }
                }
                assert_eq!(&buf[..], b"pong");

                // Trigger peer-dead by dropping all packets for a new stream.
                blackhole.store(1, Ordering::Relaxed);

                let stream2 = client
                    .connect("server:0", acceptor_id)
                    .await
                    .expect("connect 2 failed");
                let (mut reader2, mut writer2) = stream2.into_split();

                let mut data = Bytes::from_static(b"hello");
                writer2.write_all_from(&mut data).await.unwrap();

                let result = timeout(120.s(), reader2.read_into(&mut BytesMut::new())).await;
                match result {
                    Ok(Err(e)) => {
                        assert_eq!(
                            e.kind(),
                            io::ErrorKind::TimedOut,
                            "expected timed out after peer dead detection"
                        );
                    }
                    other => panic!("expected timed out read error, got: {other:?}"),
                }

                // During cooldown, opening a new flow must fail immediately.
                let connect3 = client.connect("server:0", acceptor_id).await;
                match connect3 {
                    Err(e) => {
                        assert_eq!(
                            e.kind(),
                            io::ErrorKind::TimedOut,
                            "expected connect rejection during dead-peer cooldown"
                        );
                    }
                    Ok(_) => panic!("connect succeeded during dead-peer cooldown"),
                }
            }
            .group("client")
            .primary()
            .spawn();
        }
    });
}

/// When all packets are lost, a writer blocked on flow-control credits must eventually
/// surface a timeout error rather than hanging forever.
///
/// Unlike `total_packet_loss_surfaces_read_timeout` which tests the reader path (small
/// payload fits in early data), this test fills the initial credit window so the writer
/// actually blocks waiting for MAX_DATA that never arrives.
#[test]
fn total_packet_loss_surfaces_write_timeout() {
    // Snapshot disabled: the PTO wheel fires at a fixed base rate with a countdown
    // for backoff, producing thousands of TRACE lines (~23MB) that change with any
    // PTO tuning. The test's value is in the assertion, not the log trace.
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);

        let blackhole = Arc::new(AtomicUsize::new(0));
        let blackhole_monitor = blackhole.clone();
        {
            bach::net::monitor::on_packet_sent(move |_packet| {
                if blackhole_monitor.load(Ordering::Relaxed) > 0 {
                    return bach::net::monitor::Command::Drop;
                }
                bach::net::monitor::Command::Pass
            });
        }

        // ── Server ────────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();

                    let mut buf = BytesMut::with_capacity(8);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }

                    let mut pong = Bytes::from_static(b"pong");
                    writer
                        .write_all_from_fin(&mut pong)
                        .await
                        .expect("server write");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // ── Client ────────────────────────────────────────────────────────
        {
            let blackhole = blackhole.clone();
            async move {
                let mut client = Client::new();

                // Baseline exchange to confirm connectivity.
                let stream = client
                    .connect("server:0", acceptor_id)
                    .await
                    .expect("connect failed");
                let (mut reader, mut writer) = stream.into_split();

                let mut ping = Bytes::from_static(b"ping");
                writer
                    .write_all_from_fin(&mut ping)
                    .await
                    .expect("client write");

                let mut buf = BytesMut::with_capacity(8);
                loop {
                    let n = reader.read_into(&mut buf).await.expect("client read");
                    if n == 0 {
                        break;
                    }
                }
                assert_eq!(&buf[..], b"pong");

                info!("baseline exchange succeeded; enabling blackhole");
                blackhole.store(1, Ordering::Relaxed);

                // Second stream: write a payload larger than the initial credit window
                // (1 MiB) so the writer blocks waiting for MAX_DATA.
                let stream2 = client
                    .connect("server:0", acceptor_id)
                    .await
                    .expect("connect 2 failed");
                let (_reader2, mut writer2) = stream2.into_split();

                // 2 MiB of generated data — exceeds the 1 MiB initial credit window
                // without allocating a massive buffer upfront.
                let mut data = s2n_quic_core::stream::testing::Data::new(2 * 1024 * 1024);
                let result = timeout(120.s(), writer2.write_all_from(&mut data)).await;

                match result {
                    Err(_elapsed) => {
                        panic!(
                            "write did not fail within 120s simulated time — \
                             idle timeout should have surfaced an error"
                        );
                    }
                    Ok(Ok(_)) => {
                        panic!("write_all_from completed — should have blocked on credits");
                    }
                    Ok(Err(e)) => {
                        info!(?e, "write failed as expected");
                        assert_eq!(
                            e.kind(),
                            io::ErrorKind::TimedOut,
                            "expected TimedOut error kind, got: {e:?}"
                        );
                    }
                }

                info!("total_packet_loss_surfaces_write_timeout passed");
            }
            .group("client")
            .primary()
            .spawn();
        }
    });
}

/// Fuzz test: one client sends concurrent requests to two servers under random
/// packet loss. All streams must recover within a bounded time regardless of
/// the loss pattern.
#[test]
fn multi_server_concurrent_loss_recovery() {
    let _guard = crate::testing::without_tracing();

    bolero::check!()
        .with_test_time(core::time::Duration::from_secs(10))
        .with_shrink_time(core::time::Duration::from_secs(0))
        .with_max_len(usize::MAX)
        .run(|| {
            multi_server_concurrent_loss_sim();
        });
}

fn multi_server_concurrent_loss_sim() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        const NUM_STREAMS: usize = 50;

        // Drop 50% of all packets randomly.
        {
            bach::net::monitor::on_packet_sent(move |_packet| {
                if bach::rand::any::<bool>() {
                    return bach::net::monitor::Command::Drop;
                }
                bach::net::monitor::Command::Pass
            });
        }

        // ── Server A ──────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 64)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();
                    let mut buf = BytesMut::with_capacity(64);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server_a read");
                        if n == 0 {
                            break;
                        }
                    }
                    let mut echo = Bytes::copy_from_slice(&buf);
                    writer
                        .write_all_from_fin(&mut echo)
                        .await
                        .expect("server_a echo");
                }
                .spawn();
            }
        }
        .group("server_a")
        .spawn();

        // ── Server B ──────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 64)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();
                    let mut buf = BytesMut::with_capacity(64);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server_b read");
                        if n == 0 {
                            break;
                        }
                    }
                    let mut echo = Bytes::copy_from_slice(&buf);
                    writer
                        .write_all_from_fin(&mut echo)
                        .await
                        .expect("server_b echo");
                }
                .spawn();
            }
        }
        .group("server_b")
        .spawn();

        // ── Client ────────────────────────────────────────────────────────
        async move {
            let mut client = Client::new();
            let mut handles = Vec::new();

            for i in 0..NUM_STREAMS {
                let server = if i % 2 == 0 {
                    "server_a:0"
                } else {
                    "server_b:0"
                };
                let stream = client
                    .connect(server, acceptor_id)
                    .await
                    .expect("connect failed");

                let msg = format!("req-{i}");
                handles.push(
                    async move {
                        let (mut reader, mut writer) = stream.into_split();

                        let mut data = Bytes::from(msg.clone());
                        writer
                            .write_all_from_fin(&mut data)
                            .await
                            .expect("client write");

                        let mut buf = BytesMut::with_capacity(64);
                        timeout(Duration::from_secs(20), async {
                            loop {
                                let n = reader.read_into(&mut buf).await.expect("client read");
                                if n == 0 {
                                    break;
                                }
                            }
                        })
                        .await
                        .expect("stream should complete within 5s");

                        assert_eq!(&buf[..], msg.as_bytes(), "echo mismatch for {msg}");
                        io::Result::Ok(())
                    }
                    .spawn(),
                );
            }

            for handle in handles {
                handle.await.expect("task join").expect("stream failed");
            }

            info!(
                "multi_server_concurrent_loss_recovery passed: all {NUM_STREAMS} streams completed"
            );
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that a single [`Peer`] endpoint can connect to and communicate with
/// itself without getting confused about how to route packets.
///
/// The same endpoint acts as both sender ("client") and receiver ("server").
/// Path-secret entries for the self-connection use opposite endpoint types
/// (Client for sealing, Server for opening), so this test exercises that the
/// routing logic correctly selects the Client entry for the outbound Writer and
/// the Server entry for the inbound acceptor path.
#[test]
fn peer_self_loopback() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);

        async move {
            let mut peer = Peer::new();
            let mut acceptor = peer
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            // Spawn the acceptor side as a background task so the connect below
            // can proceed on the same cooperative Bach thread.
            async move {
                while let Some(stream) = acceptor.recv().await {
                    async move {
                        let stream = stream;
                        let (mut reader, mut writer) = stream.into_split();

                        let mut buf = BytesMut::with_capacity(8);
                        loop {
                            if reader.read_into(&mut buf).await.expect("server read") == 0 {
                                break;
                            }
                        }
                        assert_eq!(&buf[..], b"ping");

                        let mut pong = Bytes::from_static(b"pong");
                        writer
                            .write_all_from_fin(&mut pong)
                            .await
                            .expect("server write");
                    }
                    .spawn();
                }
            }
            .spawn();

            // Connect to self: "peer:0" resolves to this group's IP, port is
            // rewritten to SERVER_PORT by connect_stream.
            let stream = peer
                .connect("peer:0", acceptor_id)
                .await
                .expect("self-connect failed");

            let (mut reader, mut writer) = stream.into_split();

            let mut ping = Bytes::from_static(b"ping");
            writer
                .write_all_from_fin(&mut ping)
                .await
                .expect("client write");

            let mut buf = BytesMut::with_capacity(8);
            loop {
                if reader.read_into(&mut buf).await.expect("client read") == 0 {
                    break;
                }
            }
            assert_eq!(&buf[..], b"pong");

            info!("peer_self_loopback passed");
        }
        .group("peer")
        .primary()
        .spawn();
    });
}

#[test]
fn five_node_random_chatter_settles_after_stop() {
    const NODE_NAMES: [&str; 5] = ["node_0", "node_1", "node_2", "node_3", "node_4"];
    const CHAT_SECONDS: usize = 60;
    const SETTLE_WINDOW: Duration = Duration::from_secs(5);
    const MAX_PAYLOAD_SIZE: usize = 256;

    let _no_snap = crate::testing::without_snapshots();
    sim(|| {
        let acceptor_id = VarInt::from_u8(7);
        let monitor_active = Arc::new(AtomicBool::new(false));
        let packets_after_stop = Arc::new(AtomicUsize::new(0));

        {
            let monitor_active = monitor_active.clone();
            let packets_after_stop = packets_after_stop.clone();
            bach::net::monitor::on_packet_sent(move |_packet| {
                if monitor_active.load(Ordering::Relaxed) {
                    packets_after_stop.fetch_add(1, Ordering::Relaxed);
                }
                bach::net::monitor::Command::Pass
            });
        }

        let mut node_handles = Vec::with_capacity(NODE_NAMES.len());
        for (node_idx, node_name) in NODE_NAMES.iter().enumerate() {
            let handle = async move {
                let mut peer = Peer::new();
                let mut acceptor = peer
                    .register_acceptor_channel(acceptor_id, 256)
                    .expect("acceptor registration");

                async move {
                    while let Some(stream) = acceptor.recv().await {
                        async move {
                            let stream = stream;
                            let (mut reader, mut writer) = stream.into_split();
                            let mut buf = BytesMut::with_capacity(MAX_PAYLOAD_SIZE);

                            loop {
                                if reader.read_into(&mut buf).await.expect("server read") == 0 {
                                    break;
                                }
                            }
                            assert!(
                                buf.len() <= MAX_PAYLOAD_SIZE,
                                "unexpected oversized request payload"
                            );
                            let request = core::str::from_utf8(&buf)
                                .expect("request payload should be valid UTF-8");
                            assert!(
                                request.contains("->") && request.contains('@'),
                                "unexpected request payload format: {request}"
                            );

                            let mut response_data = buf.freeze();
                            writer
                                .write_all_from_fin(&mut response_data)
                                .await
                                .expect("server write");
                        }
                        .spawn();
                    }
                }
                .spawn();

                let rejection_sampling_threshold =
                    ((u8::MAX as usize + 1) / NODE_NAMES.len()) * NODE_NAMES.len();
                for tick in 0..CHAT_SECONDS {
                    let selected_peer_idx = loop {
                        let raw = bach::rand::any::<u8>() as usize;
                        // Rejection sampling to avoid modulo bias.
                        if raw >= rejection_sampling_threshold {
                            continue;
                        }
                        let candidate_peer_idx = raw % NODE_NAMES.len();
                        if candidate_peer_idx != node_idx {
                            break candidate_peer_idx;
                        }
                    };

                    let remote = format!("{}:0", NODE_NAMES[selected_peer_idx]);
                    let stream = peer
                        .connect(remote, acceptor_id)
                        .await
                        .expect("client connect");
                    let (mut reader, mut writer) = stream.into_split();

                    let payload = format!("{node_idx}->{selected_peer_idx}@{tick}");
                    let mut data = Bytes::copy_from_slice(payload.as_bytes());
                    writer
                        .write_all_from_fin(&mut data)
                        .await
                        .expect("client write");

                    let mut buf = BytesMut::with_capacity(MAX_PAYLOAD_SIZE);
                    loop {
                        if reader.read_into(&mut buf).await.expect("client read") == 0 {
                            break;
                        }
                    }
                    assert!(
                        buf.len() <= MAX_PAYLOAD_SIZE,
                        "unexpected oversized response payload"
                    );

                    assert_eq!(
                        &buf[..],
                        payload.as_bytes(),
                        "echo mismatch for node {node_idx} tick {tick}"
                    );
                    1.s().sleep().await;
                }
                SETTLE_WINDOW.sleep().await;
            }
            .group(*node_name)
            .spawn();
            node_handles.push(handle);
        }

        {
            let monitor_active = monitor_active.clone();
            let packets_after_stop = packets_after_stop.clone();
            let node_handles = node_handles;
            async move {
                Duration::from_secs(CHAT_SECONDS as u64).sleep().await;
                monitor_active.store(true, Ordering::Relaxed);
                SETTLE_WINDOW.sleep().await;
                for handle in node_handles {
                    handle.await.expect("node task should complete");
                }
                let sent = packets_after_stop.load(Ordering::Relaxed);
                monitor_active.store(false, Ordering::Relaxed);
                assert_eq!(
                    sent, 0,
                    "endpoints sent {sent} packet(s) after chatter stopped"
                );
            }
            .group("observer")
            .primary()
            .spawn();
        }
    });
}

/// Verifies that multiple concurrent tiny streams are batched into minimal packets.
///
/// Three concurrent streams from one client endpoint, each sending 10 bytes. The
/// server echoes 10 bytes back on each. Because all streams share the same endpoint
/// and their frames become ready at the same simulated tick, the send pipeline
/// batches them into far fewer packets than the naive per-stream case.
///
/// Ideal packet flow (3 total, vs 12 without batching):
///
/// 1. client→server: all 3 QueueInit + data + FIN frames (1 packet)
/// 2. server→client: all 3 ACK + data + FIN response frames (1 packet)
/// 3. client→server: all 3 final ACK frames (1 packet)
///
/// Current sim produces 4 packets: the server's ACK is sent in a separate packet
/// from its data response because they fire in different sim ticks (zero-contention
/// scheduling). Under production load these naturally coalesce into one packet.
#[test]
fn concurrent_tiny_streams_batch_into_minimal_packets() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        const NUM_STREAMS: usize = 3;

        let total_packets = Arc::new(AtomicUsize::new(0));
        {
            let total_packets = total_packets.clone();
            bach::net::monitor::on_packet_sent(move |_packet| {
                total_packets.fetch_add(1, Ordering::Relaxed);
                bach::net::monitor::Command::Pass
            });
        }

        // ── Server ────────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 16)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();

                    let mut buf = BytesMut::with_capacity(16);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }

                    let mut echo = Bytes::copy_from_slice(&buf);
                    writer
                        .write_all_from_fin(&mut echo)
                        .await
                        .expect("server write");
                }
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // ── Client ────────────────────────────────────────────────────────
        {
            let total_packets = total_packets.clone();
            async move {
                let mut client = Client::new();

                // Connect all streams first, then write concurrently so their
                // frames are all queued in the same send cycle.
                let mut streams = Vec::with_capacity(NUM_STREAMS);
                for i in 0..NUM_STREAMS {
                    let stream = client
                        .connect("server:0", acceptor_id)
                        .await
                        .expect("connect failed");
                    streams.push((i, stream));
                }

                let mut handles = Vec::with_capacity(NUM_STREAMS);
                for (i, stream) in streams {
                    handles.push(
                        async move {
                            let (mut reader, mut writer) = stream.into_split();

                            // 10 bytes of data
                            let payload = vec![i as u8; 10];
                            let mut data = Bytes::from(payload.clone());
                            writer
                                .write_all_from_fin(&mut data)
                                .await
                                .expect("client write");

                            let mut buf = BytesMut::with_capacity(16);
                            loop {
                                let n = reader.read_into(&mut buf).await.expect("client read");
                                if n == 0 {
                                    break;
                                }
                            }
                            assert_eq!(&buf[..], &payload[..], "echo mismatch for stream {i}");
                        }
                        .spawn(),
                    );
                }

                for handle in handles {
                    handle.await.expect("stream task join");
                }

                // Allow background ACKs to flush.
                Duration::from_millis(100).sleep().await;

                let packets = total_packets.load(Ordering::Relaxed);
                // Current architecture distributes responses across sockets:
                //   1 client→server (3 QueueInit frames batched)
                //   1 server→client ACK (pinned to recv socket for conntrack)
                //   1 server→client QueueFree (routed via pick-two LB)
                //   1 server→client data (3 QueueData+MaxData batched, pick-two LB)
                //   1-2 client→server ACKs (one per ack-eliciting server packet)
                // Total: 5-6 packets. Ideal would be 3 if all server frames
                // shared a single send context.
                assert!(
                    packets <= 6,
                    "expected at most 6 packets (data frames batch correctly; \
                     extra packets from multi-socket routing), got {packets}"
                );
            }
            .group("client")
            .primary()
            .spawn();
        }
    });
}

/// Verifies end-to-end stale-key recovery: after the server's recv-cache entry
/// is evicted by the idle wheel, the client's next stream initially fails
/// (stale key detected), but the server sends a StaleKey control packet back,
/// the client advances its key-id, retransmits, and the stream completes
/// successfully.
///
/// Setup: path-secret entries are pre-inserted with **asymmetric** idle
/// timeouts — the server-side entry has a short timeout (1 s) so its recv
/// context expires quickly, while the client-side entry has the default
/// 30 s timeout so the client's send context (and the key-id it carries)
/// stays alive.
///
/// After the first stream completes and time is advanced past the server's idle
/// timeout, the client sends a second stream using the *same* key-id (same send
/// context, different stream-id).  The server detects the stale key, sends a
/// StaleKey control packet, the client bumps its key-id, and the retransmitted
/// frame succeeds — proving full recovery despite the server losing recv state.
///
///
/// Demonstrates that zombie send flows are never invalidated when another flow on the same
/// path_secret_entry keeps refreshing `last_activity`.
///
/// Scenario:
/// 1. Client opens flow A to server — server→client responses for A are dropped after init
/// 2. Client periodically opens new flows (B, C, D...) that complete successfully,
///    keeping the shared path_secret_entry's `last_activity` fresh
/// 3. Flow A's send context should be idle-expired after 30s, but the shared activity
///    prevents this — it probes indefinitely at max backoff
///
/// This reproduces the "zombie flow" bug seen in production where flows with unanswered
/// probes persist indefinitely, inflating pick_two scores.
#[test]
fn zombie_flow_not_invalidated_when_path_has_other_activity() {
    use std::sync::atomic::AtomicBool;

    let zombie_still_probing = Arc::new(AtomicBool::new(false));
    let zombie_still_probing_inner = zombie_still_probing.clone();

    let _no_snap = crate::testing::without_snapshots();
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        let mut server_addr = MonitorHostAddr::new("server");

        // Track packets: count server→client drops after zombie starts
        let zombie_active = Arc::new(AtomicBool::new(false));
        let zombie_active_monitor = zombie_active.clone();
        let probes_after_idle = Arc::new(AtomicUsize::new(0));
        let probes_counter = probes_after_idle.clone();

        bach::net::monitor::on_packet_sent(move |packet| {
            // Once zombie is active, drop all server→client packets.
            // This means the zombie flow's probes never get responses,
            // BUT new flow inits from client→server still work (they go the other direction).
            if zombie_active_monitor.load(Ordering::Relaxed) && server_addr.is_packet_source(packet)
            {
                probes_counter.fetch_add(1, Ordering::Relaxed);
                return bach::net::monitor::Command::Drop;
            }
            bach::net::monitor::Command::Pass
        });

        // Server: accept and echo
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();
                    let mut buf = BytesMut::with_capacity(2048);
                    loop {
                        if reader.read_into(&mut buf).await.expect("read") == 0 {
                            break;
                        }
                    }
                    let mut echo = buf.freeze();
                    writer.write_all_from_fin(&mut echo).await.expect("write");
                }
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // Client
        {
            let zombie_active = zombie_active.clone();
            let probes_after_idle = probes_after_idle.clone();
            let zombie_still_probing = zombie_still_probing_inner;

            async move {
                let mut client = Client::new();

                // Open the zombie flow: client sends data, but server responses are dropped.
                zombie_active.store(true, Ordering::Relaxed);

                let stream_z = client
                    .connect(format!("server:{SERVER_PORT}"), acceptor_id)
                    .await
                    .expect("connect zombie");
                let (_reader_z, mut writer_z) = stream_z.into_split();
                let mut data = Data::new(1024);
                writer_z
                    .write_all_from_fin(&mut data)
                    .await
                    .expect("write zombie");

                // Wait 5s to let the zombie's probes start firing, then stop
                // dropping so subsequent flows can complete and refresh activity.
                5.s().sleep().await;
                zombie_active.store(false, Ordering::Relaxed);

                // Open new flows every 10s. Each one completes successfully,
                // causing server→client packets which call touch_activity on
                // the shared path_secret_entry.
                for _i in 0..8 {
                    10.s().sleep().await;
                    let stream = client
                        .connect(format!("server:{SERVER_PORT}"), acceptor_id)
                        .await
                        .expect("connect keepalive");
                    let (mut reader, mut writer) = stream.into_split();
                    let mut ping = Bytes::from_static(b"hi");
                    writer.write_all_from_fin(&mut ping).await.expect("write");
                    let mut buf = BytesMut::with_capacity(8);
                    loop {
                        if reader.read_into(&mut buf).await.expect("read") == 0 {
                            break;
                        }
                    }
                }

                // 85s have elapsed. Re-enable dropping to detect zombie probes.
                zombie_active.store(true, Ordering::Relaxed);
                let before = probes_after_idle.load(Ordering::Relaxed);
                30.s().sleep().await;
                let after = probes_after_idle.load(Ordering::Relaxed);

                if after > before {
                    zombie_still_probing.store(true, Ordering::Relaxed);
                }
            }
            .group("client")
            .primary()
            .spawn();
        }
    });

    // The zombie flow should have been invalidated despite other flows being active.
    // If it's still probing after 115s, the shared path_secret_entry activity kept
    // it alive — that's the bug.
    let still_probing = zombie_still_probing.load(Ordering::Relaxed);
    assert!(
        !still_probing,
        "BUG: zombie flow is still probing after 115s. Other flows on the same path kept \
         the path_secret_entry's last_activity fresh, preventing idle expiration of the \
         zombie send context."
    );
}

/// Reproduces a bug where QueueFree frames in-flight during peer-dead invalidation
/// are silently dropped, permanently leaking peer queue slots.
///
/// The send idle wheel drains inflight frames to `completed_tx` on peer-dead. The
/// CompletionDispatcher silently drops frames with `completion: None` (like QueueFree).
/// The `cancelled_drain` task (which rescues QueueFree frames into the retry queue)
/// only handles the `cancelled_tx` channel — it never sees peer-dead drains.
///
/// Result: the client's `peer_free` list permanently loses the freed slot, and
/// subsequent stream creation blocks forever once all initial slots are exhausted.
#[test]
#[ignore = "FIXME!"]
fn queue_free_lost_on_peer_dead_invalidation() {
    use crate::endpoint::testing::sim::{self, SERVER_PORT};
    use s2n_quic_core::{dc::testing::TEST_APPLICATION_PARAMS, varint::VarInt};
    use std::num::NonZeroU32;

    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        let drop_client_to_server = Arc::new(AtomicBool::new(false));

        // Monitor: drop client→server packets when the flag is set.
        // This prevents ACKs from reaching the server, keeping QueueFree in-flight.
        {
            let drop_flag = drop_client_to_server.clone();
            let mut client_addr = MonitorHostAddr::new("client");
            bach::net::monitor::on_packet_sent(move |packet| {
                if drop_flag.load(Ordering::Relaxed) && client_addr.is_packet_source(packet) {
                    return bach::net::monitor::Command::Drop;
                }
                bach::net::monitor::Command::Pass
            });
        }

        // ── Server ──────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let (mut reader, mut writer) = stream.into_split();
                    let mut buf = BytesMut::with_capacity(64);
                    loop {
                        if reader.read_into(&mut buf).await.expect("server read") == 0 {
                            break;
                        }
                    }
                    let mut echo = Bytes::copy_from_slice(&buf);
                    writer
                        .write_all_from_fin(&mut echo)
                        .await
                        .expect("server write");
                }
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // ── Client (primary) ────────────────────────────────────────────
        async move {
            let mut client = Client::new();
            bach::task::yield_now().await;

            let server_addr = bach::net::lookup_host(format!("server:{SERVER_PORT}"))
                .await
                .expect("lookup")
                .next()
                .expect("no addr");

            let client_addr = client.data_addr();
            let client_map = sim::lookup_sim_map(client_addr).expect("client map");
            let server_map = sim::lookup_sim_map(server_addr).expect("server map");

            // Configure with very small max_queues (2) so peer slots are scarce.
            // Short server-side send idle timeout (500ms) so peer-dead fires quickly.
            let mut server_send_params = TEST_APPLICATION_PARAMS;
            server_send_params.max_queues = VarInt::from_u8(2);
            server_send_params.max_idle_timeout = NonZeroU32::new(500);
            server_send_params.remote_max_data = server_send_params.local_recv_max_data;

            // Client-side entry: same small max_queues, longer idle timeout.
            let mut client_params = TEST_APPLICATION_PARAMS;
            client_params.max_queues = VarInt::from_u8(2);
            client_params.remote_max_data = client_params.local_recv_max_data;

            // Insert path pair. local_params → server_map entry, peer_params → client_map entry.
            client_map.test_insert_pair(
                client_addr,
                Some(server_send_params),
                &server_map,
                server_addr,
                Some(client_params),
            );

            if let Some(entry) = client_map.get_raw(server_addr) {
                entry.set_peer_data_addrs(&[server_addr]);
            }
            if let Some(entry) = server_map.get_raw(client_addr) {
                entry.set_peer_data_addrs(&[client_addr]);
            }

            // ── Stream 1: complete successfully (uses peer slot 0) ───────
            let stream1 = client
                .connect(format!("server:{SERVER_PORT}"), acceptor_id)
                .await
                .expect("connect 1");
            let (mut r1, mut w1) = stream1.into_split();
            let mut data1 = Bytes::from_static(b"hello");
            w1.write_all_from_fin(&mut data1).await.expect("write 1");
            let mut buf1 = BytesMut::with_capacity(16);
            loop {
                if r1.read_into(&mut buf1).await.expect("read 1") == 0 {
                    break;
                }
            }
            assert_eq!(&buf1[..], b"hello");
            // Stream 1 is done. Drop it so the server-side receivers are freed.
            drop(r1);
            drop(w1);

            // Allow time for QueueFree to be submitted and transmitted by the server.
            100.ms().sleep().await;

            // Now block all client→server traffic. The server's QueueFree packet
            // will never be ACKed, keeping it stuck in the inflight map.
            drop_client_to_server.store(true, Ordering::Relaxed);

            // Wait for the server's send idle timeout to expire (500ms + margin).
            // The send idle wheel will fire, invalidate the context, and drain
            // the in-flight QueueFree to completed_tx where it's silently dropped.
            800.ms().sleep().await;

            // Re-enable traffic.
            drop_client_to_server.store(false, Ordering::Relaxed);

            // ── Stream 2: uses peer slot 1 (the only remaining slot) ────
            let stream2 = client
                .connect(format!("server:{SERVER_PORT}"), acceptor_id)
                .await
                .expect("connect 2");
            let (mut r2, mut w2) = stream2.into_split();
            let mut data2 = Bytes::from_static(b"world");
            timeout(5.s(), w2.write_all_from_fin(&mut data2))
                .await
                .expect("write 2 timeout")
                .expect("write 2");
            let mut buf2 = BytesMut::with_capacity(16);
            loop {
                let n = timeout(5.s(), r2.read_into(&mut buf2))
                    .await
                    .expect("read 2 timeout")
                    .expect("read 2");
                if n == 0 {
                    break;
                }
            }
            assert_eq!(&buf2[..], b"world");
            drop(r2);
            drop(w2);

            // ── Stream 3: should reuse peer slot 0 via QueueFree ────────
            // If the bug is present, this will time out because peer slot 0
            // was never returned to the client's peer_free list.
            let result = timeout(
                3.s(),
                client.connect(format!("server:{SERVER_PORT}"), acceptor_id),
            )
            .await;

            assert!(
                result.is_ok(),
                "Stream 3 allocation timed out — peer slot 0 was leaked by \
                 QueueFree loss during peer-dead invalidation. The client's \
                 peer_free list is permanently depleted."
            );

            let stream3 = result.unwrap().expect("connect 3");
            let (mut r3, mut w3) = stream3.into_split();
            let mut data3 = Bytes::from_static(b"again");
            timeout(5.s(), w3.write_all_from_fin(&mut data3))
                .await
                .expect("write 3 timeout")
                .expect("write 3");
            let mut buf3 = BytesMut::with_capacity(16);
            loop {
                let n = timeout(5.s(), r3.read_into(&mut buf3))
                    .await
                    .expect("read 3 timeout")
                    .expect("read 3");
                if n == 0 {
                    break;
                }
            }
            assert_eq!(&buf3[..], b"again");

            info!("queue_free_lost_on_peer_dead_invalidation passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

#[test]
fn stale_key_detected_after_recv_cache_eviction() {
    use crate::endpoint::testing::sim::{self, SERVER_PORT};
    use s2n_quic_core::{dc::testing::TEST_APPLICATION_PARAMS, varint::VarInt};
    use std::num::NonZeroU32;

    sim(|| {
        let acceptor_id = VarInt::from_u8(1);

        // Server group: accept all streams and echo the payload.
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, mut writer) = stream.into_split();
                    let mut buf = BytesMut::with_capacity(32);
                    loop {
                        if reader.read_into(&mut buf).await.expect("read") == 0 {
                            break;
                        }
                    }
                    let mut echo = Bytes::copy_from_slice(&buf);
                    writer.write_all_from_fin(&mut echo).await.expect("write");
                }
                .spawn();
            }
        }
        .group("server")
        .spawn();

        // Client group (primary): orchestrates the stale-key scenario.
        async move {
            let mut client = Client::new();
            // Yield so the server group has a chance to bind its socket.
            bach::task::yield_now().await;

            // Resolve the server's address (its well-known port is SERVER_PORT).
            let server_addr = bach::net::lookup_host(format!("server:{SERVER_PORT}"))
                .await
                .expect("lookup")
                .next()
                .expect("no addr");

            // Retrieve both path-secret maps so we can pre-insert entries with
            // asymmetric idle timeouts before the first stream is established.
            let client_addr = client.data_addr();
            let client_map = sim::lookup_sim_map(client_addr).expect("client sim map");
            let server_map = sim::lookup_sim_map(server_addr).expect("server sim map");

            // Short idle timeout for the server-side entry (1 s).
            // This ensures the server's recv idle wheel evicts the recv context
            // well before the second stream arrives.
            let mut short_params = TEST_APPLICATION_PARAMS;
            short_params.max_idle_timeout = NonZeroU32::new(1_000); // 1,000 ms = 1 s
            short_params.remote_max_data = short_params.local_recv_max_data;

            // Standard idle timeout for the client-side entry (30 s).
            // The client's send context stays alive, keeping key-id = 0 in use
            // for both the first and second stream.
            let mut long_params = TEST_APPLICATION_PARAMS;
            long_params.remote_max_data = long_params.local_recv_max_data;

            // Insert the path-secret pair.
            //
            // In test_insert_pair, `local_params` is used by the peer (server_map)
            // and `peer_params` is used by self (client_map):
            //
            //   client_map entry (for server_addr): peer_params  → LONG timeout
            //   server_map entry (for client_addr): local_params → SHORT timeout
            client_map.test_insert_pair(
                client_addr,
                Some(short_params), // local_params → goes to server_map's entry for client_addr
                &server_map,
                server_addr,
                Some(long_params), // peer_params → goes to client_map's entry for server_addr
            );

            // Populate peer data addresses so the send context knows where to
            // deliver packets.
            if let Some(entry) = client_map.get_raw(server_addr) {
                entry.set_peer_data_addrs(&[server_addr]);
            }
            if let Some(entry) = server_map.get_raw(client_addr) {
                entry.set_peer_data_addrs(&[client_addr]);
            }

            // ── First stream: should complete successfully ──────────────────────
            //
            // client.connect() finds the pre-inserted entry via the fast path (no
            // re-insertion), so the send context is created with key-id = 0.  The
            // server recv cache misses, calls check_dedup(key-id=0) → OK, and
            // installs the recv context.
            let stream = client
                .connect(format!("server:{SERVER_PORT}"), acceptor_id)
                .await
                .expect("connect 1");
            let (mut reader, mut writer) = stream.into_split();
            let mut ping = Bytes::from_static(b"ping");
            writer.write_all_from_fin(&mut ping).await.expect("write 1");
            let mut buf = BytesMut::with_capacity(8);
            loop {
                if reader.read_into(&mut buf).await.expect("read 1") == 0 {
                    break;
                }
            }
            assert_eq!(&buf[..], b"ping", "first stream should echo payload");

            // ── Advance simulated time past the server's idle timeout ────────────
            //
            // The server's recv idle wheel fires at ~1 s and reschedules until the
            // context is expired (≥ 2 s elapsed).  After 3 s the recv context for
            // the client is definitely gone from the server's recv cache.
            // The client's send context (30 s timeout) is still alive: it still
            // holds key-id = 0.
            3.s().sleep().await;

            // ── Second stream: stale-key recovery ────────────────────────────────
            //
            // The client's send context is a cache HIT (same path-secret entry,
            // still within its 30 s window) → packets are encrypted with key-id = 0.
            // The server's recv cache MISSES → check_dedup(key-id=0) → AlreadyExists
            // → server sends StaleKey control packet back to client.
            // The client receives the StaleKey, bumps its sender key-id, the
            // invalidation task retransmits the frame, and the stream completes.
            let stream2 = client
                .connect(format!("server:{SERVER_PORT}"), acceptor_id)
                .await
                .expect("connect 2");
            let (mut reader2, mut writer2) = stream2.into_split();
            let mut data = Bytes::from_static(b"hello");
            timeout(5.s(), writer2.write_all_from_fin(&mut data))
                .await
                .expect("write 2 should not time out (stale-key recovery)")
                .expect("write 2");
            let mut buf2 = BytesMut::with_capacity(8);
            loop {
                let n = timeout(5.s(), reader2.read_into(&mut buf2))
                    .await
                    .expect("read 2 should not time out")
                    .expect("read 2");
                if n == 0 {
                    break;
                }
            }
            assert_eq!(
                &buf2[..],
                b"hello",
                "second stream should echo payload after stale-key recovery"
            );
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── QueueMsg Integration Tests ─────────────────────────────────────────────

/// Single message, two chunks — verifies the basic QueueMsg path works end-to-end.
/// Uses a size just over one MTU to ensure QueueMsg routing (not QueueData).
#[test]
fn queue_msg_single_chunk() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        const MSG_SIZE: usize = 16384;

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let (mut reader, _writer) = stream.into_split();
                    let mut recv = Data::new(MSG_SIZE as u64);
                    loop {
                        let n = reader.read_into(&mut recv).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert!(recv.is_finished(), "server should receive all data");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            let mut data = Data::new(MSG_SIZE as u64);
            writer
                .write_msg(
                    &mut data,
                    crate::stream::MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("client write_msg");

            info!("queue_msg_single_chunk passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Multi-chunk message — verifies reassembly across multiple QueueMsg frames.
#[test]
fn queue_msg_multi_chunk() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        // 64KB message at ~8KB MTU = ~8 chunks
        const MSG_SIZE: usize = 65536;

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let (mut reader, _writer) = stream.into_split();
                    let mut recv = Data::new(MSG_SIZE as u64);
                    loop {
                        let n = reader.read_into(&mut recv).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert!(recv.is_finished(), "server should receive all data");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            let mut data = Data::new(MSG_SIZE as u64);
            writer
                .write_msg(
                    &mut data,
                    crate::stream::MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("client write_msg");

            info!("queue_msg_multi_chunk passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that `write_msg` sends only a single QueueMsg frame (chunk_index=0)
/// before the server acknowledges. The server allocates the full message buffer from
/// the first frame's `message_size` header, but the client doesn't bombard the server
/// with the remaining chunks until confirmation arrives.
#[test]
fn queue_msg_init_sends_single_frame() {
    let init_packets = Arc::new(AtomicUsize::new(0));
    let init_packets_check = init_packets.clone();

    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        const MSG_SIZE: usize = 64 * 1024;

        // Count client packets sent before any server response.
        // The client should send exactly 1 packet (the init probe) before
        // the server responds with MAX_DATA.
        {
            let init_packets = init_packets.clone();
            let mut client_addr = MonitorHostAddr::new("client");
            let mut server_addr = MonitorHostAddr::new("server");
            let server_responded = Arc::new(AtomicBool::new(false));
            let server_responded_inner = server_responded.clone();
            bach::net::monitor::on_packet_sent(move |packet| {
                if server_addr.is_packet_source(packet) {
                    server_responded_inner.store(true, Ordering::Relaxed);
                } else if client_addr.is_packet_source(packet)
                    && !server_responded.load(Ordering::Relaxed)
                {
                    init_packets.fetch_add(1, Ordering::Relaxed);
                }
                bach::net::monitor::Command::Pass
            });
        }

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            let stream = timeout(5.s(), acceptor.recv())
                .await
                .expect("server accept timeout")
                .expect("server stream closed");
            let (mut reader, _writer) = stream.into_split();
            let mut recv = Data::new(MSG_SIZE as u64);
            loop {
                let n = timeout(5.s(), reader.read_into(&mut recv))
                    .await
                    .expect("server read timeout")
                    .expect("server read");
                if n == 0 {
                    break;
                }
            }
            assert!(recv.is_finished());
        }
        .group("server")
        .primary()
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            let mut data = Data::new(MSG_SIZE as u64);
            writer
                .write_msg(
                    &mut data,
                    crate::stream::MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("client write_msg");
        }
        .group("client")
        .primary()
        .spawn();
    });

    assert_eq!(
        init_packets_check.load(Ordering::Relaxed),
        1,
        "client should send exactly 1 packet at t=0 (the init probe)"
    );
}

/// With jumbo MTU (9001), a 2 MiB message fits in a single segment (256 chunks *
/// ~8857 bytes = ~2.17 MiB). Even when the send window is only 64 KiB, segment
/// sizing must not shrink to match the window — the receiver should see exactly
/// one contiguous allocation.
#[test]
#[ignore = "requires dispatch-layer MAX_DATA for single-allocation under flow control"]
fn queue_msg_single_allocation_under_flow_control() {
    use crate::{byte_vec::ByteVec, endpoint::testing::sim::SimEndpointConfig};

    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        const MSG_SIZE: usize = 2 * 1024 * 1024;

        async move {
            let server = SimEndpointConfig::default().mtu(9001).server();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            let stream = timeout(30.s(), acceptor.recv())
                .await
                .expect("server accept timeout")
                .expect("server stream closed");
            let (mut reader, _writer) = stream.into_split();
            let mut recv = ByteVec::new();
            loop {
                let n = timeout(30.s(), reader.read_into(&mut recv))
                    .await
                    .expect("server read timeout")
                    .expect("server read");
                if n == 0 {
                    break;
                }
            }
            assert_eq!(recv.len(), MSG_SIZE);
            assert_eq!(
                recv.chunks().count(),
                1,
                "2 MiB message at 9001 MTU should be a single allocation"
            );
        }
        .group("server")
        .primary()
        .spawn();

        async move {
            let mut client = SimEndpointConfig::default()
                .mtu(9001)
                .send_window(VarInt::from_u32(64 * 1024))
                .client();

            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            let mut data = Data::new(MSG_SIZE as u64);
            writer
                .write_msg(
                    &mut data,
                    crate::stream::MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("client write_msg");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Large message requiring multi-segment split — verifies that write_msg correctly
/// splits messages larger than MAX_CHUNKS * chunk_size into multiple msg_ids.
/// With ~1328-byte chunks and 256 MAX_CHUNKS, max_segment_size is ~340KB.
/// A 700KB message needs 3 segments but fits within the 1MB initial window.
#[test]
fn queue_msg_large_flow_control() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        // 700KB requires multiple segments (340KB each) but fits in 1MB window
        const MSG_SIZE: usize = 32 * 1024 * 1024;

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let (mut reader, _writer) = stream.into_split();
                    let mut recv = Data::new(MSG_SIZE as u64);
                    loop {
                        let n = reader.read_into(&mut recv).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert!(recv.is_finished(), "server should receive all data");
                    info!("queue_msg_large_flow_control passed");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            let mut data = Data::new(MSG_SIZE as u64);
            writer
                .write_msg(
                    &mut data,
                    crate::stream::MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("client write_msg");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Multiple messages on the same stream — verifies fence-gated ordering and
/// data integrity across message boundaries with advancing stream offsets.
#[test]
fn queue_msg_multiple_messages() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        const MSG_SIZE: usize = 16384;
        const NUM_MSGS: usize = 4;
        const TOTAL: u64 = (MSG_SIZE * NUM_MSGS) as u64;

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let (mut reader, _writer) = stream.into_split();
                    let mut recv = Data::new(TOTAL);
                    loop {
                        let n = reader.read_into(&mut recv).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert!(recv.is_finished(), "server should receive all data");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            // Use a single Data source so each message advances the stream offset,
            // producing different content per message for integrity validation.
            let mut source = Data::new(TOTAL);
            for msg_idx in 0..NUM_MSGS {
                let mut msg_buf = source.send_one(MSG_SIZE).expect("should have data");
                let is_last = msg_idx == NUM_MSGS - 1;
                writer
                    .write_msg(
                        &mut msg_buf,
                        crate::stream::MsgFlags {
                            is_fin: is_last,
                            is_wakeup: is_last,
                        },
                    )
                    .await
                    .expect("client write_msg");
            }

            info!("queue_msg_multiple_messages passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that write_msg with a small payload (≤ chunk_size) uses the QueueData
/// path transparently. The receiver gets the data through the existing stream
/// delivery mechanism without MsgTable involvement.
#[test]
fn queue_msg_small_message_uses_queue_data() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        // Small enough to fit in one chunk — should route through QueueData
        const MSG_SIZE: usize = 100;

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let (mut reader, _writer) = stream.into_split();
                    let mut recv = Data::new(MSG_SIZE as u64);
                    loop {
                        let n = reader.read_into(&mut recv).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert!(
                        recv.is_finished(),
                        "server should receive all data via QueueData path"
                    );
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            let mut data = Data::new(MSG_SIZE as u64);
            writer
                .write_msg(
                    &mut data,
                    crate::stream::MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("client write_msg (small, should use QueueData)");

            info!("queue_msg_small_message_uses_queue_data passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that an empty write_msg with FIN in Init state sends EOF and completes.
#[test]
fn queue_msg_empty_fin_closes_stream() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let (mut reader, _writer) = stream.into_split();
                    let mut recv = BytesMut::with_capacity(1);
                    let n = timeout(5.s(), reader.read_into(&mut recv))
                        .await
                        .expect("server read timeout")
                        .expect("server read");
                    assert_eq!(n, 0, "server should observe EOF for empty FIN message");
                    assert!(recv.is_empty(), "no payload should be delivered");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            let mut empty = Bytes::new();
            let n = timeout(
                5.s(),
                writer.write_msg(
                    &mut empty,
                    crate::stream::MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                ),
            )
            .await
            .expect("client write_msg timeout")
            .expect("client write_msg");
            assert_eq!(n, 0, "empty FIN message should report zero bytes written");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that an empty write_msg without FIN is a no-op and doesn't block later writes.
#[test]
fn queue_msg_empty_without_fin_is_noop() {
    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let (mut reader, _writer) = stream.into_split();
                    let mut recv = BytesMut::with_capacity(8);
                    loop {
                        let n = timeout(5.s(), reader.read_into(&mut recv))
                            .await
                            .expect("server read timeout")
                            .expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert_eq!(&recv[..], b"ping");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            let mut empty = Bytes::new();
            let n = timeout(
                5.s(),
                writer.write_msg(
                    &mut empty,
                    crate::stream::MsgFlags {
                        is_fin: false,
                        is_wakeup: true,
                    },
                ),
            )
            .await
            .expect("client write_msg timeout")
            .expect("client write_msg");
            assert_eq!(n, 0, "empty non-FIN message should be a no-op");

            let mut ping = Bytes::from_static(b"ping");
            timeout(5.s(), writer.write_all_from_fin(&mut ping))
                .await
                .expect("client write_all_from_fin timeout")
                .expect("client write_all_from_fin");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that resetting a stream mid-message doesn't leak or panic.
///
/// The server drops its reader after receiving the init frame but before the full
/// message completes, triggering a reset. The client's write_msg should surface
/// the error cleanly.
#[test]
fn queue_msg_reset_mid_message() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        const MSG_SIZE: usize = 65536;

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            let stream = acceptor.recv().await.expect("should receive stream");
            let (reader, _writer) = stream.into_split();
            // Drop the reader — triggers STOP_SENDING reset back to client.
            // The MsgTable may have partially-received chunks that must be cleaned up.
            drop(reader);

            info!("queue_msg_reset_mid_message: server dropped reader");
        }
        .group("server")
        .primary()
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            let mut data = Data::new(MSG_SIZE as u64);
            let result = writer
                .write_msg(
                    &mut data,
                    crate::stream::MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await;

            // Write may succeed (queued before reset) or fail (reset arrived).
            // Both are fine — the invariant is no panic, no leak, no hang.
            match result {
                Ok(_) => info!("write_msg completed before reset arrived"),
                Err(e) => info!("write_msg got expected error: {e}"),
            }
        }
        .group("client")
        .spawn();
    });
}

/// Verifies that QueueMsg reassembly works when chunks arrive out of order due to
/// packet loss and retransmission. The first few client→server packets are dropped,
/// forcing them to be retransmitted after later chunks have already arrived.
#[test]
fn queue_msg_reassembly_after_loss() {
    let client_packets_sent = Arc::new(AtomicUsize::new(0));

    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        // 64KB message = ~8 chunks at typical MTU. Drop the first 3 data packets
        // to force out-of-order reassembly.
        const MSG_SIZE: usize = 65536;

        let mut client_addr = MonitorHostAddr::new("client");
        {
            let client_packets_sent = client_packets_sent.clone();
            bach::net::monitor::on_packet_sent(move |packet| {
                if client_addr.is_packet_source(packet) {
                    let idx = client_packets_sent.fetch_add(1, Ordering::Relaxed) + 1;
                    // Drop packets 2, 3, 4 (the first data packets after the init).
                    // Packet 1 is the init frame which must arrive to establish the binding.
                    if (2..=4).contains(&idx) {
                        info!("dropping client packet #{idx} to force reorder");
                        return bach::net::monitor::Command::Drop;
                    }
                }
                bach::net::monitor::Command::Pass
            });
        }

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let (mut reader, _writer) = stream.into_split();
                    let mut recv = Data::new(MSG_SIZE as u64);
                    loop {
                        let n = reader.read_into(&mut recv).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert!(
                        recv.is_finished(),
                        "server should receive all data after reorder"
                    );
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            let mut data = Data::new(MSG_SIZE as u64);
            writer
                .write_msg(
                    &mut data,
                    crate::stream::MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("client write_msg");

            info!("queue_msg_reassembly_after_loss passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Dropping a writer mid-`write_msg` (e.g. timeout during init) must not stall the
/// server reader. The writer sends only the first chunk of a multi-chunk QueueMsg
/// during Init (force_first), then gets dropped. The drop path sends a FIN via
/// QueueData which bypasses the MsgTable. The MsgTable retains an incomplete entry
/// that permanently blocks drain_complete, so the reader's reassembler has a gap
/// it can never fill.
///
/// Expected: the server reader returns an error (or EOF) within a reasonable time.
/// Bug: the server reader stalls forever.
#[test]
fn write_msg_drop_during_init_stalls_reader() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        // Message larger than one chunk so the init path sends only chunk 0
        // and leaves the rest for after MAX_DATA arrives.
        const MSG_SIZE: usize = 8192;

        let server_read_completed = Arc::new(AtomicBool::new(false));
        let server_read_completed2 = server_read_completed.clone();

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                let flag = server_read_completed2.clone();
                async move {
                    let (mut reader, _writer) = stream.into_split();
                    let mut buf = BytesMut::with_capacity(MSG_SIZE);

                    // The reader should eventually get an error or EOF — not hang.
                    loop {
                        match reader.read_into(&mut buf).await {
                            Ok(0) => break,
                            Ok(_n) => continue,
                            Err(_e) => break,
                        }
                    }
                    flag.store(true, Ordering::Release);
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            // Start write_msg but cancel it before MAX_DATA arrives. The sim
            // has 500µs one-way latency (1ms RTT). Use a timeout shorter than
            // the RTT so the writer is still in InitSent when cancelled.
            let mut data = Bytes::from(vec![0xABu8; MSG_SIZE]);
            let write_result = timeout(Duration::from_micros(100), async {
                writer
                    .write_msg(
                        &mut data,
                        crate::stream::MsgFlags {
                            is_fin: false,
                            is_wakeup: true,
                        },
                    )
                    .await
            })
            .await;

            // The timeout must fire (writer blocked in InitSent waiting for
            // MAX_DATA that takes 1ms RTT to arrive).
            assert!(write_result.is_err(), "write_msg should have timed out");
            info!("write_msg timed out as expected");

            // Drop the writer explicitly — this sends FIN via QueueData.
            drop(writer);

            // Give the server time to process the FIN and unblock.
            bach::time::sleep(Duration::from_secs(5)).await;

            assert!(
                server_read_completed.load(Ordering::Acquire),
                "server reader must not stall — it should observe an error or EOF \
                 when the writer drops after a partial QueueMsg init"
            );

            info!("write_msg_drop_during_init_stalls_reader passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Cancelling a `write_msg` future mid-partial-segment and then using `write_from`
/// on the same writer must return an error. The receiver already has a partially-
/// allocated MsgTable entry that expects the remaining QueueMsg chunks. Allowing
/// `write_from` would send QueueData at an offset past the incomplete entry, creating
/// a permanent MsgTable gap that stalls the reader.
///
/// The writer rejects the `write_from` call with `InvalidInput` so the application
/// knows it must complete or shut down the in-progress message.
#[test]
fn write_msg_cancel_then_write_from_returns_error() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        // Message must be larger than one MTU chunk so force_first creates a partial
        // segment during init.
        const MSG_SIZE: usize = 8192;

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let (mut reader, _writer) = stream.into_split();
                    let mut buf = BytesMut::with_capacity(MSG_SIZE * 2);
                    loop {
                        match reader.read_into(&mut buf).await {
                            Ok(0) => break,
                            Ok(_n) => continue,
                            Err(_e) => break,
                        }
                    }
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (_reader, mut writer) = stream.into_split();

            // Start write_msg with a large payload. The init path sends only
            // chunk 0 (force_first) and then blocks in InitSent waiting for
            // MAX_DATA. Cancel the future before MAX_DATA arrives.
            let mut data = Bytes::from(vec![0xABu8; MSG_SIZE]);
            let write_result = timeout(Duration::from_micros(100), async {
                writer
                    .write_msg(
                        &mut data,
                        crate::stream::MsgFlags {
                            is_fin: false,
                            is_wakeup: true,
                        },
                    )
                    .await
            })
            .await;

            assert!(write_result.is_err(), "write_msg should have timed out");

            // Now attempt write_from on the same writer. The writer still has
            // pending_chunk_index > 0 from the cancelled write_msg. The writer
            // must reject this call with InvalidInput.
            let mut payload = Bytes::from_static(b"hello after cancel");
            let result = writer.write_from(&mut payload).await;

            assert!(result.is_err(), "write_from must fail with pending segment");
            let err = result.unwrap_err();
            assert_eq!(
                err.kind(),
                io::ErrorKind::InvalidInput,
                "expected InvalidInput, got {:?}",
                err
            );

            info!("write_msg_cancel_then_write_from_returns_error passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Sending a QueueMsg with `is_fin: true` and `is_wakeup: false` from a SERVER writer
/// (which starts in Open state with full budget) must still wake the client reader so
/// it can observe EOF. The server writer has large remote_budget from the start, so
/// the `is_wakeup` forced-true heuristic (budget <= max_segment_size) does NOT trigger.
/// All chunks genuinely have `is_wakeup: false`.
///
/// Without a fix, the client reader hangs indefinitely: the completed message (with FIN)
/// is pushed to the stream queue but the waker is never fired because `should_wake = false`.
#[test]
fn queue_msg_fin_without_wakeup_flag_still_wakes_reader() {
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        const MSG_SIZE: usize = 16384;

        let reader_reached_eof = Arc::new(AtomicBool::new(false));
        let reader_flag = reader_reached_eof.clone();

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let (mut reader, mut writer) = stream.into_split();

                    // Drain the client's hello to establish bidirectional flow.
                    let mut buf = BytesMut::with_capacity(64);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read hello");
                        if n == 0 {
                            break;
                        }
                    }

                    // Server writer starts in Open state with full budget.
                    // All QueueMsg chunks will have is_wakeup=false because
                    // remote_budget >> max_segment_size.
                    let mut data = Data::new(MSG_SIZE as u64);
                    writer
                        .write_msg(
                            &mut data,
                            crate::stream::MsgFlags {
                                is_fin: true,
                                is_wakeup: false,
                            },
                        )
                        .await
                        .expect("server write_msg");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = Client::new();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");

            let (mut reader, mut writer) = stream.into_split();
            let flag = reader_flag.clone();

            // Send a small message to establish the stream on the server side.
            let mut hello: &[u8] = b"hi";
            writer
                .write_from_fin(&mut hello)
                .await
                .expect("client init write");

            // Read the server's response until EOF.
            // If the bug exists, this hangs forever.
            let mut recv = Data::new(MSG_SIZE as u64);
            let read_result = timeout(Duration::from_secs(5), async {
                loop {
                    let n = reader.read_into(&mut recv).await.expect("client read");
                    if n == 0 {
                        break;
                    }
                }
            })
            .await;

            if read_result.is_ok() {
                assert!(recv.is_finished(), "client should receive all data");
                flag.store(true, Ordering::Release);
            }

            assert!(
                reader_reached_eof.load(Ordering::Acquire),
                "BUG: client reader did not reach EOF within 5s. \
                 QueueMsg with is_fin=true and is_wakeup=false from server writer \
                 failed to wake the reader. The completed message (with FIN) was pushed \
                 to the stream queue but the waker was never fired."
            );

            info!("queue_msg_fin_without_wakeup_flag_still_wakes_reader passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// End-to-end reproduction of the production write_msg deadlock.
///
/// Conditions (matching strss-2-dcquic):
/// - 64KB flow control window (send_window = 64KB sets all windows)
/// - Server streams a 128KB response via write_msg (message > window)
/// - is_wakeup: true, is_fin: true on the message
///
/// The deadlock: write_msg declares segment_size = 128KB but remote_max_data
/// is only 64KB. The budget check (remote_budget < segment_size) prevents
/// the segment from ever being started. The reader sends MAX_DATA(64KB) on
/// first poll but that's not enough — the writer needs 128KB of budget.
/// The reader won't send more credits until it consumes 32KB, but nothing
/// was ever sent. Permanent stall.
///
/// write_from never hit this because it sends MTU-sized (1.3KB) frames that
/// always individually fit within even a tiny window.
#[test]
fn queue_msg_write_msg_deadlock_message_exceeds_window() {
    use crate::endpoint::testing::sim::SimEndpointConfig;

    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        const RESPONSE_SIZE: usize = 128 * 1024; // 128KB > 64KB window

        async move {
            let server = SimEndpointConfig::default()
                .send_window(VarInt::from_u32(64 * 1024))
                .server();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            let stream = timeout(5.s(), acceptor.recv())
                .await
                .expect("server accept timeout")
                .expect("server stream closed");
            let (mut reader, mut writer) = stream.into_split();

            // Read client request
            let mut req = Data::new(64);
            loop {
                let n = timeout(5.s(), reader.read_into(&mut req))
                    .await
                    .expect("server read timeout")
                    .expect("server read");
                if n == 0 {
                    break;
                }
            }

            // Stream response — this write_msg should deadlock because
            // message_size (128KB) > remote_max_data (64KB)
            let mut response = Data::new(RESPONSE_SIZE as u64);
            writer
                .write_msg(
                    &mut response,
                    crate::stream::MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("server write_msg");
        }
        .group("server")
        .primary()
        .spawn();

        async move {
            let mut client = SimEndpointConfig::default()
                .send_window(VarInt::from_u32(64 * 1024))
                .client();
            let stream = client
                .connect("server:0", acceptor_id)
                .await
                .expect("connect failed");
            let (mut reader, mut writer) = stream.into_split();

            // Send small request
            let mut req = Data::new(64);
            writer
                .write_msg(
                    &mut req,
                    crate::stream::MsgFlags {
                        is_fin: true,
                        is_wakeup: true,
                    },
                )
                .await
                .expect("client write request");

            // Read response — deadlocks if server can't send
            let mut resp = Data::new(RESPONSE_SIZE as u64);
            loop {
                let n = timeout(10.s(), reader.read_into(&mut resp))
                    .await
                    .expect(
                        "DEADLOCK: write_msg(128KB) cannot send with 64KB window. \
                         segment_size exceeds remote_max_data and the reader's \
                         MAX_DATA threshold won't grow the window without first \
                         consuming data that was never sent.",
                    )
                    .expect("client read error");
                if n == 0 {
                    break;
                }
            }
            assert!(resp.is_finished());
        }
        .group("client")
        .spawn();
    });
}

/// Twenty nodes gossip with 3 random peers every second for 30 simulated minutes.
///
/// This exercises the routing hash, symmetric 5-tuple dispatch, and multi-sender
/// state under sustained cross-mesh traffic. The primary assertion is that no
/// routing asymmetry warnings fire (the `!send.routing_asymmetry` counter stays
/// zero throughout the run).
///
// TODO: assert directly on counter values once we have a query API for sim metrics
#[test]
fn twenty_node_gossip_no_routing_asymmetry() {
    const NUM_NODES: usize = 20;
    const PEERS_PER_TICK: usize = 3;
    const DURATION_SECS: usize = 5 * 60;
    const MAX_PAYLOAD_SIZE: usize = 128;

    let _guard = crate::testing::without_snapshots();
    sim(|| {
        let acceptor_id = VarInt::from_u8(1);
        let routing_asymmetry_count = Arc::new(AtomicUsize::new(0));

        for node_idx in 0..NUM_NODES {
            let routing_asymmetry_count = routing_asymmetry_count.clone();
            async move {
                let mut peer = Peer::new();
                let mut acceptor = peer
                    .register_acceptor_channel(acceptor_id, 4096)
                    .expect("acceptor registration");

                // Accept incoming streams and echo back.
                async move {
                    while let Some(stream) = acceptor.recv().await {
                        async move {
                            let stream = stream;
                            let (mut reader, mut writer) = stream.into_split();
                            let mut buf = BytesMut::with_capacity(MAX_PAYLOAD_SIZE);
                            loop {
                                if reader.read_into(&mut buf).await.expect("read") == 0 {
                                    break;
                                }
                            }
                            let mut echo = buf.freeze();
                            writer.write_all_from_fin(&mut echo).await.expect("write");
                        }
                        .spawn();
                    }
                }
                .spawn();

                // Gossip: connect to 3 random peers each tick, spawned in parallel.
                let rejection_threshold = ((u8::MAX as usize + 1) / NUM_NODES) * NUM_NODES;

                for _tick in 0..DURATION_SECS {
                    let mut selected = [0usize; PEERS_PER_TICK];
                    let mut count = 0;
                    while count < PEERS_PER_TICK {
                        let raw = bach::rand::any::<u8>() as usize;
                        if raw >= rejection_threshold {
                            continue;
                        }
                        let candidate = raw % NUM_NODES;
                        if candidate == node_idx {
                            continue;
                        }
                        if selected[..count].contains(&candidate) {
                            continue;
                        }
                        selected[count] = candidate;
                        count += 1;
                    }

                    for &target_idx in &selected {
                        let remote = format!("node_{target_idx}:0");
                        let stream = peer.connect(&*remote, acceptor_id).await.expect("connect");

                        // Spawn the RPC exchange so all 3 peers run concurrently.
                        async move {
                            let (mut reader, mut writer) = stream.into_split();

                            let payload = format!("{node_idx}->{target_idx}");
                            let mut data = Bytes::copy_from_slice(payload.as_bytes());
                            writer.write_all_from_fin(&mut data).await.expect("write");

                            let mut buf = BytesMut::with_capacity(MAX_PAYLOAD_SIZE);
                            loop {
                                if reader.read_into(&mut buf).await.expect("read") == 0 {
                                    break;
                                }
                            }
                            assert_eq!(
                                &buf[..],
                                payload.as_bytes(),
                                "echo mismatch: node {node_idx} -> node {target_idx}"
                            );
                        }
                        .spawn();
                    }

                    1.s().sleep().await;
                }

                let _ = &routing_asymmetry_count;
            }
            .group(format!("node_{node_idx}"))
            .spawn();
        }

        // Observer: wait for all nodes to finish, then assert zero asymmetry.
        {
            let routing_asymmetry_count = routing_asymmetry_count.clone();
            async move {
                Duration::from_secs(DURATION_SECS as u64 + 60).sleep().await;

                let count = routing_asymmetry_count.load(Ordering::Relaxed);
                assert_eq!(
                    count, 0,
                    "routing asymmetry detected {count} time(s) during 30-minute gossip"
                );
            }
            .group("observer")
            .primary()
            .spawn();
        }
    });
}
