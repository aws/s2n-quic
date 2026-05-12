// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for the stream3 endpoint packet pipeline.
//!
//! Each test runs inside Bach's deterministic simulation (`testing::sim`) with two fully
//! wired endpoints backed by simulated UDP sockets.  Each endpoint lives in its own Bach
//! group so it is treated as a separate machine from the network perspective.

use crate::stream3::endpoint::testing::sim::{Client, Server};
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
