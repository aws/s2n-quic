// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{server::tokio::stats, socket::Protocol};
use crate::{
    event,
    event::testing,
    path::secret,
    stream::{
        application,
        client::tokio as stream_client,
        environment::{tokio as env, Environment as _},
        recv, send,
        server::{accept, tokio as stream_server},
    },
};
use s2n_quic_core::dc::{self, ApplicationParams};
use s2n_quic_platform::socket;
use std::{io, net::SocketAddr, sync::Arc};
use tracing::Instrument;

pub type Subscriber = (Arc<event::testing::Subscriber>, event::tracing::Subscriber);

pub type Stream = application::Stream<Subscriber>;
pub type Writer = send::application::Writer<Subscriber>;
pub type Reader = recv::application::Reader<Subscriber>;

const DEFAULT_POOLED: bool = true;

// limit the number of threads used in testing to reduce costs of harnesses
const TEST_THREADS: usize = 2;

pub(crate) const MAX_DATAGRAM_SIZE: u16 = if cfg!(target_os = "linux") {
    8950
} else {
    1450
};

#[derive(Clone)]
pub struct Client {
    map: secret::Map,
    env: env::Environment<Subscriber>,
    mtu: Option<u16>,
}

impl Default for Client {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl Client {
    pub fn builder() -> client::Builder {
        client::Builder::default()
    }

    pub fn handshake_with<S: AsRef<server::Handle>>(
        &self,
        server: &S,
    ) -> io::Result<secret::map::Peer> {
        let server = server.as_ref();
        let peer = server.local_addr;
        if let Some(peer) = self.map.get_tracked(peer) {
            return Ok(peer);
        }

        let local_addr = "127.0.0.1:1337".parse().unwrap();
        self.map.test_insert_pair(
            local_addr,
            Some(self.params()),
            &server.map,
            server.local_addr,
            Some(server.params()),
        );

        // cache hit already tracked above
        self.map.get_untracked(peer).ok_or_else(|| {
            io::Error::new(io::ErrorKind::AddrNotAvailable, "path secret not available")
        })
    }

    fn params(&self) -> ApplicationParams {
        let mut params = dc::testing::TEST_APPLICATION_PARAMS;
        params.max_datagram_size = self.mtu.unwrap_or(MAX_DATAGRAM_SIZE).into();
        params
    }

    pub async fn connect_to<S: AsRef<server::Handle>>(&self, server: &S) -> io::Result<Stream> {
        let server = server.as_ref();
        let handshake = async { self.handshake_with(server) };

        match server.protocol {
            Protocol::Tcp => {
                stream_client::connect_tcp(handshake, server.local_addr, &self.env, None).await
            }
            Protocol::Udp => {
                stream_client::connect_udp(handshake, server.local_addr, &self.env).await
            }
            Protocol::Other(name) => {
                todo!("protocol {name:?} not implemented")
            }
        }
    }

    pub async fn connect_tcp_with<S: AsRef<server::Handle>>(
        &self,
        server: &S,
        stream: tokio::net::TcpStream,
    ) -> io::Result<Stream> {
        let server = server.as_ref();
        let handshake = async { self.handshake_with(server) }.await?;

        stream_client::connect_tcp_with(handshake, stream, &self.env).await
    }

    pub fn subscriber(&self) -> Arc<testing::Subscriber> {
        self.env.subscriber().0.clone()
    }
}

pub mod client {
    use super::*;

    pub struct Builder {
        map_capacity: usize,
        mtu: Option<u16>,
        subscriber: event::testing::Subscriber,
        pooled: bool,
    }

    impl Default for Builder {
        fn default() -> Self {
            Self {
                map_capacity: 16,
                mtu: None,
                subscriber: event::testing::Subscriber::no_snapshot(),
                pooled: DEFAULT_POOLED,
            }
        }
    }

    impl Builder {
        pub fn map_capacity(mut self, map_capacity: usize) -> Self {
            self.map_capacity = map_capacity;
            self
        }

        pub fn mtu(mut self, mtu: u16) -> Self {
            self.mtu = Some(mtu);
            self
        }

        pub fn subscriber(mut self, subscriber: event::testing::Subscriber) -> Self {
            self.subscriber = subscriber;
            self
        }

