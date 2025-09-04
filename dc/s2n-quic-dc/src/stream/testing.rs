// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{client::tokio::Client as ClientTokio, server::tokio::Server as ServerTokio};
use crate::{
    either::Either,
    event::{self, testing},
    path::secret,
    psk::{client::Provider as ClientProvider, server::Provider as ServerProvider},
    stream::{
        application, client as stream_client,
        environment::{bach, tokio, udp, Environment},
        server::{self as stream_server, accept, stats},
        socket::Protocol,
    },
    testing::NoopSubscriber,
};
use s2n_quic_core::dc::{self, ApplicationParams};
use s2n_quic_platform::socket;
use std::{
    cell::RefCell,
    collections::HashMap,
    io,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tracing::Instrument;

thread_local! {
    static SERVERS: RefCell<HashMap<SocketAddr, server::Handle>> = Default::default();
}

pub type Subscriber = (Arc<event::testing::Subscriber>, event::tracing::Subscriber);

pub type Stream = application::Stream<Subscriber>;

const DEFAULT_POOLED: bool = true;

// limit the number of threads used in testing to reduce costs of harnesses
const TEST_THREADS: usize = 2;

pub(crate) const MAX_DATAGRAM_SIZE: u16 = if cfg!(target_os = "linux") {
    8950
} else {
    1450
};

type Env = Either<tokio::Environment<Subscriber>, bach::Environment<Subscriber>>;

pub fn bind_pair(
    protocol: Protocol,
    server_addr: SocketAddr,
    client: ClientProvider,
    server: ServerProvider,
) -> (
    ClientTokio<ClientProvider, NoopSubscriber>,
    ServerTokio<ServerProvider, NoopSubscriber>,
) {
    let test_subscriber = NoopSubscriber {};
    let client = ClientTokio::<ClientProvider, NoopSubscriber>::builder()
        .with_default_protocol(protocol)
        .build(client, test_subscriber.clone())
        .unwrap();

    let server = ServerTokio::<ServerProvider, NoopSubscriber>::builder()
        .with_address(server_addr)
        .with_protocol(protocol)
        .with_workers(1.try_into().unwrap())
        .build(server, test_subscriber)
        .unwrap();

    (client, server)
}

macro_rules! check_pair_addrs {
    ($local:ident, $peer:ident) => {
        debug_assert_eq!(
            $local.local_addr().ok().map(|addr| addr.port()),
            $peer.peer_addr().ok().map(|addr| addr.port())
        );
    };
}

macro_rules! dcquic_context {
    ($protocol:ident) => {
        use super::Protocol;
        use std::net::SocketAddr;

        pub type Stream = crate::stream::application::Stream<super::NoopSubscriber>;

        pub struct Context(super::Context);

        #[allow(dead_code)]
        impl Context {
            pub async fn new() -> Self {
                Self(super::Context::new(Protocol::$protocol).await)
            }

            pub fn new_sync(addr: SocketAddr) -> Self {
                Self(super::Context::new_sync(Protocol::$protocol, addr))
            }

            pub async fn bind(addr: SocketAddr) -> Self {
                Self(super::Context::bind(Protocol::$protocol, addr).await)
            }

            pub fn acceptor_addr(&self) -> SocketAddr {
                self.0.acceptor_addr()
            }

            pub fn handshake_addr(&self) -> SocketAddr {
                self.0.handshake_addr()
            }

            pub async fn pair(&self) -> (Stream, Stream) {
                self.0.pair().await
            }

            pub async fn pair_with(&self, acceptor_addr: SocketAddr) -> (Stream, Stream) {
                self.0.pair_with(acceptor_addr).await
            }

            pub fn protocol(&self) -> Protocol {
                self.0.protocol
            }
        }
    };
}

pub mod dcquic {
    use crate::{
        psk::{client::Provider as ClientProvider, server::Provider as ServerProvider},
        stream::{
            client::tokio::Client as ClientTokio,
            server::tokio::Server as ServerTokio,
            socket::Protocol,
            testing::{bind_pair, NoopSubscriber},
        },
        testing::server_name,
    };
    use std::net::SocketAddr;

    pub type Stream = crate::stream::application::Stream<NoopSubscriber>;

    pub mod tcp {
        dcquic_context!(Tcp);
    }

    pub mod udp {
        dcquic_context!(Udp);
    }

    pub struct Context {
        pub(crate) server: ServerTokio<ServerProvider, NoopSubscriber>,
        pub(crate) client: ClientTokio<ClientProvider, NoopSubscriber>,
        protocol: Protocol,
    }

    impl Context {
        pub async fn new(protocol: Protocol) -> Self {
            if protocol.is_udp() {
                Self::bind(protocol, "[::1]:0".parse().unwrap()).await
            } else {
                Self::bind(protocol, "127.0.0.1:0".parse().unwrap()).await
            }
        }

        pub fn new_sync(protocol: Protocol, addr: SocketAddr) -> Self {
            let (client, server) = crate::testing::pair_sync();
            let (client, server) = bind_pair(protocol, addr, client, server);
            Self {
                client,
                server,
                protocol,
            }
        }

        pub async fn bind(protocol: Protocol, addr: SocketAddr) -> Self {
            Self::bind_with(protocol, addr, crate::testing::Pair::default()).await
        }

        pub async fn bind_with(
            protocol: Protocol,
            addr: SocketAddr,
            pair: crate::testing::Pair,
        ) -> Self {
            let (client, server) = pair.build().await;
            let (client, server) = bind_pair(protocol, addr, client, server);
            Self {
                client,
                server,
                protocol,
            }
        }

        pub fn acceptor_addr(&self) -> SocketAddr {
            self.server.acceptor_addr().expect("acceptor_addr")
        }

        pub fn handshake_addr(&self) -> SocketAddr {
            self.server.handshake_addr().expect("handshake_addr")
        }

        pub async fn pair(&self) -> (Stream, Stream) {
            self.pair_with(self.acceptor_addr()).await
        }

        pub async fn pair_with(&self, acceptor_addr: SocketAddr) -> (Stream, Stream) {
            let handshake_addr = self.handshake_addr();
            let (client, server) = tokio::join!(
                async move {
                    self.client
                        .connect(handshake_addr, acceptor_addr, server_name())
                        .await
                        .expect("connect")
                },
                async move {
                    let (conn, _) = self.server.accept().await.expect("accept");
                    conn
                }
            );

            check_pair_addrs!(client, server);

            // the client doesn't have a response from the server so it might not know its
            // port yet
            if self.protocol().is_udp() {
                let acceptor_port = acceptor_addr.port();
                let server_local_port = server.local_addr().unwrap().port();
                let client_peer_port = client.peer_addr().unwrap().port();
                assert!(
                    [acceptor_port, server_local_port].contains(&client_peer_port),
                    "acceptor_port={acceptor_port}, server_local_port={server_local_port}, client_peer_port={client_peer_port}"
                );
            } else {
                check_pair_addrs!(server, client);
            }

            (client, server)
        }

        pub fn protocol(&self) -> Protocol {
            self.protocol
        }

        #[allow(dead_code)]
        pub fn split(
            self,
        ) -> (
            ClientTokio<ClientProvider, NoopSubscriber>,
            ServerTokio<ServerProvider, NoopSubscriber>,
        ) {
            (self.client, self.server)
        }
    }
}

pub mod tcp {
    use super::Protocol;
    use std::net::SocketAddr;
    use tokio::net::{TcpListener, TcpStream};

    pub type Stream = TcpStream;

    pub struct Context {
        acceptor: TcpListener,
    }

    impl Context {
        pub async fn new() -> Self {
            Self::bind("127.0.0.1:0".parse().unwrap()).await
        }

        pub async fn bind(addr: SocketAddr) -> Self {
            let acceptor = TcpListener::bind(addr).await.expect("bind");
            Self { acceptor }
        }

        pub fn acceptor_addr(&self) -> SocketAddr {
            self.acceptor.local_addr().expect("acceptor_addr")
        }

        pub async fn pair(&self) -> (Stream, Stream) {
            self.pair_with(self.acceptor_addr()).await
        }

        pub async fn pair_with(&self, acceptor_addr: SocketAddr) -> (Stream, Stream) {
            let (client, server) = tokio::join!(
                async move { Stream::connect(acceptor_addr).await.expect("connect") },
                async move {
                    let (conn, _addr) = self.acceptor.accept().await.expect("accept");
                    conn
                }
            );

            check_pair_addrs!(client, server);
            check_pair_addrs!(server, client);

            (client, server)
        }

        pub fn protocol(&self) -> Protocol {
            Protocol::Tcp
        }
    }
}

#[derive(Clone)]
pub struct Client {
    map: secret::Map,
    env: Env,
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
        let server_addr = server.local_addr;
        if let Some(peer) = self.map.get_tracked(server_addr) {
            return Ok(peer);
        }

        let local_addr = "127.0.0.1:1337".parse().unwrap();
        self.map.test_insert_pair(
            local_addr,
            Some(self.params()),
            &server.map,
            server_addr,
            Some(server.params()),
        );

        // cache hit already tracked above
        self.map.get_untracked(server_addr).ok_or_else(|| {
            io::Error::new(io::ErrorKind::AddrNotAvailable, "path secret not available")
        })
    }

    fn params(&self) -> ApplicationParams {
        let mut params = dc::testing::TEST_APPLICATION_PARAMS;
        params.max_datagram_size = self.mtu.unwrap_or(MAX_DATAGRAM_SIZE).into();
        params
    }

    pub async fn connect_to<S: AsRef<server::Handle>>(&self, server: &S) -> io::Result<Stream> {
        // write an empty prelude
        let mut prelude = s2n_quic_core::buffer::reader::storage::Empty;
        let mut stream = self.open(server.as_ref()).await?;
        stream.write_from(&mut prelude).await?;
        Ok(stream)
    }

    pub async fn connect_sim<Addr>(&self, addr: Addr) -> io::Result<Stream>
    where
        Addr: ::bach::net::ToSocketAddrs,
    {
        let server = Self::lookup_sim_server(addr).await?;
        self.connect_to(&server).await
    }

    pub async fn rpc_to<S, Req, Res>(
        &self,
        server: &S,
        request: Req,
        response: Res,
    ) -> io::Result<Res::Output>
    where
        S: AsRef<server::Handle>,
        Req: crate::stream::client::rpc::Request,
        Res: crate::stream::client::rpc::Response,
    {
        let stream = self.open(server.as_ref()).await?;
        crate::stream::client::rpc::from_stream(stream, request, response).await
    }

    pub async fn rpc_sim<Addr, Req, Res>(
        &self,
        addr: Addr,
        request: Req,
        response: Res,
    ) -> io::Result<Res::Output>
    where
        Addr: ::bach::net::ToSocketAddrs,
        Req: crate::stream::client::rpc::Request,
        Res: crate::stream::client::rpc::Response,
    {
        let server = Self::lookup_sim_server(addr).await?;
        self.rpc_to(&server, request, response).await
    }

    async fn lookup_sim_server<Addr>(addr: Addr) -> io::Result<server::Handle>
    where
        Addr: ::bach::net::ToSocketAddrs,
    {
        assert!(::bach::is_active());

        // yield before we look up the server's addr - it might not have run yet
        ::bach::task::yield_now().await;

        let addr = ::bach::net::lookup_host(addr).await?.next().unwrap();

        let server = SERVERS.with(|servers| servers.borrow().get(&addr).cloned());

        let server = server
            .ok_or_else(|| io::Error::new(io::ErrorKind::AddrNotAvailable, "server not found"))?;

        Ok(server)
    }

    async fn open(&self, server: &server::Handle) -> io::Result<Stream> {
        let server_addr = server.local_addr;
        let handshake = core::future::ready(self.handshake_with(server));

        match (server.protocol, &self.env) {
            (Protocol::Tcp, Either::A(env)) => {
                stream_client::tokio::connect_tcp(handshake, server_addr, env, None).await
            }
            (Protocol::Tcp, Either::B(_env)) => {
                todo!("tcp is not implemented in bach yet");
            }
            (Protocol::Udp, Either::A(env)) => {
                stream_client::tokio::connect_udp(handshake, server_addr, env).await
            }
            (Protocol::Udp, Either::B(env)) => {
                stream_client::bach::connect_udp(handshake, server_addr, env).await
            }
            (Protocol::Other(name), _) => {
                todo!("protocol {name:?} not implemented")
            }
        }
    }

    pub async fn connect_tcp_with<S: AsRef<server::Handle>>(
        &self,
        server: &S,
        stream: ::tokio::net::TcpStream,
    ) -> io::Result<Stream> {
        let server = server.as_ref();
        let handshake = self.handshake_with(server)?;

        let mut stream = if let Either::A(env) = &self.env {
            stream_client::tokio::connect_tcp_with(handshake, stream, env).await
        } else {
            todo!("Raw connect is only supported with tokio");
        }?;

        // TODO accept these as parameters instead
        let mut prelude = s2n_quic_core::buffer::reader::storage::Empty;

        stream.write_from(&mut prelude).await?;

        Ok(stream)
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
            let subscriber = Arc::new(subscriber);
            let subscriber = (subscriber, event::tracing::Subscriber::default());

            macro_rules! build {
                ($krate:ident, $pooled:expr, $addr:expr) => {{
                    let options = socket::options::Options::new($addr.parse().unwrap());

                    let mut env = $krate::Builder::new(subscriber)
                        .with_threads(TEST_THREADS)
                        .with_socket_options(options);

                    if $pooled {
                        let pool = udp::Config::new(map.clone());
                        env = env.with_pool(pool);
                    }

                    env.build().unwrap()
                }};
            }

            let env = if ::bach::is_active() {
                Either::B(build!(bach, true, "0.0.0.0:0"))
            } else {
                Either::A(build!(tokio, pooled, "127.0.0.1:0"))
            };

            Client { map, env, mtu }
        }
    }
}

