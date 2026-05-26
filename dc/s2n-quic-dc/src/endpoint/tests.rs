// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the stream endpoint packet pipeline.
//!
//! Each test runs inside Bach's deterministic simulation (`testing::sim`) with two fully
//! wired endpoints backed by simulated UDP sockets.  Each endpoint lives in its own Bach
//! group so it is treated as a separate machine from the network perspective.

use crate::{
    stream::endpoint::testing::sim::{Client, MonitorHostAddr, Peer, Server, SERVER_PORT},
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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

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
                    let stream = stream.validate().await.expect("server validate");
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

    crate::testing::sim(|| {
        use crate::testing::ext::*;

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
                    let stream = stream.validate().await.expect("server validate");
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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

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
                    let stream = stream.validate().await.expect("server validate");
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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let acceptor_id = VarInt::from_u8(1);

        // ── Server ────────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 16)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream.validate().await.expect("server validate");
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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

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
                    let stream = stream.validate().await.expect("server validate");
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

    crate::testing::sim(|| {
        use crate::testing::ext::*;

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
                    let stream = stream.validate().await.expect("server validate");
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
        server_packets, 4,
        "expected exactly four server packets after dropping the first two server packets"
    );
}

/// Verifies that ACKs are sent back and the sender's inflight map is drained.
///
/// After both directions complete, neither endpoint should have pending inflight
/// packets. This is an implicit test since the sim finishes cleanly without
/// hanging (which would indicate stuck inflight tracking).
#[test]
fn ack_drains_inflight() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let acceptor_id = VarInt::from_u8(1);

        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream.validate().await.expect("server validate");
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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let acceptor_id = VarInt::from_u8(1);

        // ── Server ────────────────────────────────────────────────────────
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream.validate().await.expect("server validate");
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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

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

            let stream = stream.validate().await.expect("server validate");
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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

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
                    let stream = stream.validate().await.expect("server validate");
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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

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
                    let stream = stream.validate().await.expect("server validate");
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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

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
                    let stream = stream.validate().await.expect("server validate");
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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

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
                    let stream = stream.validate().await.expect("server_a validate");
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
                    let stream = stream.validate().await.expect("server_b validate");
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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

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
                        let stream = stream.validate().await.expect("server validate");
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
    use crate::testing::ext::*;

    const NODE_NAMES: [&str; 5] = ["node_0", "node_1", "node_2", "node_3", "node_4"];
    const CHAT_SECONDS: usize = 60;
    const SETTLE_WINDOW: Duration = Duration::from_secs(5);
    const MAX_PAYLOAD_SIZE: usize = 256;

    let _no_snap = crate::testing::without_snapshots();
    crate::testing::sim(|| {
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
                            let stream = stream.validate().await.expect("server validate");
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
    crate::testing::sim(|| {
        use crate::testing::ext::*;

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
                    let stream = stream.validate().await.expect("server validate");
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
                                let n =
                                    reader.read_into(&mut buf).await.expect("client read");
                                if n == 0 {
                                    break;
                                }
                            }
                            assert_eq!(
                                &buf[..],
                                &payload[..],
                                "echo mismatch for stream {i}"
                            );
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
                assert!(
                    packets <= 4,
                    "expected at most 4 packets (3 streams batched; ideal is 3), got {packets}"
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
#[ignore = "TODO need to figure out what's going on here"]
fn zombie_flow_not_invalidated_when_path_has_other_activity() {
    use crate::testing::ext::*;
    use std::sync::atomic::AtomicBool;

    let zombie_still_probing = Arc::new(AtomicBool::new(false));
    let zombie_still_probing_inner = zombie_still_probing.clone();

    let _no_snap = crate::testing::without_snapshots();
    crate::testing::sim(|| {
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
                    let stream = stream.validate().await.expect("validate");
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

#[test]
fn stale_key_detected_after_recv_cache_eviction() {
    use crate::{
        endpoint::testing::sim::{self, SERVER_PORT},
        testing::ext::*,
    };
    use s2n_quic_core::{dc::testing::TEST_APPLICATION_PARAMS, varint::VarInt};
    use std::num::NonZeroU32;

    crate::testing::sim(|| {
        let acceptor_id = VarInt::from_u8(1);

        // Server group: accept all streams and echo the payload.
        async move {
            let server = Server::new();
            let mut acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream.validate().await.expect("validate");
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
