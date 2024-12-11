// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{server::tokio::stats, socket::Protocol};
use crate::{
    event,
    path::secret,
    stream::{
        application::Stream,
        client::tokio as client,
        environment::{tokio as env, Environment as _},
        server::tokio::{self as server, accept},
    },
};
use std::{io, net::SocketAddr};
use tracing::Instrument;

type Subscriber = event::tracing::Subscriber;

pub struct Client {
    map: secret::Map,
    env: env::Environment<Subscriber>,
}

impl Default for Client {
    fn default() -> Self {
        let _span = tracing::info_span!("client").entered();
        let map = secret::map::testing::new(16);
        Self {
            map,
            env: Default::default(),
        }
    }
}

impl Client {
    pub fn handshake_with<S: AsRef<ServerHandle>>(
        &self,
        server: &S,
    ) -> io::Result<secret::map::Peer> {
        let server = server.as_ref();
        let peer = server.local_addr;
        if let Some(peer) = self.map.get_tracked(peer) {
            return Ok(peer);
        }

        let local_addr = "127.0.0.1:1337".parse().unwrap();
        self.map
            .test_insert_pair(local_addr, &server.map, server.local_addr);

        // cache hit already tracked above
        self.map.get_untracked(peer).ok_or_else(|| {
            io::Error::new(io::ErrorKind::AddrNotAvailable, "path secret not available")
        })
    }

    pub async fn connect_to<S: AsRef<ServerHandle>>(
        &self,
        server: &S,
    ) -> io::Result<Stream<Subscriber>> {
        let server = server.as_ref();
        let handshake = async { self.handshake_with(server) };

        let subscriber = Subscriber::default();

        match server.protocol {
            Protocol::Tcp => {
                client::connect_tcp(handshake, server.local_addr, &self.env, subscriber).await
            }
            Protocol::Udp => {
                client::connect_udp(handshake, server.local_addr, &self.env, subscriber).await
            }
            Protocol::Other(name) => {
                todo!("protocol {name:?} not implemented")
            }
        }
    }
}

#[derive(Clone)]
pub struct ServerHandle {
    map: secret::Map,
    protocol: Protocol,
    local_addr: SocketAddr,
}

impl AsRef<ServerHandle> for ServerHandle {
    fn as_ref(&self) -> &ServerHandle {
        self
    }
}

pub struct Server {
    handle: ServerHandle,
    receiver: accept::Receiver<Subscriber>,
    stats: stats::Sender,
    #[allow(dead_code)]
    drop_handle: drop_handle::Sender,
}

impl Default for Server {
    fn default() -> Self {
        Self::new_udp(accept::Flavor::Fifo)
    }
}

impl AsRef<ServerHandle> for Server {
    fn as_ref(&self) -> &ServerHandle {
        &self.handle
    }
}

impl Server {
    pub fn new_tcp(accept_flavor: accept::Flavor) -> Self {
        Self::new(Protocol::Tcp, accept_flavor)
    }

    pub fn new_udp(accept_flavor: accept::Flavor) -> Self {
        Self::new(Protocol::Udp, accept_flavor)
    }

    pub fn new(protocol: Protocol, accept_flavor: accept::Flavor) -> Self {
        if s2n_quic_platform::io::testing::is_in_env() {
            todo!()
        } else {
            Self::new_tokio(protocol, accept_flavor)
        }
    }

    fn new_tokio(protocol: Protocol, accept_flavor: accept::Flavor) -> Self {
        let _span = tracing::info_span!("server").entered();
        let map = secret::map::testing::new(16);
        let (sender, receiver) = accept::channel(16);

        let options = crate::socket::Options::new("127.0.0.1:0".parse().unwrap());

        let env = env::Builder::default().build().unwrap();

        let subscriber = event::tracing::Subscriber::default();
        let (drop_handle_sender, drop_handle_receiver) = drop_handle::new();

        let local_addr = match protocol {
            Protocol::Tcp => {
                let socket = options.build_tcp_listener().unwrap();
                let local_addr = socket.local_addr().unwrap();
                let socket = tokio::net::TcpListener::from_std(socket).unwrap();

                let acceptor = server::tcp::Acceptor::new(
                    0,
                    socket,
                    &sender,
                    &env,
                    &map,
                    16,
                    accept_flavor,
                    subscriber,
                );
                let acceptor = drop_handle_receiver.wrap(acceptor.run());
                let acceptor = acceptor.instrument(tracing::info_span!("tcp"));
                tokio::task::spawn(acceptor);

                local_addr
            }
            Protocol::Udp => {
                let socket = options.build_udp().unwrap();
                let local_addr = socket.local_addr().unwrap();

                let socket = tokio::io::unix::AsyncFd::new(socket).unwrap();

                let acceptor = server::udp::Acceptor::new(
                    0,
                    socket,
                    &sender,
                    &env,
                    &map,
                    accept_flavor,
                    subscriber,
                );
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

        if matches!(accept_flavor, accept::Flavor::Lifo) {
            let channel = receiver.downgrade();
            let task = accept::Pruner::default().run(env, channel, stats);
            let task = task.instrument(tracing::info_span!("pruner"));
            let task = drop_handle_receiver.wrap(task);
            tokio::task::spawn(task);
        }

        let handle = ServerHandle {
            map,
            protocol,
            local_addr,
        };

        Self {
            handle,
            receiver,
            stats: stats_sender,
            drop_handle: drop_handle_sender,
        }
    }

    pub fn handle(&self) -> ServerHandle {
        self.handle.clone()
    }

    pub async fn accept(&self) -> io::Result<(Stream<Subscriber>, SocketAddr)> {
        accept::accept(&self.receiver, &self.stats).await
    }
}

mod drop_handle {
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

    pub struct Sender(#[allow(dead_code)] watch::Sender<()>);
}
