// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stream::{socket::Protocol, testing},
    testing::{ext::*, sleep, spawn, timeout},
};
use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::Instrument as _;

fn pair(protocol: Protocol) -> (testing::Client, testing::Server) {
    let client = testing::Client::default();
    let server = testing::Server::builder().protocol(protocol).build();
    (client, server)
}

bitflags::bitflags!(
    #[derive(Clone, Copy, Debug, Default)]
    struct TestFeatures: u16 {
        const EXPLICIT_SHUTDOWN = 1 << 0;
        const FLOW_LIMITED = 1 << 1;
        const SEND_LIMITED = 1 << 2;
        const RECV_LIMITED = 1 << 3;
    }
);

async fn run(protocol: Protocol, buffer_len: usize, iterations: usize, features: TestFeatures) {
    timeout(
        core::time::Duration::from_secs(120),
        run_impl(protocol, buffer_len, iterations, features),
    )
    .await
    .unwrap()
}

async fn run_impl(
    protocol: Protocol,
    buffer_len: usize,
    iterations: usize,
    features: TestFeatures,
) {
    let (client, server) = pair(protocol);
    let server_handle = server.handle();

    let (server_response, client_response) = tokio::sync::oneshot::channel();

    spawn(
        async move {
            let mut server_response = Some(server_response);
            loop {
                let (mut stream, _peer_addr) = server.accept().await.unwrap();
                let server_response = server_response.take().unwrap();

                let total = Arc::new(AtomicUsize::new(0));

                spawn({
                    let total = total.clone();
                    async move {
                        let mut prev = 0;
                        loop {
                            sleep(core::time::Duration::from_secs(1)).await;
                            let total = total.load(Ordering::Relaxed);
                            let gbps = (total - prev) as f64 * 8e-9;
                            prev = total;
                            println!("total={total} gbps={gbps:.2}");
                        }
                    }
                });

                spawn(
                    async move {
                        let mut data = vec![0; 1 << 17];
                        loop {
                            let Ok(len) = stream.read(&mut data).await else {
                                break;
                            };
                            if len == 0 {
                                break;
                            }
                            total.fetch_add(len, Ordering::Relaxed);
                            if features.contains(TestFeatures::RECV_LIMITED) {
                                sleep(core::time::Duration::from_millis(1)).await;
                            }
                        }
                        let _ = server_response.send(total.load(Ordering::Relaxed));
                    }
                    .instrument(tracing::debug_span!("stream")),
                );
            }
        }
        .instrument(tracing::debug_span!("server")),
    );

    let expected = buffer_len * iterations;
    println!("expected={expected}");

    spawn({
        let client = client.clone();

        async move {
            let mut stream = client.connect_to(&server_handle).await.unwrap();
            let mut total = 0;
            let buffer = vec![0; buffer_len];
            for _ in 0..iterations {
                stream.write_all(&buffer).await.unwrap();
                total += buffer.len();
                if features.contains(TestFeatures::SEND_LIMITED) {
                    sleep(core::time::Duration::from_millis(1)).await;
                }
            }
            assert_eq!(total, expected);

            if features.contains(TestFeatures::EXPLICIT_SHUTDOWN) {
                let _ = stream.shutdown().await;
            }
        }
        .instrument(tracing::debug_span!("client"))
    });

    let actual = client_response.await.unwrap();
    assert_eq!(expected, actual);

    sleep(core::time::Duration::from_secs(1)).await;

    // make sure the client lives long enough to complete the streams
    drop(client);

    // TODO make sure the worker shut down correctly
    //worker.await.unwrap();
}

