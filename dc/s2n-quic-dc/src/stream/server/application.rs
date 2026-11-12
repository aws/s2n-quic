// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Server accepting from a [`crate::stream::server::manager`].

use crate::{
    event,
    stream::{
        environment::{tokio as env, udp::Config as PoolConfig},
        server::tokio::{
            common_builder_methods,
            uds::{self, ApplicationDataDeserializer},
        },
        socket,
    },
};
use s2n_quic_core::ensure;
use std::{
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use tracing::Instrument as _;

#[derive(Clone)]
pub struct Server<S: event::Subscriber + Clone> {
    receiver: uds::Receiver<S>,
    span: tracing::Span,
}

impl<S: event::Subscriber + Clone> Server<S> {
    #[inline]
    pub fn new(subscriber: S) -> io::Result<Self> {
        Builder::default().build(subscriber)
    }

    pub fn builder() -> Builder {
        Builder::default()
    }

    #[inline]
    pub async fn accept(&self) -> io::Result<(crate::stream::application::Stream<S>, SocketAddr)> {
        let stream = if self.span.is_disabled() {
            self.receiver.receive_stream().await?
        } else {
            self.receiver
                .receive_stream()
                .instrument(self.span.clone())
                .await?
        };
        let (stream, _sojourn_time) = stream.accept()?;
        let remote_addr = stream.peer_addr()?;

        Ok((stream, remote_addr))
    }
}

pub struct Builder {
    span: Option<tracing::Span>,
    enable_udp: bool,
    enable_tcp: bool,
    udp_address: SocketAddr,
    socket_path: Option<PathBuf>,
    application_data_deserializer: Option<ApplicationDataDeserializer>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            span: None,
            enable_udp: false,
            enable_tcp: false,
            udp_address: SocketAddr::from((std::net::Ipv6Addr::UNSPECIFIED, 0)),
            socket_path: None,
            application_data_deserializer: None,
        }
    }
}

impl Builder {
    common_builder_methods!();

    /// The provided path will be used to unlink + bind a Unix socket. Users on the local system
    /// that can connect to this socket can send traffic to the application with no additional
    /// authentication being performed, so appropriate access controls should be enforced. These
    /// are the responsibility of the application owner.
    pub fn with_socket_path(mut self, path: &Path) -> Self {
        self.socket_path = Some(path.to_path_buf());
        self
    }

    /// Sets the application's UDP address (default: `[::]:0`).
    /// Use the manager's IP and typically port `0`; clients follow replies only from that IP.
    pub fn with_udp_address(mut self, addr: SocketAddr) -> Self {
        self.udp_address = addr;
        self
    }

    /// Registers a callback that reconstructs the path secret's
    /// [`ApplicationData`](crate::path::secret::map::ApplicationData) from the opaque blob carried
    /// in a UDS handoff packet. Accepted streams will then have their `application_data()`
    /// populated with the value returned by the callback. Absent this registration, accepted
    /// streams carry no path-secret `ApplicationData` (pre-existing behavior).
    pub fn with_application_data_deserializer(
        mut self,
        deserializer: ApplicationDataDeserializer,
    ) -> Self {
        self.application_data_deserializer = Some(deserializer);
        self
    }

    pub fn build<S: event::Subscriber + Clone>(self, subscriber: S) -> io::Result<Server<S>> {
        ensure!(
            self.enable_udp || self.enable_tcp,
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "at least one acceptor type needs to be enabled"
            ))
        );

        let map = uds::placeholder_map(subscriber.clone());
        let mut env = env::Builder::new(subscriber);

        if self.enable_udp {
            let mut pool = PoolConfig::new(map.clone());
            pool.route_by_credentials = true;
            env = env
                .with_socket_options(socket::Options::new(self.udp_address))
                .with_pool(pool);
        } else {
            // If we only have TCP, there shouldn't be significant load on the background worker
            // threads, so reduce thread count. Only the background shutdowns get sent there for
            // TCP workloads.
            env = env.with_threads(1);
        }

        let env = env.build()?;

        let mut span = self.span.unwrap_or_else(tracing::span::Span::current);
        if span.is_none() {
            span = tracing::debug_span!("server");
        }

        let path = self.socket_path.ok_or(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unix domain socket path is required",
        ))?;
        let mut receiver = uds::Receiver::new_with_map(&path, &env, map)?;
        if let Some(deserializer) = self.application_data_deserializer {
            receiver = receiver.with_application_data_deserializer(deserializer);
        }
        let server = Server { receiver, span };

        Ok(server)
    }
}
