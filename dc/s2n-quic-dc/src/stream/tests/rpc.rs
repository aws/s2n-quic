// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stream::{
        client::rpc,
        testing::{Client, Server},
    },
    testing::{ext::*, sim, without_tracing},
};
use bolero::check;
use bytes::BytesMut;
use s2n_quic_core::stream::testing::Data;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info_span, Instrument};

fn hello_goodbye() {
    async move {
        let client = Client::builder().build();
        let response = rpc::InMemoryResponse::from(BytesMut::default());
        let response = client
            .rpc_sim("server:443", &b"hello!"[..], response)
            .await
            .unwrap();

        assert_eq!(response, b"goodbye!"[..]);
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

                stream.write_from_fin(&mut &b"goodbye!"[..]).await.unwrap();
            }
            .instrument(info_span!("stream", ?peer_addr))
            .primary()
            .spawn();
        }
    }
    .group("server")
    .instrument(info_span!("server"))
    .spawn();
}

#[test]
fn simple() {
    sim(hello_goodbye);
}

// TODO use this with bach >= 0.0.13
#[cfg(todo)]
#[test]
fn no_loss() {
    use core::sync::atomic::{AtomicUsize, Ordering};

    static COUNT: AtomicUsize = AtomicUsize::new(0);

    sim(|| {
        hello_goodbye();

        {
            ::bach::net::monitor::on_socket_write(move |write| {
                let count = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
                assert!(count <= 4, "flow should only consume 4 packets\n{write:#?}");
                tracing::info!(?write, "on_socket_write");
                Ok(())
            });
        }
    });

    assert_eq!(COUNT.load(Ordering::Relaxed), 4);
}

// TODO use this with bach >= 0.0.13
// Also - this test is broken... need to fix the impl
#[cfg(todo)]
#[test]
fn packet_loss() {
    check!()
        .exhaustive()
        .with_generator(0usize..=4)
        .cloned()
        .for_each(|loss_idx| {
            sim(|| {
                hello_goodbye();

                {
                    let mut count = 0;
                    ::bach::net::monitor::on_packet_sent(move |packet| {
                        let idx = count;
                        count += 1;

                        assert!(
                            count <= 5,
                            "flow should only consume 5 packets\n{packet:#?}"
                        );

                        if loss_idx == idx {
                            return ::bach::net::monitor::Command::Drop;
                        }

                        Default::default()
                    });
                }
            });
        });
}

#[test]
fn echo_stream() {
    without_tracing(|| {
        check!().with_test_time(30.s()).run(|| {
            sim(|| {
                async move {
                    let client = Client::builder().build();
                    let data = Data::new((0..=512_000).any());
                    let response = rpc::InMemoryResponse::from(data);
                    let response = client.rpc_sim("server:443", data, response).await.unwrap();

                    assert!(response.is_finished());
                }
                .group("client")
                .primary()
                .spawn();

                async move {
                    let server = Server::udp().port(443).build();

                    while let Ok((mut stream, _addr)) = server.accept().await {
                        async move {
                            let mut buffer = vec![];
                            // echo the response back
                            loop {
                                let len = stream.read_buf(&mut buffer).await.unwrap();
                                if len == 0 {
                                    break;
                                }

                                stream.write_all(&buffer[..len]).await.unwrap();
                                buffer.clear();
                            }
                        }
                        .spawn();
                    }
                }
                .group("server")
                .spawn();
            })
        })
    });
}
