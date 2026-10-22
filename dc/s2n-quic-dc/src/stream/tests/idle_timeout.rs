// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    stream::testing::{Client, Server},
    testing::{ext::*, sim, spawn},
};
use std::sync::{atomic::Ordering, Arc, OnceLock};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info_span, Instrument};

#[test]
fn other_half_keep_alive() {
    sim(|| {
        async move {
            let client = Client::builder().build();
            let mut stream = client.connect_sim("server:443").await.unwrap();

            for _ in 0..120 {
                stream.write_all(b"ping").await.unwrap();
                1.s().sleep().await;
            }
            stream.shutdown().await.unwrap();

            let mut response = vec![];
            stream.read_to_end(&mut response).await.unwrap();

            assert_eq!(response, b"pong!"[..]);
        }
        .group("client")
        .instrument(info_span!("client"))
        .primary()
        .spawn();

        async move {
            let server = Server::udp().port(443).build();

            while let Ok((mut stream, peer_addr)) = server.accept().await {
                async move {
                    let mut request = vec![];
                    stream.read_to_end(&mut request).await.unwrap();

                    stream.write_from_fin(&mut &b"pong!"[..]).await.unwrap();
                }
                .instrument(info_span!("stream", ?peer_addr))
                .primary()
                .spawn();
            }
        }
        .group("server")
        .instrument(info_span!("server"))
        .spawn();
    });
}

#[test]
fn server_no_response() {
    sim(|| {
        async move {
            let client = Client::builder().build();
            let mut stream = client.connect_sim("server:443").await.unwrap();

            stream.write_all(b"ping").await.unwrap();
            stream.shutdown().await.unwrap();

            let mut response = vec![];
            stream.read_to_end(&mut response).await.unwrap_err();
        }
        .group("client")
        .instrument(info_span!("client"))
        .primary()
        .spawn();

        async move {
            let server = Server::udp().port(443).build();

            while let Ok((mut stream, peer_addr)) = server.accept().await {
                async move {
                    let mut request = vec![];
                    stream.read_to_end(&mut request).await.unwrap();

                    // sleep long enough to trigger the idle timer
                    120.s().sleep().await;
                }
                .instrument(info_span!("stream", ?peer_addr))
                .primary()
                .spawn();
            }
        }
        .group("server")
        .instrument(info_span!("server"))
        .spawn();
    });
}

const REQUEST: &[u8] = b"request";
const RESPONSE: &[u8] = &[42; 245];

#[derive(Debug)]
struct ServerCounts {
    transmitted: u64,
    #[allow(dead_code, reason = "only printed in failure messages")]
    sender_errored: u64,
    receiver_errored: u64,
}

impl ServerCounts {
    fn of(subscriber: &event::testing::Subscriber) -> Self {
        Self {
            transmitted: subscriber.stream_packet_transmitted.load(Ordering::Relaxed),
            sender_errored: subscriber.stream_sender_errored.load(Ordering::Relaxed),
            receiver_errored: subscriber.stream_receiver_errored.load(Ordering::Relaxed),
        }
    }
}

/// The client opens its stream once its endpoint has been up for longer than the idle timeout,
/// and drops it as soon as it has the response, before the server's FIN arrives. The server
/// only finishes its half after reading the client's FIN.
#[test]
fn server_fin_is_answered_after_client_drops_stream() {
    let server_subscriber = Arc::new(OnceLock::new());
    let observed = server_subscriber.clone();

    sim(move || {
        async move {
            let client = Client::builder().build();

            45.s().sleep().await;

            let mut stream = client.connect_sim("server:443").await.unwrap();
            stream.write_all(REQUEST).await.unwrap();
            let mut response = vec![0; RESPONSE.len()];
            stream.read_exact(&mut response).await.unwrap();
            assert_eq!(response, RESPONSE);
            drop(stream);

            60.s().sleep().await;
        }
        .group("client")
        .primary()
        .spawn();

        async move {
            let server = Server::udp().port(443).build();
            let _ = observed.set(server.subscriber());

            while let Ok((mut stream, _addr)) = server.accept().await {
                spawn(async move {
                    let mut request = vec![0; REQUEST.len()];
                    stream.read_exact(&mut request).await.unwrap();
                    stream.write_all(RESPONSE).await.unwrap();

                    let mut rest = vec![];
                    stream.read_to_end(&mut rest).await.unwrap();
                    assert!(rest.is_empty());
                });
            }
        }
        .group("server")
        .spawn();
    });

    let server = ServerCounts::of(server_subscriber.get().unwrap());

    assert_eq!(
        server.transmitted, 2,
        "the server retransmitted its FIN to a client that never answered: {server:?}"
    );
    assert_eq!(
        server.receiver_errored, 0,
        "the server's stream idled out: {server:?}"
    );
}
