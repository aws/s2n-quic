// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stream::testing::{Client, Server},
    testing::{ext::*, sim, spawn},
};
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
