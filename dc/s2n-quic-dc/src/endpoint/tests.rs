// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the stream endpoint packet pipeline.
//!
//! Each test runs inside Bach's deterministic simulation (`testing::sim`) with two fully
//! wired endpoints backed by simulated UDP sockets.  Each endpoint lives in its own Bach
//! group so it is treated as a separate machine from the network perspective.

pub mod deterministic;

use crate::stream::endpoint::testing::sim::{Client, Server, SERVER_PORT};
use bach::time::timeout;
use bytes::{Bytes, BytesMut};
use s2n_quic_core::varint::VarInt;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

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

            // Send "ping" + FIN in the FlowInit packet.
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

            tracing::info!("ping_pong passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that PTO retransmission recovers from lost server responses.
///
/// The server sends "pong" back to the client but the first response packet is
/// dropped by the network monitor. The server's PTO mechanism should detect the
/// missing ACK and retransmit, allowing the exchange to complete.
#[test]
fn server_response_loss_triggers_pto() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let acceptor_id = VarInt::from_u8(1);

        // Drop the first packet sent from the server to the client.
        // The server binds to SERVER_PORT, so we identify its packets by source port.
        {
            let mut server_pkt_count = 0u32;
            bach::net::monitor::on_packet_sent(move |packet| {
                if packet.source().port() == SERVER_PORT {
                    server_pkt_count += 1;
                    if server_pkt_count == 1 {
                        tracing::info!(
                            "dropping server packet #{server_pkt_count} (source={:?}, len={})",
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

            tracing::info!("server_response_loss_triggers_pto passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that the client's initial packet loss is recovered by PTO.
///
/// The first packet from the client (FlowInit + ping data) is dropped. The
/// client should PTO-retransmit and the server should still see "ping".
#[test]
fn client_request_loss_triggers_pto() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let acceptor_id = VarInt::from_u8(1);

        // Drop the first packet from the client to the server.
        {
            let mut client_pkt_count = 0u32;
            bach::net::monitor::on_packet_sent(move |packet| {
                if packet.source().port() != SERVER_PORT {
                    client_pkt_count += 1;
                    if client_pkt_count == 1 {
                        tracing::info!(
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

            tracing::info!("client_request_loss_triggers_pto passed");
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
                tracing::info!("stream {:?} completed", msg);
            }

            tracing::info!("multiple_sequential_streams passed");
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

            tracing::info!("large_payload_transfer passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

/// Verifies that multiple consecutive packet drops are recovered by PTO.
///
/// The first two response packets from the server are dropped; PTO backoff
/// should recover on the third attempt.
#[test]
fn multiple_packet_loss_recovered_by_pto() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        let acceptor_id = VarInt::from_u8(1);

        // Drop the first two packets from the server.
        {
            let mut server_pkt_count = 0u32;
            bach::net::monitor::on_packet_sent(move |packet| {
                if packet.source().port() == SERVER_PORT {
                    server_pkt_count += 1;
                    if server_pkt_count <= 2 {
                        tracing::info!(
                            "dropping server packet #{server_pkt_count} len={}",
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

            tracing::info!("multiple_packet_loss_recovered_by_pto passed");
        }
        .group("client")
        .primary()
        .spawn();
    });
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

            tracing::info!("ack_drains_inflight passed");
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

            tracing::info!("bidirectional_simultaneous_send passed");
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
                // FlowInit packet, so duplicating that first original packet exercises init dedup.
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
