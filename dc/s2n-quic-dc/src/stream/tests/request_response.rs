// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::{
    socket::Protocol,
    testing::{self, Stream, MAX_DATAGRAM_SIZE},
};
use bolero::{produce, TypeGenerator, ValueGenerator as _};
use core::time::Duration;
use s2n_quic_core::{buffer::writer::Storage as _, stream::testing::Data};
use std::{io, sync::Arc};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    time::sleep,
};
use tracing::{info, info_span, Instrument};

#[derive(Clone, Copy, Debug, Default, TypeGenerator)]
struct Delays {
    #[generator(Duration::ZERO..Duration::from_millis(2))]
    read: Duration,
    #[generator(Duration::ZERO..Duration::from_millis(2))]
    write: Duration,
    #[generator(Duration::ZERO..Duration::from_millis(2))]
    shutdown_write: Duration,
    #[generator(Duration::ZERO..Duration::from_millis(2))]
    shutdown_read: Duration,
    #[generator(Duration::ZERO..Duration::from_millis(2))]
    drop: Duration,
}

macro_rules! delay {
    ($field:ident) => {
        async fn $field(&self) {
            if !self.$field.is_zero() {
                info!(delay = %stringify!($field), duration = ?self.$field);
            }
            sleep(self.$field).await
        }
    }
}

impl Delays {
    delay!(read);
    delay!(write);
    delay!(shutdown_write);
    delay!(shutdown_read);
    delay!(drop);
}

#[derive(Clone, Copy, Debug, TypeGenerator)]
struct Request {
    #[generator(1..10)]
    count: usize,
    #[generator(1..100_000)]
    request_size: usize,
    #[generator(1..100_000)]
    response_size: usize,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            count: 1,
            request_size: 1000,
            response_size: 2000,
        }
    }
}

#[derive(Clone, Copy, Debug, TypeGenerator)]
struct Client {
    delays: Delays,
    #[generator(1..5)]
    count: usize,
    #[generator(1..5)]
    concurrency: usize,
    #[generator(1..u16::MAX as usize)]
    max_read_len: usize,
    #[generator((1250..MAX_DATAGRAM_SIZE).map_gen(Some))]
    max_mtu: Option<u16>,
}

impl Default for Client {
    fn default() -> Self {
        Self {
            delays: Default::default(),
            count: 1,
            concurrency: 1,
            max_read_len: usize::MAX,
            max_mtu: None,
        }
    }
}

impl Client {
    fn build(&self) -> testing::Client {
        let mut builder = testing::Client::builder();
        if let Some(max_mtu) = self.max_mtu {
            builder = builder.mtu(max_mtu);
        }
        builder.build()
    }
}

#[derive(Clone, Copy, Debug, TypeGenerator)]
struct Server {
    delays: Delays,
    #[generator(1..5)]
    count: usize,
    #[generator(1..u16::MAX as usize)]
    max_read_len: usize,
    #[generator((1250..MAX_DATAGRAM_SIZE).map_gen(Some))]
    max_mtu: Option<u16>,
}

impl Default for Server {
    fn default() -> Self {
        Self {
            delays: Default::default(),
            max_read_len: usize::MAX,
            max_mtu: None,
            count: 1,
        }
    }
}

impl Server {
    fn build(&self, protocol: Protocol) -> testing::Server {
        let mut builder = testing::Server::builder().protocol(protocol);
        if let Some(max_mtu) = self.max_mtu {
            builder = builder.mtu(max_mtu);
        }
        builder.build()
    }
}

#[derive(Clone, Debug)]
struct Harness {
    protocol: Protocol,
    requests: Vec<Request>,
    client: Client,
    server: Server,
}

impl Default for Harness {
    fn default() -> Self {
        Self {
            protocol: Protocol::Udp,
            requests: vec![Request::default()],
            client: Default::default(),
            server: Default::default(),
        }
    }
}

async fn check_read(
    stream: &mut Stream,
    delays: &Delays,
    amount: usize,
    max_read_len: usize,
) -> io::Result<()> {
    delays.read().await;
    info!(reading = amount);
    let mut data = Data::new(amount as _);
    while !data.is_finished() {
        let mut data = data.with_write_limit(max_read_len);
        let len = stream.read_into(&mut data).await?;
        if len == 0 {
            break;
        }
    }
    info!(read = data.offset());
    assert!(data.is_finished());
    Ok(())
}

async fn check_write(stream: &mut Stream, delays: &Delays, amount: usize) -> io::Result<()> {
    delays.write().await;
    info!(writing = amount);
    let mut data = Data::new(amount as _);
    while !data.is_finished() {
        stream.write_from(&mut data).await?;
    }
    info!(wrote = amount);
    Ok(())
}

