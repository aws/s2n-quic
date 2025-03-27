// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stream::{
        client::rpc,
        testing::{Client, Server},
    },
    testing::{ext::*, sim, spawn, without_tracing},
};
use bolero::check;
use bytes::BytesMut;
use s2n_quic_core::stream::testing::Data;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[test]
fn request_response() {
    sim(|| {
        async move {
            let client = Client::builder().build();
            let mut stream = client.connect_sim("server:443").await.unwrap();

            let request = vec![42; 100_000];
            stream.write_all(&request).await.unwrap();
            stream.shutdown().await.unwrap();

            let mut response = vec![];
            stream.read_to_end(&mut response).await.unwrap();

            assert_eq!(request, response);
        }
        .group("client")
        .primary()
        .spawn();

        async move {
            let server = Server::udp().port(443).build();

            while let Ok((mut stream, _addr)) = server.accept().await {
                spawn(async move {
                    let mut request = vec![];
                    stream.read_to_end(&mut request).await.unwrap();

                    stream.write_all(&request).await.unwrap();
                });
            }
        }
        .group("server")
        .spawn();
    });
}

#[test]
fn rpc_simple() {
    sim(|| {
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
        .primary()
        .spawn();

        async move {
            let server = Server::udp().port(443).build();

            while let Ok((mut stream, _addr)) = server.accept().await {
                spawn(async move {
                    let mut request = vec![];
                    stream.read_to_end(&mut request).await.unwrap();

                    stream.write_from_fin(&mut &b"goodbye!"[..]).await.unwrap();
                });
            }
        }
        .group("server")
        .spawn();
    });
}

#[test]
fn rpc_echo() {
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
