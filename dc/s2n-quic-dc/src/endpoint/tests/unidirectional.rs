// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Tests for "unidirectional" stream usage patterns.
//!
//! s2n-quic-dc streams are always bidirectional at the protocol level, but
//! applications often use them in a send-only or receive-only pattern by
//! immediately dropping the unused half (Reader or Writer).
//!
//! These tests verify that data integrity is preserved, STOP_SENDING from a
//! dropped Reader does not interfere with the peer's read path, flow control
//! works for substantial transfers, and stream cleanup is correct across
//! multiple sequential streams.

use crate::tracing::*;
use s2n_quic_core::{stream::testing::Data, varint::VarInt};

const ACCEPTOR_ID: VarInt = VarInt::from_u32(1);

// ── client_to_server_send_only ──────────────────────────────────────────────

/// Client opens a stream, drops its Reader immediately, and writes 64 KiB of
/// data + FIN. The server drops its Writer immediately and reads all data to
/// EOF, verifying data integrity.
///
/// Protocol interactions:
/// - Client Reader drop → STOP_SENDING to server's writer direction (already
///   dropped, so benign).
/// - Server Writer drop → FIN (0 bytes) to client's reader direction (already
///   dropped, endpoint handles gracefully).
/// - 64 KiB exceeds QueueInit early data (~1363 bytes), exercising the full
///   QueueData flow with MAX_DATA credit management.
#[test]
fn client_to_server_send_only() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        const PAYLOAD_LEN: u64 = 64 * 1024;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, writer) = stream.into_split();

                    drop(writer);

                    let mut buf = Data::new(PAYLOAD_LEN);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert!(buf.is_finished(), "server did not receive all expected data");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (reader, mut writer) = stream.into_split();

            drop(reader);

            let mut data = Data::new(PAYLOAD_LEN);
            writer
                .write_all_from_fin(&mut data)
                .await
                .expect("client write");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── client_to_server_send_only_large ────────────────────────────────────────

/// Same pattern as `client_to_server_send_only` but with 2 MiB to exercise
/// flow control round-trips.
///
/// The default remote_max_data in sim is 1 MiB, so this transfer requires at
/// least one MAX_DATA credit update from the server's recv path. The server
/// must still send MAX_DATA credits even though its Writer is dropped.
#[test]
fn client_to_server_send_only_large() {
    let _guard = crate::testing::without_snapshots();
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        const PAYLOAD_LEN: u64 = 2 * 1024 * 1024;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, writer) = stream.into_split();

                    drop(writer);

                    let mut buf = Data::new(PAYLOAD_LEN);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert!(buf.is_finished(), "server did not receive all 2 MiB");
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (reader, mut writer) = stream.into_split();

            drop(reader);

            let mut data = Data::new(PAYLOAD_LEN);
            writer
                .write_all_from_fin(&mut data)
                .await
                .expect("client write 2 MiB");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── server_to_client_send_only ──────────────────────────────────────────────

/// Server writes 64 KiB + FIN, drops its Reader. Client drops its Writer
/// immediately (no writes at all) and reads server data to EOF.
///
/// The client's Writer drop calls `shutdown()` from Init state, sending a
/// QueueInit with `is_fin=true` and 0 bytes of data. This alone establishes
/// the stream on the server — proving that a stream can be created without
/// any application-level write from the client.
#[test]
fn server_to_client_send_only() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        const PAYLOAD_LEN: u64 = 64 * 1024;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 8)
                .expect("acceptor registration");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (reader, mut writer) = stream.into_split();

                    drop(reader);

                    let mut data = Data::new(PAYLOAD_LEN);
                    writer
                        .write_all_from_fin(&mut data)
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
            let mut client = crate::stream::endpoint::testing::sim::Client::new();
            let stream = client
                .connect("server:0", ACCEPTOR_ID)
                .await
                .expect("connect");
            let (mut reader, writer) = stream.into_split();

            drop(writer);

            let mut buf = Data::new(PAYLOAD_LEN);
            loop {
                let n = reader.read_into(&mut buf).await.expect("client read");
                if n == 0 {
                    break;
                }
            }
            assert!(buf.is_finished(), "client did not receive all expected data");
        }
        .group("client")
        .primary()
        .spawn();
    });
}

// ── client_to_server_multiple_streams ───────────────────────────────────────

/// Multiple sequential unidirectional streams from client to server.
///
/// Verifies that stream cleanup is correct when the unidirectional pattern
/// is repeated: queue slots are freed, no leaked state accumulates, and
/// subsequent streams can be established without blocking.
#[test]
fn client_to_server_multiple_streams() {
    crate::testing::sim(|| {
        use crate::testing::ext::*;

        const PAYLOAD_LEN: u64 = 64 * 1024;
        const NUM_STREAMS: usize = 5;

        async move {
            let server = crate::stream::endpoint::testing::sim::Server::new();
            let mut acceptor = server
                .register_acceptor_channel(ACCEPTOR_ID, 16)
                .expect("acceptor registration");

            while let Some(stream) = acceptor.recv().await {
                async move {
                    let stream = stream;
                    let (mut reader, writer) = stream.into_split();

                    drop(writer);

                    let mut buf = Data::new(PAYLOAD_LEN);
                    loop {
                        let n = reader.read_into(&mut buf).await.expect("server read");
                        if n == 0 {
                            break;
                        }
                    }
                    assert!(buf.is_finished());
                }
                .primary()
                .spawn();
            }
        }
        .group("server")
        .spawn();

        async move {
            let mut client = crate::stream::endpoint::testing::sim::Client::new();

            for i in 0..NUM_STREAMS {
                let stream = client
                    .connect("server:0", ACCEPTOR_ID)
                    .await
                    .expect("connect");
                let (reader, mut writer) = stream.into_split();

                drop(reader);

                let mut data = Data::new(PAYLOAD_LEN);
                writer
                    .write_all_from_fin(&mut data)
                    .await
                    .expect("client write");

                info!("stream {i} completed");
            }
        }
        .group("client")
        .primary()
        .spawn();
    });
}