async fn check_shutdown_read(stream: &mut Stream, delays: &Delays) -> io::Result<()> {
    delays.shutdown_read().await;
    info!(shutting_down = "read");
    assert_eq!(0, stream.read(&mut [0]).await?);
    info!(shutdown = "read");
    Ok(())
}

async fn check_shutdown_write(stream: &mut Stream, delays: &Delays) -> io::Result<()> {
    delays.shutdown_write().await;
    info!(shutting_down = "write");
    stream.shutdown().await?;
    info!(shutdown = "write");
    Ok(())
}

async fn check_server(mut stream: Stream, server: Server, requests: Arc<[Request]>) {
    let mut idx = 0;

    for request in requests.iter() {
        info!(?request);

        for _ in 0..request.count {
            let span = info_span!("request", request = idx);
            check_read(
                &mut stream,
                &server.delays,
                request.request_size,
                server.max_read_len,
            )
            .instrument(span.clone())
            .await
            .unwrap();
            check_write(&mut stream, &server.delays, request.response_size)
                .instrument(span)
                .await
                .unwrap();

            // increment the request counter
            idx += 1;
        }
    }

    let _ = check_shutdown_write(&mut stream, &server.delays).await;
    let _ = check_shutdown_read(&mut stream, &server.delays).await;

    server.delays.drop().await;
    info!("dropping stream");
}

async fn check_client(mut stream: Stream, client: Client, requests: Arc<[Request]>) {
    let mut idx = 0;

    for request in requests.iter() {
        info!(?request);

        for _ in 0..request.count {
            let span = info_span!("request", request = idx);
            check_write(&mut stream, &client.delays, request.request_size)
                .instrument(span.clone())
                .await
                .unwrap();
            check_read(
                &mut stream,
                &client.delays,
                request.response_size,
                client.max_read_len,
            )
            .instrument(span)
            .await
            .unwrap();

            // increment the request counter
            idx += 1;
        }
    }

    let _ = check_shutdown_write(&mut stream, &client.delays).await;
    let _ = check_shutdown_read(&mut stream, &client.delays).await;

    client.delays.drop().await;
    info!("dropping stream");
}

impl Harness {
    async fn run(self) {
        let client = self.client.build();
        let server = self.server.build(self.protocol);
        self.run_with(client, server).await;
    }

    async fn run_with(self, client: testing::Client, server: testing::Server) {
        let (run_handle, run_watch) = testing::drop_handle::new();
        let task = self.run_with_drop_handle(client, server, run_watch);
        tokio::time::timeout(Duration::from_secs(60), task)
            .await
            .unwrap();
        drop(run_handle);
    }

    async fn run_with_drop_handle(
        self,
        client: testing::Client,
        server: testing::Server,
        run_watch: testing::drop_handle::Receiver,
    ) {
        crate::testing::init_tracing();

        let requests: Arc<[Request]> = self.requests.into();

        for idx in 0..self.server.count {
            tokio::spawn({
                let config = self.server;
                let server = server.clone();
                let requests = requests.clone();
                let task = async move {
                    let mut idx = 0;
                    loop {
                        info!("accepting");
                        let (stream, peer_addr) = server.accept().await.unwrap();
                        info!(%peer_addr, local_addr = %stream.local_addr().unwrap());

                        tokio::spawn(
                            check_server(stream, config, requests.clone())
                                .instrument(info_span!("stream", stream = idx)),
                        );

                        idx += 1;
                    }
                }
                .instrument(info_span!("server", server = idx));

                run_watch.wrap(task)
            });
        }

        let concurrency = tokio::sync::Semaphore::new(self.client.concurrency);
        let concurrency = Arc::new(concurrency);
        let mut application = tokio::task::JoinSet::new();

        for idx in 0..self.client.count {
            let permit = loop {
                tokio::select! {
                    permit = concurrency.clone().acquire_owned() => break permit.unwrap(),
                    Some(res) = application.join_next() => {
                        res.expect("task panic");
                        continue;
                    }
                }
            };

            application.spawn({
                let config = self.client;
                let requests = requests.clone();
                let client = client.clone();
                let server = server.clone();
                let task = async move {
                    info!("connecting");
                    let stream = client.connect_to(&server).await.unwrap();
                    info!(peer_addr = %stream.peer_addr().unwrap(), local_addr = %stream.local_addr().unwrap());

                    check_client(stream, config, requests).await;

                    drop(permit);
                }
                .instrument(info_span!("client", client = idx));

                run_watch.wrap(task)
            });
        }

        while let Some(res) = application.join_next().await {
            res.expect("task panic");
        }

        drop(client);
        drop(server);
    }
}

