// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the stream3 endpoint packet pipeline.
//!
//! Each test runs inside Bach's deterministic simulation (`testing::sim`) with two fully
//! wired endpoints backed by simulated UDP sockets.  Each endpoint lives in its own Bach
//! group so it is treated as a separate machine from the network perspective.

use crate::stream3::endpoint::testing::sim::{Client, Server, SERVER_PORT};
use bytes::{Bytes, BytesMut};
use s2n_quic_core::varint::VarInt;

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
            let acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            // Accept one stream.
            while let Ok(stream) = acceptor.recv_front().await {
                async move {
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

            let (mut reader, mut writer) = stream.into_split();

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
            let acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Ok(stream) = acceptor.recv_front().await {
                async move {
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
            let acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Ok(stream) = acceptor.recv_front().await {
                async move {
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
            let acceptor = server
                .register_acceptor_channel(acceptor_id, 16)
                .expect("acceptor registration failed");

            while let Ok(stream) = acceptor.recv_front().await {
                async move {
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
            let acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Ok(stream) = acceptor.recv_front().await {
                async move {
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
            let acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Ok(stream) = acceptor.recv_front().await {
                async move {
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
            let acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Ok(stream) = acceptor.recv_front().await {
                async move {
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
            let acceptor = server
                .register_acceptor_channel(acceptor_id, 8)
                .expect("acceptor registration failed");

            while let Ok(stream) = acceptor.recv_front().await {
                async move {
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