macro_rules! suite {
    ($flavor:ident) => {
        mod $flavor {
            use super::{TestFeatures as F, *};

            fn large_times() -> usize {
                std::env::var("S2N_QUIC_DC_LARGE_TIMES")
                    .ok()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(100)
            }

            suite!($flavor, empty, 0, 0, F::default());

            suite!($flavor, write_1k, 1000);
            suite!($flavor, write_10k, 10_000);
            suite!($flavor, write_100k, 100_000);
            suite!($flavor, write_100k_10_times, 100_000, 10);
            suite!($flavor, write_100k_x_times, 100_000, large_times());
        }
    };
    ($flavor:ident, $name:ident, $size:expr) => {
        suite!($flavor, $name, $size, 1);
    };
    ($flavor:ident, $name:ident, $size:expr, $times:expr, $features:expr) => {
        mod $name {
            use super::*;

            $flavor!(drop_test, PROTOCOL, $size, $times, $features);
            $flavor!(
                shutdown_test,
                PROTOCOL,
                $size,
                $times,
                $features | F::EXPLICIT_SHUTDOWN
            );
        }
    };
    ($flavor:ident, $name:ident, $size:expr, $times:expr) => {
        mod $name {
            use super::*;

            suite!($flavor, send_limited, $size, $times, F::SEND_LIMITED);
            suite!($flavor, recv_limited, $size, $times, F::RECV_LIMITED);
            suite!($flavor, flow_limited, $size, $times, F::FLOW_LIMITED);
            suite!($flavor, congestion_limited, $size, $times, F::default());
        }
    };
}

macro_rules! negative_suite {
    () => {
        mod negative {
            use super::*;

            #[tokio::test]
            async fn unresponsive_reader_test() {
                let (client, server) = pair(PROTOCOL);
                let server_handle = server.handle();

                spawn(
                    async move {
                        loop {
                            let (stream, _peer_addr) = server.accept().await.unwrap();

                            spawn(
                                async move {
                                    let () = core::future::pending().await;
                                    drop(stream);
                                }
                                .instrument(tracing::debug_span!("stream")),
                            );
                        }
                    }
                    .instrument(tracing::debug_span!("server")),
                );

                let application = tokio::spawn(
                    async move {
                        let mut stream = client.connect_to(&server_handle).await.unwrap();
                        stream.write_all(b"hello!").await?;
                        stream.shutdown().await
                    }
                    .instrument(tracing::debug_span!("application")),
                );

                // the application should succeed, even if the server didn't respond
                application.await.unwrap().unwrap();
            }

            #[tokio::test]
            async fn panicking_writer_test() {
                let (client, server) = pair(PROTOCOL);
                let server_handle = server.handle();

                let (server_response, client_response) = tokio::sync::oneshot::channel();

                spawn(
                    async move {
                        let mut server_response = Some(server_response);
                        loop {
                            let (mut stream, _peer_addr) = server.accept().await.unwrap();
                            let server_response = server_response.take().unwrap();

                            spawn(
                                async move {
                                    let mut buffer = vec![];
                                    let _ =
                                        server_response.send(stream.read_to_end(&mut buffer).await);
                                }
                                .instrument(tracing::debug_span!("stream")),
                            );
                        }
                    }
                    .instrument(tracing::debug_span!("server")),
                );

                spawn(
                    async move {
                        let mut stream = client.connect_to(&server_handle).await.unwrap();
                        let _ = stream.write_all(b"hello!").await;
                        panic!("the application panicked (as expected)!");
                    }
                    .instrument(tracing::debug_span!("application")),
                );

                match client_response.await {
                    Ok(Err(_)) => {}
                    other => {
                        panic!("unexpected result {other:?}");
                    }
                }
            }
        }
    };
}

macro_rules! current_thread {
    ($name:ident, $protocol:expr, $buffer_len:expr, $iterations:expr, $features:expr) => {
        #[tokio::test]
        async fn $name() {
            run($protocol, $buffer_len, $iterations, $features).await;
        }
    };
}

macro_rules! multi_thread {
    ($name:ident, $protocol:expr, $buffer_len:expr, $iterations:expr, $features:expr) => {
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn $name() {
            run($protocol, $buffer_len, $iterations, $features).await;
        }
    };
}

macro_rules! sim {
    ($name:ident, $protocol:expr, $buffer_len:expr, $iterations:expr, $features:expr) => {
        #[test]
        fn $name() {
            crate::testing::sim(|| {
                async {
                    run($protocol, $buffer_len, $iterations, $features).await;
                }
                .primary()
                .spawn();
            });
        }
    };
}

mod tcp {
    use super::*;
    const PROTOCOL: Protocol = Protocol::Tcp;

    suite!(current_thread);
    suite!(multi_thread);
    negative_suite!();
}

#[cfg(target_os = "linux")] // things are only working on linux right now
mod udp {
    use super::*;
    const PROTOCOL: Protocol = Protocol::Udp;

    suite!(current_thread);
    suite!(multi_thread);
    suite!(sim);
    negative_suite!();
}