        pub fn build(self) -> Client {
            let Self {
                map_capacity,
                mtu,
                subscriber,
                pooled,
            } = self;
            let _span = tracing::info_span!("client").entered();
            let map = secret::map::testing::new(map_capacity);
            let options = socket::options::Options::new("127.0.0.1:0".parse().unwrap());
            let subscriber = Arc::new(subscriber);
            let subscriber = (subscriber, event::tracing::Subscriber::default());
            let mut env = env::Builder::new(subscriber)
                .with_threads(TEST_THREADS)
                .with_socket_options(options);

            if pooled {
                let pool = env::pool::Config::new(map.clone());
                env = env.with_pool(pool);
            }

            let env = env.build().unwrap();
            Client { map, env, mtu }
        }
    }
}

#[derive(Clone)]
pub struct Server {
    handle: server::Handle,
    receiver: accept::Receiver<Subscriber>,
    stats: stats::Sender,
    #[allow(dead_code)]
    drop_handle: drop_handle::Sender,
    subscriber: Arc<event::testing::Subscriber>,
}

impl Default for Server {
    fn default() -> Self {
        Self::tcp().build()
    }
}

impl AsRef<server::Handle> for Server {
    fn as_ref(&self) -> &server::Handle {
        &self.handle
    }
}

impl Server {
    pub fn builder() -> server::Builder {
        server::Builder::default()
    }

    pub fn tcp() -> server::Builder {
        Self::builder().tcp()
    }

    pub fn udp() -> server::Builder {
        Self::builder().udp()
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.as_ref().local_addr
    }

    pub fn handle(&self) -> server::Handle {
        self.handle.clone()
    }

    pub async fn accept(&self) -> io::Result<(Stream, SocketAddr)> {
        stream_server::accept::accept(&self.receiver, &self.stats).await
    }

    pub fn subscriber(&self) -> Arc<testing::Subscriber> {
        self.subscriber.clone()
    }
}

pub(crate) mod drop_handle {
    use core::future::Future;
    use tokio::sync::watch;

    pub fn new() -> (Sender, Receiver) {
        let (sender, receiver) = watch::channel(());
        (Sender(sender), Receiver(receiver))
    }

    #[derive(Clone)]
    pub struct Receiver(watch::Receiver<()>);

    impl Receiver {
        pub fn wrap<F>(&self, other: F) -> impl Future<Output = ()>
        where
            F: Future<Output = ()>,
        {
            let mut watch = self.0.clone();
            async move {
                tokio::select! {
                    _ = other => {}
                    _ = watch.changed() => {}
                }
            }
        }
    }

    #[derive(Clone)]
    pub struct Sender(#[allow(dead_code)] watch::Sender<()>);
}

pub mod server {
    use std::time::Duration;

    use super::*;

    #[derive(Clone)]
    pub struct Handle {
        pub(super) map: secret::Map,
        pub(super) protocol: Protocol,
        pub(super) local_addr: SocketAddr,
        pub(super) mtu: Option<u16>,
    }

    impl Handle {
        pub(super) fn params(&self) -> ApplicationParams {
            let mut params = dc::testing::TEST_APPLICATION_PARAMS;
            params.max_datagram_size = self.mtu.unwrap_or(MAX_DATAGRAM_SIZE).into();
            params
        }
    }

    impl AsRef<Handle> for Handle {
        fn as_ref(&self) -> &Handle {
            self
        }
    }

    pub struct Builder {
        backlog: usize,
        flavor: accept::Flavor,
        protocol: Protocol,
        map_capacity: usize,
        linger: Option<Duration>,
        mtu: Option<u16>,
        subscriber: event::testing::Subscriber,
        pooled: bool,
    }

    impl Default for Builder {
        fn default() -> Self {
            Self {
                backlog: 16,
                flavor: accept::Flavor::default(),
                protocol: Protocol::Tcp,
                map_capacity: 16,
                linger: None,
                mtu: None,
                subscriber: event::testing::Subscriber::no_snapshot(),
                pooled: DEFAULT_POOLED,
            }
        }
    }

    impl Builder {
        pub fn build(self) -> Server {
            if s2n_quic_platform::io::testing::is_in_env() {
                todo!()
            } else {
                self.build_tokio()
            }
        }

        pub fn tcp(mut self) -> Self {
            self.protocol = Protocol::Tcp;
            self
        }

