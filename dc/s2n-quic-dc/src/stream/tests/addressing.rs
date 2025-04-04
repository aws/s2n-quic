// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use tokio::io::AsyncReadExt as _;

use crate::{stream::testing::*, testing::spawn};

async fn test_addr(client_addr: &str, server_addr: &str) {
    let client_addr = client_addr.parse().unwrap();
    let server_addr = server_addr.parse().unwrap();

    let client = Client::builder().local_addr(client_addr).build();
    let server = Server::builder().local_addr(server_addr).build();

    let server_handle = server.handle();
    spawn(async move {
        while let Ok((mut stream, _addr)) = server.accept().await {
            spawn(async move {
                let mut buf = Vec::new();
                stream.read_to_end(&mut buf).await.unwrap();

                stream.write_from_fin(&mut &buf[..]).await.unwrap();
            });
        }
    });

    let mut stream = client.connect_to(&server_handle).await.unwrap();

    stream.write_from_fin(&mut &b"hello"[..]).await.unwrap();

    let mut res = Vec::new();
    stream.read_to_end(&mut res).await.unwrap();
    assert_eq!(res, b"hello");
}

macro_rules! tests {
    () => {
        #[tokio::test]
        async fn ipv4_to_ipv4() {
            super::test_addr("127.0.0.1:0", "127.0.0.1:0").await
        }

        #[tokio::test]
        async fn ipv4_to_ipv6() {
            super::test_addr("127.0.0.1:0", "[::1]:0").await
        }

        #[tokio::test]
        async fn ipv6_to_ipv4() {
            super::test_addr("[::1]:0", "127.0.0.1:0").await
        }

        #[tokio::test]
        async fn ipv6_to_ipv6() {
            super::test_addr("[::1]:0", "[::1]:0").await
        }
    };
}

mod tcp {
    tests!();
}

mod udp {
    tests!();
}
