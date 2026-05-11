// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::Context;
use core::time::Duration;
use std::panic::{catch_unwind, AssertUnwindSafe};
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

/// Q: What happens when the server uses read_exact to consume the payload but does not
///    read the authenticated FIN frame, then panics (dropping the stream without sending
///    the authenticated closure)?
///
/// A: The stream's shutdown path drains the FIN from the kernel's TCP receive buffer
///    before closing the fd, so close() sends a clean TCP FIN rather than RST. The client
///    observes UnexpectedEof (TruncatedTransport — no authenticated closure) rather than
///    ConnectionReset (which would indicate the kernel sent RST due to unread data).
#[tokio::test]
async fn read_exact_missing_fin_no_rst() {
    let context = Context::new().await;

    // The dcQUIC over UDP transmits an explicit message on panic! drop which is translated as a
    // ConnectionReset. For now skip the test there as a result.
    if !context.protocol().is_tcp() {
        return;
    }

    // Any non-zero size works: the read_done channel ensures the server's read_exact
    // completes before the client sends the FIN frame, so payload size doesn't affect
    // whether the FIN ends up sitting unread in the kernel recv buffer.
    let payload_size = 16;

    let (mut client, mut server) = context.pair().await;

    let (read_done_tx, read_done_rx) = tokio::sync::oneshot::channel::<()>();

    let client_handle = tokio::spawn(async move {
        let payload = vec![42u8; payload_size];
        client.write_all(&payload).await.unwrap();

        // Wait for the server to consume the payload via read_exact
        let _ = read_done_rx.await;

        // Client shuts down write side — this writes the dcquic FIN frame which
        // lands in the server's kernel recv buffer
        client.shutdown().await.unwrap();

        // Give time for the FIN frame to arrive at the server
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Client tries to read — should get UnexpectedEof (TruncatedTransport),
        // not ConnectionReset (which would mean the kernel sent RST)
        let mut buf = vec![0u8; 1];
        let err = client.read_exact(&mut buf).await.unwrap_err();
        assert_eq!(
            err.kind(),
            std::io::ErrorKind::UnexpectedEof,
            "Expected UnexpectedEof (TruncatedTransport from drained shutdown), \
             got {err:?} — if ConnectionReset, the drain-on-shutdown fix is broken"
        );
    });

    let server_handle = tokio::spawn(async move {
        // Server reads exactly the payload — does NOT consume the FIN frame
        let mut buf = vec![0u8; payload_size];
        server.read_exact(&mut buf).await.unwrap();

        let _ = read_done_tx.send(());

        // Wait for client's FIN frame to arrive in our kernel recv buffer
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Panic to drop the stream without sending the authenticated closure.
        // The stream is moved into the closure so it is dropped during unwind
        // (when std::thread::panicking() == true). This causes the writer to skip
        // the authenticated FIN, but the reader still drains the kernel recv
        // buffer — preventing RST.
        let _ = catch_unwind(AssertUnwindSafe(move || {
            let _server = server;
            panic!("intentional panic to skip authenticated closure");
        }));
    });

    server_handle.await.unwrap();
    client_handle.await.unwrap();
}
