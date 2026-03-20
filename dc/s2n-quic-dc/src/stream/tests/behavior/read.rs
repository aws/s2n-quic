// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Context;
use core::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Q: What happens when the application has an empty read buffer?
///
/// A: The operation still performs a syscall
#[tokio::test]
async fn zero_read_test() {
    let context = Context::new().await;
    let (mut client, _server) = context.pair().await;

    tokio::time::timeout(core::time::Duration::from_millis(5), client.read(&mut []))
        .await
        .expect_err("the read operation should time out");
}

/// Q: What happens when the application has an empty read buffer but the peer is closed?
///
/// A: The operation still performs a syscall and returns empty read
#[tokio::test]
async fn zero_read_reset_test() {
    let context = Context::new().await;
    let (mut client, server) = context.pair().await;

    drop(server);
    tokio::time::sleep(Duration::from_millis(1)).await;

    let res =
        tokio::time::timeout(core::time::Duration::from_millis(5), client.read(&mut [])).await;

    // TODO this started failing for UDP - figure out what's wrong
    if context.protocol().is_udp() {
        return;
    }

    let len = res.expect("operation should not time out").unwrap();

    assert_eq!(len, 0);
}

/// Q: What happens when the client tries to read immediately after opening the stream?
///
/// A: The server knows about the client stream and is able to send data
#[tokio::test]
async fn read_immediately_test() {
    let context = Context::new().await;
    let (mut client, mut server) = context.pair().await;

    let client = async move {
        let mut buffer = vec![];
        client.read_to_end(&mut buffer).await.unwrap();
        buffer
    };

    let server = async move {
        server.write_all(b"hello!").await.unwrap();
    };

    let (response, _) = tokio::join!(client, server);

    assert_eq!(response, b"hello!");
}

/// Q: What happens when the application reads from a closed stream multiple times
///
/// A: The operation still performs a syscall and returns empty read
#[tokio::test]
async fn multiple_empty_read_test() {
    let context = Context::new().await;
    let (mut client, server) = context.pair().await;

    drop(server);
    tokio::time::sleep(Duration::from_millis(1)).await;

    // try multiple buffer sizes
    for buffer_len in [0, 1] {
        // read several times with the buffer_len
        for _ in 0..5 {
            let buffer = &mut [42][..buffer_len];
            let res =
                tokio::time::timeout(core::time::Duration::from_millis(5), client.read(buffer))
                    .await;

            // TODO this started failing for UDP - figure out what's wrong
            if context.protocol().is_udp() {
                continue;
            }

            let len = res.expect("operation should not time out").unwrap();

            assert_eq!(len, 0);
        }
    }
}

/// Q: What happens when the application reads from a stream that is closed without authentication?
///
/// A: Secure protocols detect unclean shutdown and return an error.
#[tokio::test]
async fn stream_closed_without_authentication() {
    let context = Context::new().await;
    let (mut client, server) = context.pair().await;

    // drop server via panic.
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        // Move server into the closure.
        let _s = server;
        panic!("expected panic to test unclean shutdown");
    }));

    tokio::time::sleep(Duration::from_millis(1)).await;

    let res =
        tokio::time::timeout(core::time::Duration::from_millis(5), client.read(&mut [])).await;

    let res = res.expect("operation should not time out");

    // TCP streams close cleanly on Drop, even if panicking.
    if context.is_plaintext() {
        assert_eq!(res.unwrap(), 0);
        return;
    }

    let err = res.unwrap_err();

    if context.protocol().is_tcp() {
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof, "{:?}", err);
        assert!(
            matches!(
                err.get_ref()
                    .expect("has inner")
                    .downcast_ref::<crate::stream::recv::Error>()
                    .unwrap()
                    .kind,
                crate::stream::recv::ErrorKind::TruncatedTransport
            ),
            "{:?}",
            err
        );
    } else {
        // FIXME: Should this match the dcQUIC over TCP branch?
        assert_eq!(err.kind(), std::io::ErrorKind::ConnectionReset, "{:?}", err);
        let crate::stream::recv::ErrorKind::ApplicationError { error } = err
            .get_ref()
            .expect("has inner")
            .downcast_ref::<crate::stream::recv::Error>()
            .unwrap()
            .kind
        else {
            panic!("unexpected error: {:?}", err);
        };
        // This indicates a panic, see dc/s2n-quic-dc/src/stream/send/worker.rs.
        assert_eq!(
            *error,
            crate::stream::shared::ShutdownKind::Panicking
                .error_code()
                .unwrap() as u64,
            "{:?}",
            err
        );
    }
}