struct Runtime {
    rt: tokio::runtime::Runtime,
    client: testing::Client,
    server: testing::Server,
    protocol: Protocol,
}

impl Runtime {
    fn new(harness: Harness) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();

        let _guard = rt.enter();

        let protocol = harness.protocol;
        let client = harness.client.build();
        let server = harness.server.build(protocol);

        Self {
            rt,
            client,
            server,
            protocol,
        }
    }

    fn run_with(&self, client: Client, server: Server, requests: Vec<Request>) {
        let harness = Harness {
            client,
            server,
            requests,
            protocol: self.protocol,
        };
        let client = self.client.clone();
        let server = self.server.clone();
        let task = harness.run_with(client, server);
        self.rt.block_on(task);
    }
}

macro_rules! tests {
    () => {
        #[tokio::test]
        async fn no_delay_test() {
            harness().run().await;
        }

        // limit the client's MTU to lower than the server's
        #[tokio::test]
        async fn client_small_mtu() {
            Harness {
                requests: vec![Request {
                    count: 1,
                    request_size: 2usize.pow(18),
                    response_size: 2usize.pow(18),
                }],
                client: Client {
                    max_mtu: Some(1250),
                    ..Default::default()
                },
                ..harness()
            }
            .run()
            .await;
        }

        // limit the server's MTU to lower than the client's
        #[tokio::test]
        async fn server_small_mtu() {
            Harness {
                requests: vec![Request {
                    count: 1,
                    request_size: 2usize.pow(18),
                    response_size: 2usize.pow(18),
                }],
                server: Server {
                    max_mtu: Some(1250),
                    ..Default::default()
                },
                ..harness()
            }
            .run()
            .await;
        }

        // limit the number of bytes that each side reads
        #[tokio::test]
        async fn small_read_test() {
            Harness {
                requests: vec![Request {
                    count: 1,
                    request_size: 16,
                    response_size: 16,
                }],
                client: Client {
                    max_read_len: 4,
                    ..Default::default()
                },
                server: Server {
                    max_read_len: 4,
                    ..Default::default()
                },
                ..harness()
            }
            .run()
            .await;
        }

        #[tokio::test]
        async fn multi_request_test() {
            let harness = harness();

            // TODO make this not flaky with UDP
            if harness.protocol.is_udp() {
                return;
            }

            Harness {
                requests: vec![Request {
                    count: 10,
                    request_size: 1_000,
                    response_size: 64_000,
                }],
                client: Client {
                    count: 1_000,
                    concurrency: 10,
                    ..Default::default()
                },
                ..harness
            }
            .run()
            .await;
        }

        #[tokio::test]
        async fn client_delay_read_test() {
            Harness {
                client: Client {
                    delays: Delays {
                        read: Duration::from_millis(100),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..harness()
            }
            .run()
            .await;
        }

        #[tokio::test]
        async fn client_delayed_multi_request_test() {
            Harness {
                requests: vec![Request {
                    count: 2,
                    ..Default::default()
                }],
                client: Client {
                    delays: Delays {
                        read: Duration::from_secs(5),
                        write: Duration::from_secs(5),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..harness()
            }
            .run()
            .await;
        }

        #[tokio::test]
        async fn server_delayed_multi_request_test() {
            Harness {
                requests: vec![Request {
                    count: 2,
                    ..Default::default()
                }],
                server: Server {
                    delays: Delays {
                        read: Duration::from_secs(5),
                        write: Duration::from_secs(5),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                ..harness()
            }
            .run()
            .await;
        }

        #[tokio::test]
        async fn server_bulk_transfer_test() {
            Harness {
                requests: vec![Request {
                    count: 1,
                    request_size: 1000,
                    response_size: 424_242_424,
                }],
                ..harness()
            }
            .run()
            .await;
        }

        #[tokio::test]
        async fn client_bulk_transfer_test() {
            Harness {
                requests: vec![Request {
                    count: 1,
                    request_size: 424_242_424,
                    response_size: 1000,
                }],
                ..harness()
            }
            .run()
            .await;
        }

        #[test]
        fn fuzz_test() {
            use std::sync::OnceLock;

            bolero::check!()
                .with_generator((produce(), produce(), produce::<Vec<_>>().with().len(1..5)))
                .cloned()
                .with_test_time(Duration::from_secs(45))
                .for_each(|(client, server, requests)| {
                    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
                    RUNTIME
                        .get_or_init(|| Runtime::new(harness()))
                        .run_with(client, server, requests);
                });
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

#[cfg(target_os = "linux")] // TODO linux is only working right now
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
