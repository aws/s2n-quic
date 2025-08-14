// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stream::testing::{Client, Server},
    testing::init_tracing,
};
use std::{io, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info_span, Instrument};

async fn check_stream(client: &Client, server: &Server) -> io::Result<()> {
    tokio::try_join!(
        async {
            let mut a = client.connect_to(server).await?;
            let _ = a.write_all(b"testing").await;

            // wait some time before calling shutdown in case the server reset the connection so we
            // can observe it in `shutdown`
            tokio::time::sleep(Duration::from_millis(10)).await;

            let _ = a.shutdown().await;

            let mut buffer = vec![];
            a.read_to_end(&mut buffer).await?;
            assert_eq!(buffer, b"testing");
            Ok(())
        }
        .instrument(info_span!("client")),
        async {
            let (mut b, _) = server.accept().await.expect("accept");
            let mut buffer = vec![];
            b.read_to_end(&mut buffer).await.unwrap();
            assert_eq!(buffer, b"testing");

            b.write_all(&buffer).await.unwrap();
            b.shutdown().await.unwrap();

            Ok(())
        }
        .instrument(info_span!("server"))
    )
    .map(|_| ())
}

#[tokio::test]
async fn failed_packet() {
    init_tracing();

    let client = Client::default();
    let server = Server::tcp().build();
    let mut stream = tokio::net::TcpStream::connect(server.local_addr())
        .await
        .unwrap();
    // First write succeeds.
    stream
        .write_all(b"this is not a dcQUIC message")
        .await
        .unwrap();
    // Note: We do *not* shutdown the stream here, we expect the server to end the stream on its
    // side since we wrote bad data.
    let mut err = vec![];
    let kind = stream
        .read_to_end(&mut err)
        .await
        .expect_err("the server should reset the connection")
        .kind();
    assert_eq!(kind, io::ErrorKind::ConnectionReset);
    // We currently silently drop malformed streams, ending them with an EOF.
    assert_eq!(err.len(), 0);

    // Confirm subsequent streams connect successfully.
    check_stream(&client, &server).await.unwrap();
}

#[tokio::test]
async fn immediate_eof() {
    init_tracing();

    let client = Client::default();
    let server = Server::tcp().build();
    let mut stream = tokio::net::TcpStream::connect(server.local_addr())
        .await
        .unwrap();
    // Immediately end the stream without any data being sent.
    stream.shutdown().await.unwrap();
    let mut err = vec![];
    let kind = stream
        .read_to_end(&mut err)
        .await
        .expect_err("the server should reset the connection")
        .kind();
    assert_eq!(kind, io::ErrorKind::ConnectionReset);
    // We currently silently drop malformed streams, ending them with an EOF.
    assert_eq!(err.len(), 0);

    // Confirm subsequent streams connect successfully.
    check_stream(&client, &server).await.unwrap();
}

// Confirm that we can use all of the concurrency for streams that have not yet sent a prelude.
#[tokio::test]
async fn within_concurrency() {
    init_tracing();

    let client = Client::default();
    let concurrent = 300;
    let server = Server::tcp().backlog(concurrent).build();

    client.handshake_with(&server).unwrap();

    let mut pending_streams = vec![];
    for _ in 0..concurrent {
        let stream = tokio::net::TcpStream::connect(server.local_addr())
            .await
            .unwrap();
        pending_streams.push(stream);
    }
    for stream in pending_streams {
        // Effectively this just writes the prelude.
        let mut stream = client.connect_tcp_with(&server, stream).await.unwrap();
        // Confirm stream actually opened..
        stream.write_from(&mut &[0x3u8; 100][..]).await.unwrap();
    }
}

// Exercise dropping connections when we go over the allowed concurrency.
#[tokio::test]
async fn graceful_surpassing_concurrency() {
    init_tracing();

    let client = Client::default();
    let concurrent = 5;
    let server = Server::tcp().backlog(concurrent).build();

    client.handshake_with(&server).unwrap();

    let mut streams = vec![];
    for _ in 0..(concurrent * 2) {
        let stream = tokio::net::TcpStream::connect(server.local_addr())
            .await
            .unwrap();
        streams.push(stream);
        tokio::task::yield_now().await;
    }

    let server_handle = server.handle();

    tokio::task::spawn(async move {
        while let Ok((mut stream, _peer_addr)) = server.accept().await {
            let _ = stream.write_from(&mut &b"hello"[..]).await;
            let _ = stream.shutdown().await;
            drop(stream);
        }
    });

    // Need to give time for server to drop the streams.
    // Increased because of TCP_DEFER_ACCEPT delaying actual accept because we don't actually write
    // anything to the stream.
    tokio::time::sleep(Duration::from_secs(5)).await;

    let mut errors = 0;
    let mut ok = 0;

    for stream in streams {
        let mut stream = client
            .connect_tcp_with(&server_handle, stream)
            .await
            .unwrap();
        let mut out = s2n_quic_core::buffer::writer::storage::Discard;
        let res = stream.read_into(&mut out).await;
        match res {
            Ok(_) => ok += 1,
            Err(_e) => errors += 1,
        }
    }

    assert_eq!(errors + ok, concurrent * 2);
    assert_eq!(errors, concurrent);
    assert_eq!(ok, concurrent);
}