        pub fn udp(mut self) -> Self {
            self.protocol = Protocol::Udp;
            self
        }

        pub fn protocol(mut self, protocol: Protocol) -> Self {
            self.protocol = protocol;
            self
        }

        pub fn backlog(mut self, backlog: usize) -> Self {
            self.backlog = backlog;
            self
        }

        pub fn map_capacity(mut self, map_capacity: usize) -> Self {
            self.map_capacity = map_capacity;
            self
        }

        pub fn accept_flavor(mut self, flavor: accept::Flavor) -> Self {
            self.flavor = flavor;
            self
        }

        pub fn linger(mut self, linger: Duration) -> Self {
            self.linger = Some(linger);
            self
        }

        pub fn mtu(mut self, mtu: u16) -> Self {
            self.mtu = Some(mtu);
            self
        }

        pub fn subscriber(mut self, subscriber: event::testing::Subscriber) -> Self {
            self.subscriber = subscriber;
            self
        }

        fn build_tokio(self) -> super::Server {
            let Self {
                backlog,
                flavor,
                protocol,
                map_capacity,
                linger,
                mtu,
                subscriber,
                pooled,
            } = self;

            let _span = tracing::info_span!("server").entered();
            let map = secret::map::testing::new(map_capacity);
            let (sender, receiver) = accept::channel(backlog);

            let options = crate::socket::Options::new("127.0.0.1:0".parse().unwrap());

            let test_subscriber = Arc::new(subscriber);
            let subscriber = (
                test_subscriber.clone(),
                event::tracing::Subscriber::default(),
            );

            let mut env = env::Builder::new(subscriber.clone())
                .with_threads(TEST_THREADS)
                .with_socket_options(options.clone());

            if pooled {
                let mut pool = env::pool::Config::new(map.clone());
                pool.accept_flavor = flavor;
                pool.reuse_port = true;
                env = env.with_pool(pool).with_acceptor(sender.clone());
            }

            let env = env.build().unwrap();

            let (drop_handle_sender, drop_handle_receiver) = drop_handle::new();

            let local_addr = match protocol {
                Protocol::Tcp => {
                    let socket = options.build_tcp_listener().unwrap();
                    let local_addr = socket.local_addr().unwrap();
                    let socket = tokio::net::TcpListener::from_std(socket).unwrap();

                    let acceptor = stream_server::tcp::Acceptor::new(
                        0, socket, &sender, &env, &map, backlog, flavor, linger,
                    );
                    let acceptor = drop_handle_receiver.wrap(acceptor.run());
                    let acceptor = acceptor.instrument(tracing::info_span!("tcp"));
                    tokio::task::spawn(acceptor);

                    local_addr
                }
                Protocol::Udp if pooled => {
                    // acceptor configured in env
                    env.pool_addr().unwrap()
                }
                Protocol::Udp => {
                    let socket = options.build_udp().unwrap();
                    let local_addr = socket.local_addr().unwrap();

                    let socket = tokio::io::unix::AsyncFd::new(socket).unwrap();

                    let acceptor =
                        stream_server::udp::Acceptor::new(0, socket, &sender, &env, &map, flavor);
                    let acceptor = drop_handle_receiver.wrap(acceptor.run());
                    let acceptor = acceptor.instrument(tracing::info_span!("udp"));
                    tokio::task::spawn(acceptor);

                    local_addr
                }
                Protocol::Other(name) => {
                    todo!("protocol {name:?} not implemented")
                }
            };

            let (stats_sender, stats_worker, stats) = stats::channel();

            {
                let task = stats_worker.run(env.clock().clone());
                let task = task.instrument(tracing::info_span!("stats"));
                let task = drop_handle_receiver.wrap(task);
                tokio::task::spawn(task);
            }

            if matches!(flavor, accept::Flavor::Lifo) {
                let channel = receiver.downgrade();
                let task = stream_server::accept::Pruner::default().run(env, channel, stats);
                let task = task.instrument(tracing::info_span!("pruner"));
                let task = drop_handle_receiver.wrap(task);
                tokio::task::spawn(task);
            }

            let handle = server::Handle {
                map,
                protocol,
                local_addr,
                mtu,
            };

            super::Server {
                handle,
                receiver,
                stats: stats_sender,
                drop_handle: drop_handle_sender,
                subscriber: test_subscriber,
            }
        }
    }
}
