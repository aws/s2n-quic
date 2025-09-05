// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Context;
use core::time::Duration;
use std::io::ErrorKind;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::timeout,
};

/// Q: What happens when the application writes an empty buffer?
///
/// A: The operation returns immediately
#[tokio::test]
async fn zero_write_test() {
    let context = Context::new().await;
    let (mut client, server) = context.pair().await;

    client.write_all(&[]).await.unwrap();

    drop(server);
}

/// Q: What happens when the application writes an empty buffer but the peer is closed?
///
/// A: The operation returns immediately - no error
#[tokio::test]
async fn zero_write_reset_test() {
    let context = Context::new().await;
    let (mut client, server) = context.pair().await;

    drop(server);
    tokio::time::sleep(Duration::from_millis(1)).await;

    client.write_all(&[]).await.unwrap();
}

/// Q: What happens when the application writes and shuts down but the peer never reads
///    any bytes?
///
/// A: It returns immediately
#[tokio::test]
async fn unresponsive_shutdown_test() {
    let context = Context::new().await;
    let (mut client, server) = context.pair().await;

    client.write_all(b"hello!").await.unwrap();
    client.shutdown().await.unwrap();

    drop(server);
}

/// Q: What happens when the application shuts down and then attempts to write?
///
/// A: It returns a BrokenPipe error
#[tokio::test]
async fn write_after_shutdown_test() {
    let context = Context::new().await;
    let (mut client, server) = context.pair().await;

    client.shutdown().await.unwrap();

    let err = client.write_all(b"hello!").await.unwrap_err();
    assert_eq!(err.kind(), ErrorKind::BrokenPipe);

    drop(server);
}

/// Q: What happens when the application shuts down and then attempts to write an empty buffer?
///
/// A: It returns `Ok`
#[tokio::test]
async fn empty_write_after_shutdown_test() {
    let context = Context::new().await;
    let (mut client, server) = context.pair().await;

    client.shutdown().await.unwrap();
    client.write_all(&[]).await.unwrap();

    drop(server);
}

/// Q: What happens when the application shuts down multiple times
///
/// A: It returns `Ok`
#[tokio::test]
async fn multiple_shutdown_test() {
    let context = Context::new().await;
    let (mut client, server) = context.pair().await;

    for _ in 0..3 {
        client.shutdown().await.unwrap();
    }

    drop(server);
}

/// Q: What happens when the client shuts down and then looks at the peer/local addr?
///
/// A: It returns `Ok`
#[tokio::test]
async fn addr_after_half_close_test() {
    let context = Context::new().await;
    let (mut client, mut server) = context.pair().await;

    client.shutdown().await.unwrap();

    // observer the peer's shutdown
    let _ = server.read(&mut []).await;

    client.local_addr().unwrap();
    client.peer_addr().unwrap();

    server.local_addr().unwrap();
    server.peer_addr().unwrap();
}

/// Q: What happens when both sides shut down and then looks at the peer/local addr?
///
/// A: It returns `Ok`
#[tokio::test]
async fn addr_after_full_shutdown_test() {
    let context = Context::new().await;
    let (mut client, mut server) = context.pair().await;

    // This works around the server needing to tell the client which port it migrated to before
    // calling `shutdown`.
    // TODO fix this and remove this exception
    if context.protocol().is_udp() {
        let _ = timeout(Duration::from_millis(5), server.read(&mut [])).await;
        let _ = timeout(Duration::from_millis(5), client.read(&mut [])).await;
    }

    client.shutdown().await.unwrap();
    server.shutdown().await.unwrap();

    // observe the peer's shutdown
    let _ = client.read(&mut []).await;
    let _ = server.read(&mut []).await;

    let expected_err = if cfg!(target_os = "macos") {
        ErrorKind::InvalidInput
    } else {
        ErrorKind::NotConnected
    };

    client.local_addr().unwrap();
    let err = client.peer_addr().unwrap_err();
    assert_eq!(err.kind(), expected_err);

    // This check is flaky for UDP
    // TODO fix this and remove this exception
    if !context.protocol().is_udp() {
        server.local_addr().unwrap();
        let err = server.peer_addr().unwrap_err();
        assert_eq!(err.kind(), expected_err);
    }
}

/// Q: What happens when the client shuts down the stream but continues to read?
///
/// A: The open half of the stream continues to function as normal
#[tokio::test]
async fn half_close_read_test() {
    let context = Context::new().await;
    let (mut client, mut server) = context.pair().await;

    client.shutdown().await.unwrap();

    let mut buffer = vec![];
    server.read_to_end(&mut buffer).await.unwrap();

    assert!(buffer.is_empty());

    server.write_all(b"hello!").await.unwrap();

    buffer.resize(10, 0);
    let len = client.read(&mut buffer).await.unwrap();
    assert_eq!(&buffer[..len], b"hello!");

    server.shutdown().await.unwrap();

    let len = client.read(&mut buffer).await.unwrap();
    assert_eq!(len, 0);
}
