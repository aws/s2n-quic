// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stream::testing::{Client, Server},
    testing::{ext::*, sim, spawn},
};
use bach::time::Instant;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    join,
};

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
fn fail_fast_unknown_path_secret() {
    sim(|| {
        async move {
            let client = Client::builder().build();
            let start = Instant::now();

            let count = 8u32;

            for idx in 0..count {
                // The current simulation code doesn't have a way to rehandshake so this will continue to fail for the peer
                let stream = client.connect_sim("server:443").await.unwrap();

                if idx == 0 {
                    // the first stream is throw away to get the server to drop its state
                    drop(stream);
                    1.ms().sleep().await;
                } else {
                    let (mut recv, mut send) = stream.into_split();

                    let send = async move {
                        if idx % 2 == 0 {
                            // small payloads should succeed from the application's point of view
                            let request = vec![42; 1024];
                            send.write_all(&request).await.unwrap();
                        } else {
                            // write large payloads to get blocked by flow
                            let request = vec![42; 1024 * 1024];
                            send.write_all(&request).await.unwrap_err();
                        }
                    };

                    let recv = async move {
                        let mut response = vec![];
                        recv.read_to_end(&mut response).await.unwrap_err();
                    };

                    join!(send, recv);
                }
            }

            let elapsed = start.elapsed();
            assert_eq!(elapsed, count * 1.ms(), "streams should fail within 1RTT");
        }
        .group("client")
        .primary()
        .spawn();

        async move {
            let server = Server::udp().port(443).build();

            while let Ok((mut stream, _addr)) = server.accept().await {
                // drop the state after accepting a stream to simulate a restart
                server.map().drop_state();

                spawn(async move {
                    let mut request = vec![];
                    let _ = stream.read_to_end(&mut request).await;
                });
            }
        }
        .group("server")
        .spawn();
    });
}
