// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    stream::{
        socket::Protocol,
        testing::{self, Stream, MAX_DATAGRAM_SIZE},
    },
    testing::{ext::*, sim, sleep, spawn, task::spawn_named, timeout, without_tracing},
};
use bolero::{produce, TypeGenerator, ValueGenerator as _};
use core::time::Duration;
use s2n_quic_core::{
    buffer::{reader::Storage as _, writer::Storage as _},
    stream::testing::Data,
};
use std::{io, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
    async fn build(&self) -> testing::Client {
        let task = async {
            let mut builder = testing::Client::builder();
            if let Some(max_mtu) = self.max_mtu {
                builder = builder.mtu(max_mtu);
            }
            builder.build()
        };

        if ::bach::is_active() {
            task.group("client").await
        } else {
            task.await
        }
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
    async fn build(&self, protocol: Protocol) -> testing::Server {
        let task = async {
            let mut builder = testing::Server::builder().protocol(protocol);
            if let Some(max_mtu) = self.max_mtu {
                builder = builder.mtu(max_mtu);
            }
            builder.build()
        };

        if ::bach::is_active() {
            task.group("server").await
        } else {
            task.await
        }
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
        let offset = data.offset();
        let capacity = data.buffered_len();
        let mut data = data.with_write_limit(capacity.min(max_read_len));
        tracing::debug!(offset, capacity = data.remaining_capacity(), "read_into");
        let len = stream.read_into(&mut data).await?;
        tracing::debug!(len, "read_into_result");
        if len == 0 {
            break;
        }
    }
    info!(len = data.offset(), "read_into_finished");
    assert!(data.is_finished());
    Ok(())
}

async fn check_write(stream: &mut Stream, delays: &Delays, amount: usize) -> io::Result<()> {
    delays.write().await;
    info!(writing = amount);
    let mut data = Data::new(amount as _);
    while !data.is_finished() {
        tracing::debug!(
            offset = data.offset(),
            remaining = data.buffered_len(),
            "write_from"
        );
        let len = stream.write_from(&mut data).await?;
        tracing::debug!(len, "write_from_result");
    }
    info!(amount, "write_from_finished");
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
        let client = self.client.build().await;
        let server = self.server.build(self.protocol).await;
        self.run_with(client, server).await;
    }

    async fn run_with(self, client: testing::Client, server: testing::Server) {
        let (run_handle, run_watch) = testing::drop_handle::new();
        let task = self.run_with_drop_handle(client, server, run_watch);
        let duration = Duration::from_secs(180);
        timeout(duration, task).await.unwrap();
        drop(run_handle);
    }

    async fn run_with_drop_handle(
        self,
        client: testing::Client,
        server: testing::Server,
        run_watch: testing::drop_handle::Receiver,
    ) {
        crate::testing::init_tracing();
        let is_sim = ::bach::is_active();

        info!(is_sim, test = ?self, "start");

        let requests: Arc<[Request]> = self.requests.into();

        for idx in 0..self.server.count {
            let config = self.server;
            let server = server.clone();
            let requests = requests.clone();
            let task = async move {
                let mut idx = 0;
                loop {
                    info!("accepting");
                    let (stream, peer_addr) = server.accept().await.unwrap();
                    info!(%peer_addr, local_addr = %stream.local_addr().unwrap());

                    spawn(
                        check_server(stream, config, requests.clone())
                            .instrument(info_span!("stream", stream = idx)),
                    );

                    idx += 1;
                }
            }
            .instrument(info_span!("server", server = idx));

            if is_sim {
                task.group(format!("server_{idx}")).spawn();
            } else {
                spawn(run_watch.wrap(task));
            }
        }

        if is_sim {
            // TODO limit concurrency - for now we just stagger clients

            for idx in 0..self.client.count {
                let config = self.client;
                let requests = requests.clone();
                let client = client.clone();
                let server = server.clone();
                let task = async move {
                    // delay connecting by 1us per client
                    (1.us() * idx as u32).sleep().await;

                    info!("connecting");

                    let stream = client.connect_to(&server).await.unwrap();
                    info!(peer_addr = %stream.peer_addr().unwrap(), local_addr = %stream.local_addr().unwrap());

                    check_client(stream, config, requests).await;
                }
                .group(format!("client_{idx}"))
                .instrument(info_span!("client", client = idx));

                task.primary().spawn();
            }
        } else {
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
        }

        drop(client);
        drop(server);
    }
}

// Filter requests that are too large compared to client's or server's max_read_len
// Timeout will happen if the request size is 100 times larger than client's or server's max_read_len
fn filter_large_requests(
    client: &Client,
    server: &Server,
    mut requests: Vec<Request>,
) -> Vec<Request> {
    let min_max_read_len = client.max_read_len.min(server.max_read_len);
    requests.retain(|request| request.request_size <= min_max_read_len * 100);

    requests
}

