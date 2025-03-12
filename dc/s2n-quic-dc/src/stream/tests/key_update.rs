// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    path::secret::seal::TEST_MAX_RECORDS,
    stream::testing::{Client, Server},
    testing::init_tracing,
};
use std::{io, sync::atomic::Ordering, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info_span, Instrument};

/// This test checks that the sealer stream key and opener stream key are updated when
/// more than the confidentiality limit of packets are transmitted.
#[tokio::test]
#[cfg_attr(
    not(debug_assertions),
    ignore = "test requires debug_assertions to be enabled"
)]
async fn key_update() {
    test(TEST_MAX_RECORDS, 1).await;
}

/// This test checks that the sealer stream key and opener stream key are not updated when
/// less than the confidentiality limit of packets are transmitted.
#[tokio::test]
async fn no_key_update() {
    test(TEST_MAX_RECORDS - 10, 0).await;
}

async fn test(num_packets: u64, expected_key_updates: u64) {
    init_tracing();

    let client = Client::default();
    let server = Server::tcp().build();
    let client_subscriber = client.subscriber();
    let server_subscriber = server.subscriber();

    tokio::try_join!(
        async {
            let mut a = client.connect_to(&server).await?;
            // send enough packets to trigger a key update. This value is lower
            // when debug_assertions are enabled (see TEST_MAX_RECORDS in path/secret/key.rs)
            for _ in 0..num_packets {
                let _ = a.write_all(b"testing").await;
            }

            // wait some time before calling shutdown in case the server reset the connection so we
            // can observe it in `shutdown`
            tokio::time::sleep(Duration::from_millis(10)).await;

            let _ = a.shutdown().await;

            let mut buffer = vec![];
            a.read_to_end(&mut buffer).await?;
            assert_eq!(buffer, b"done");
            Ok::<(), io::Error>(())
        }
        .instrument(info_span!("client")),
        async {
            let (mut b, _) = server.accept().await.expect("accept");
            let mut buffer = vec![];
            b.read_to_end(&mut buffer).await.unwrap();

            b.write_all(b"done").await.unwrap();
            b.shutdown().await.unwrap();

            Ok(())
        }
        .instrument(info_span!("server"))
    )
    .unwrap();

    assert_eq!(
        expected_key_updates,
        client_subscriber
            .stream_write_key_updated
            .load(Ordering::Relaxed)
    );
    assert_eq!(
        expected_key_updates,
        server_subscriber
            .stream_read_key_updated
            .load(Ordering::Relaxed)
    );
}