#[derive(Clone)]
pub struct Server {
    handle: server::Handle,
    receiver: accept::Receiver<Subscriber>,
    stats: stats::Sender,
    subscriber: Arc<event::testing::Subscriber>,
    #[allow(dead_code)]
    drop_handle: drop_handle::Sender,
    #[allow(dead_code)]
    addr_reservation: Arc<server::AddrReservation>,
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

    pub fn map(&self) -> &crate::path::secret::Map {
        &self.handle.map
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
                    _ = watch.changed() => {
                        tracing::trace!("handle dropped - cancelling task");
                    }
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

    pub(super) struct AddrReservation {
        local_addr: SocketAddr,
    }

    impl core::ops::Deref for AddrReservation {
        type Target = SocketAddr;

        fn deref(&self) -> &Self::Target {
            &self.local_addr
        }
    }

    impl Drop for AddrReservation {
        fn drop(&mut self) {
            SERVERS.with(|servers| servers.borrow_mut().remove(&self.local_addr));
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
        port: u16,
    }

    impl Default for Builder {
        fn default() -> Self {
            Self {
                backlog: 4096,
                flavor: accept::Flavor::default(),
                protocol: Protocol::Tcp,
                map_capacity: 16,
                linger: None,
                mtu: None,
                subscriber: event::testing::Subscriber::no_snapshot(),
                pooled: DEFAULT_POOLED,
                port: 0,
            }
        }
    }

    impl Builder {
        pub fn tcp(mut self) -> Self {
            self.protocol = Protocol::Tcp;
            self
        }

        pub fn udp(mut self) -> Self {
            self.protocol = Protocol::Udp;
            self
        }

        pub fn port(mut self, port: u16) -> Self {
            self.port = port;
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

        pub fn build(self) -> super::Server {
            let Self {
                backlog,
                flavor,
                protocol,
                map_capacity,
                linger,
                mtu,
                subscriber,
                pooled,
                port,
            } = self;

            let _span = tracing::info_span!("server").entered();
            let map = secret::map::testing::new(map_capacity);
            let (sender, receiver) = accept::channel(backlog);

            let test_subscriber = Arc::new(subscriber);
            let subscriber = (
                test_subscriber.clone(),
                event::tracing::Subscriber::default(),
            );

            macro_rules! build {
                ($krate:ident, $pooled:expr) => {{
                    let ip: IpAddr = "127.0.0.1".parse().unwrap();
                    let options = crate::socket::Options::new((ip, port).into());

                    let mut env = $krate::Builder::new(subscriber)
                        .with_threads(TEST_THREADS)
                        .with_socket_options(options.clone());

                    if $pooled {
                        let mut pool = udp::Config::new(map.clone());
                        pool.accept_flavor = flavor;
                        pool.reuse_port = true;
                        env = env.with_pool(pool).with_acceptor(sender.clone());
                    }

                    let env = env.build().unwrap();
                    (env, options)
                }};
            }

            let (drop_handle_sender, drop_handle_receiver) = drop_handle::new();
            let (stats_sender, stats_worker, stats) = stats::channel();

            let local_addr = if ::bach::is_active() {
                assert_eq!(Protocol::Udp, protocol, "bach only supports UDP currently");

                let (env, _options) = build!(bach, true);

                env.pool_addr().unwrap()
            } else {
                let (env, options) = build!(tokio, pooled);

                let local_addr = match protocol {
                    Protocol::Tcp => {
                        let socket = options.build_tcp_listener().unwrap();
                        let local_addr = socket.local_addr().unwrap();
                        let socket = ::tokio::io::unix::AsyncFd::new(socket).unwrap();

                        let acceptor = stream_server::tokio::tcp::Acceptor::new(
                            0, socket, &sender, &env, &map, backlog, flavor, linger,
                        )
                        .unwrap();
                        let acceptor = drop_handle_receiver.wrap(acceptor.run());
                        let acceptor = acceptor.instrument(tracing::info_span!("tcp"));
                        ::tokio::task::spawn(acceptor);

                        local_addr
                    }
                    Protocol::Udp if pooled => {
                        // acceptor configured in env
                        env.pool_addr().unwrap()
                    }
                    Protocol::Udp => {
                        let socket = options.build_udp().unwrap();
                        let local_addr = socket.local_addr().unwrap();

                        let socket = ::tokio::io::unix::AsyncFd::new(socket).unwrap();

                        let acceptor = stream_server::tokio::udp::Acceptor::new(
                            0, socket, &sender, &env, &map, flavor,
                        );
                        let acceptor = drop_handle_receiver.wrap(acceptor.run());
                        let acceptor = acceptor.instrument(tracing::info_span!("udp"));
                        ::tokio::task::spawn(acceptor);

                        local_addr
                    }
                    Protocol::Other(name) => {
                        todo!("protocol {name:?} not implemented")
                    }
                };

                // TODO add support for bach
                {
                    let task = stats_worker.run(env.clock().clone());
                    let task = task.instrument(tracing::info_span!("stats"));
                    let task = drop_handle_receiver.wrap(task);
                    env.spawn_reader(task);
                }

                if matches!(flavor, accept::Flavor::Lifo) {
                    let channel = receiver.downgrade();
                    let task =
                        stream_server::accept::Pruner::default().run(env.clone(), channel, stats);
                    let task = task.instrument(tracing::info_span!("pruner"));
                    let task = drop_handle_receiver.wrap(task);
                    env.spawn_reader(task);
                }

                local_addr
            };

            let handle = server::Handle {
                map,
                protocol,
                local_addr,
                mtu,
            };

            if ::bach::is_active() {
                SERVERS.with(|servers| servers.borrow_mut().insert(local_addr, handle.clone()));
            }

            super::Server {
                handle,
                receiver,
                stats: stats_sender,
                drop_handle: drop_handle_sender,
                subscriber: test_subscriber,
                addr_reservation: Arc::new(AddrReservation { local_addr }),
            }
        }
    }
}
