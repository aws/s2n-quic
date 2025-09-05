// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stream::{testing::dcquic::Context, Protocol},
    testing::server_name,
};
use std::{io, time::Duration};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info_span, Instrument};

#[derive(Clone, Debug)]
enum Side {
    Client,
    Server,
    Both,
}

#[derive(Clone, Debug)]
struct Harness {
    protocol: Protocol,
    drop: Side,
}

impl Default for Harness {
    fn default() -> Self {
        Self {
            protocol: Protocol::Udp,
            drop: Side::Both,
        }
    }
}

// FIXME: maybe needs more coverage for various cases (e.g., server never touches the stream,
// client never touches the stream, etc.)
async fn check_stream(
    context: &Context,
    bidirectional: bool,
    sleep_before_shutdown: bool,
) -> io::Result<()> {
    // we don't use `context.pair()` here since the `server.accept` call won't return if the stream
    // is invalid
    let handshake_addr = context.handshake_addr();
    let acceptor_addr = context.acceptor_addr();
    tokio::try_join!(
        async {
            let mut a = context
                .client
                .connect(handshake_addr, acceptor_addr, server_name())
                .await?;
            let _ = a.write_all(b"testing").await;

            // wait some time before calling shutdown in case the server reset the connection so we
            // can observe it in `shutdown`
            if sleep_before_shutdown {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }

            let _ = a.shutdown().await;

            if bidirectional {
                let mut buffer = vec![];
                a.read_to_end(&mut buffer).await?;
                assert_eq!(buffer, b"testing");
            }
            Ok(())
        }
        .instrument(info_span!("client")),
        async {
            let (mut b, _) = context.server.accept().await.expect("accept");
            let mut buffer = vec![];
            b.read_to_end(&mut buffer).await.unwrap();
            assert_eq!(buffer, b"testing");

            if bidirectional {
                b.write_all(&buffer).await.unwrap();
                b.shutdown().await.unwrap();
            }

            Ok(())
        }
        .instrument(info_span!("server"))
    )
    .map(|_| ())
}

impl Harness {
    async fn run(self) {
        for bidirectional in [false, true] {
            for sleep_before_shutdown in [false, true] {
                dbg!(bidirectional, sleep_before_shutdown);
                let task = self.run_one(bidirectional, sleep_before_shutdown);
                tokio::time::timeout(core::time::Duration::from_secs(60), task)
                    .await
                    .expect("test timed out after 60 seconds");
            }
        }
    }

    async fn run_one(&self, bidirectional: bool, sleep_before_shutdown: bool) {
        tracing::info!(bidirectional, sleep_before_shutdown);
        tracing::info!("About to create context!");
        let context = Context::bind(self.protocol, "127.0.0.1:0".parse().unwrap()).await;

        tracing::info!("Context created!");

        check_stream(&context, bidirectional, sleep_before_shutdown)
            .instrument(info_span!("first"))
            .await
            .unwrap();

        tracing::info!("First check stream succeeded.");

        match self.drop {
            Side::Client => context.client.drop_state(),
            Side::Server => context.server.drop_state(),
            Side::Both => {
                context.client.drop_state();
                context.server.drop_state();
            }
        }

        tracing::info!("Restart started!");

        // This might fail, we don't care. At least two streams should fail before we
        // manage to successfully establish after dropping state.
        tracing::info!(
            "initial: {:?}",
            tokio::time::timeout(
                Duration::from_secs(2),
                check_stream(&context, bidirectional, sleep_before_shutdown)
            )
            .instrument(info_span!("second"))
            .await
        );

        // Wait for the asynchronous background handshake.
        tokio::time::sleep(Duration::from_secs(2)).await;

        // This should enqueue a recovery handshake. This used to be something we'd *wait* for, but
        // now we just do that in the background; this should still fail.
        tracing::info!(
            "recovery handshake: {:?}",
            tokio::time::timeout(
                Duration::from_secs(2),
                check_stream(&context, bidirectional, sleep_before_shutdown)
            )
            .instrument(info_span!("third"))
            .await
        );

        // Wait for the asynchronous background handshake.
        tokio::time::sleep(Duration::from_millis(50)).await;

        check_stream(&context, bidirectional, sleep_before_shutdown)
            .instrument(info_span!("four"))
            .await
            .unwrap();
    }
}

macro_rules! tests {
    () => {
        #[tokio::test]
        async fn client() {
            Harness {
                drop: Side::Client,
                ..harness()
            }
            .run()
            .await
        }

        #[tokio::test]
        async fn server() {
            Harness {
                drop: Side::Server,
                ..harness()
            }
            .run()
            .await
        }

        #[tokio::test]
        async fn both() {
            Harness {
                drop: Side::Both,
                ..harness()
            }
            .run()
            .await
        }
    };
}

mod tcp {
    use super::*;

    fn harness() -> Harness {
        Harness {
            protocol: Protocol::Tcp,
            ..Default::default()
        }
    }

    tests!();
}

mod udp {
    use super::*;

    fn harness() -> Harness {
        Harness {
            protocol: Protocol::Udp,
            ..Default::default()
        }
    }

    tests!();
}