struct Runtime {
    rt: tokio::runtime::Runtime,
    client: testing::Client,
    server: testing::Server,
    protocol: Protocol,
}

impl Runtime {
    fn new(harness: &Harness) -> Self {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let _guard = rt.enter();

        let protocol = harness.protocol;
        let (client, server) = rt.block_on(async {
            let client = harness.client.build().await;
            let server = harness.server.build(protocol).await;
            (client, server)
        });

        Self {
            rt,
            client,
            server,
            protocol,
        }
    }

    fn run(&self, harness: Harness) {
        self.rt.block_on(harness.run());
    }

    fn run_with(&self, client: Client, server: Server, requests: Vec<Request>) {
        // Filter out requests that are too large compared to client or server max_read_len
        let filtered_requests = filter_large_requests(&client, &server, requests);

        let harness = Harness {
            client,
            server,
            requests: filtered_requests,
            protocol: self.protocol,
        };
        let client = self.client.clone();
        let server = self.server.clone();
        let task = harness.run_with(client, server);
        self.rt.block_on(task);
    }
}

macro_rules! tests {
    ($test:ident) => {
        $test!(no_delay_test, harness());

        // limit the client's MTU to lower than the server's
        $test!(
            client_small_mtu,
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
        );

        // limit the server's MTU to lower than the client's
        $test!(
            server_small_mtu,
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
        );

        // limit the number of bytes that each side reads
        $test!(
            small_read_test,
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
        );

        $test!(multi_request_test, {
            let harness = harness();

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
        });

        $test!(
            client_delay_read_test,
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
        );

        $test!(
            client_delayed_multi_request_test,
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
        );

        $test!(
            server_delayed_multi_request_test,
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
        );

        $test!(
            server_bulk_transfer_test,
            Harness {
                requests: vec![Request {
                    count: 1,
                    request_size: 1000,
                    response_size: 424_242_424,
                }],
                ..harness()
            }
        );

        $test!(
            client_bulk_transfer_test,
            Harness {
                requests: vec![Request {
                    count: 1,
                    request_size: 424_242,
                    response_size: 1000,
                }],
                ..harness()
            }
        );
    };
}

macro_rules! tokio_test {
    ($name:ident, $harness:expr) => {
        #[test]
        fn $name() {
            let harness = $harness;
            without_tracing(|| {
                Runtime::new(&harness).run(harness);
            });
        }
    };
}

macro_rules! tokio_fuzz_test {
    () => {
        #[test]
        fn fuzz_test() {
            without_tracing(|| {
                use std::sync::OnceLock;

                bolero::check!()
                    .with_generator((produce(), produce(), produce::<Vec<_>>().with().len(1..5)))
                    .cloned()
                    // limit the amount of time in tests since they can produce a lot of tracing data
                    .with_test_time(Duration::from_secs(10))
                    .for_each(|(client, server, requests)| {
                        static RUNTIME: OnceLock<Runtime> = OnceLock::new();
                        RUNTIME
                            .get_or_init(|| Runtime::new(&harness()))
                            .run_with(client, server, requests);
                    });
            });
        }
    };
}

macro_rules! sim_test {
    ($name:ident, $harness:expr) => {
        #[test]
        fn $name() {
            // The tracing logs end up consuming a bunch of memory and failing the tests
            without_tracing(|| {
                sim(|| {
                    spawn_named($harness.run().primary(), "harness");

                    async {
                        sleep(Duration::from_secs(120)).await;
                        panic!("test timed out after {}", bach::time::Instant::now());
                    }
                    .spawn();
                });
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

    tests!(tokio_test);
    tokio_fuzz_test!();
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

    tests!(tokio_test);
    tokio_fuzz_test!();
}

mod udp_sim {
    use super::*;

    fn harness() -> Harness {
        Harness {
            protocol: Protocol::Udp,
            ..Default::default()
        }
    }

    tests!(sim_test);

    #[test]
    fn fuzz_test() {
        // The tracing logs end up consuming a bunch of memory and failing the tests
        without_tracing(|| {
            bolero::check!()
                .with_generator((produce(), produce(), produce::<Vec<_>>().with().len(1..5)))
                .cloned()
                .with_test_time(Duration::from_secs(60))
                .for_each(|(client, server, requests)| {
                    crate::testing::sim(|| {
                        Harness {
                            client,
                            server,
                            requests,
                            ..harness()
                        }
                        .run()
                        .primary()
                        .spawn();
                    });
                })
        });
    }
}
