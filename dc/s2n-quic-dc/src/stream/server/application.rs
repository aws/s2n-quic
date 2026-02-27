// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Server accepting from a [`crate::stream::server::manager`].

use crate::{
    event,
    stream::{
        environment::tokio as env,
        server::tokio::{common_builder_methods, uds},
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
    socket_path: Option<PathBuf>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            span: None,
            enable_udp: true,
            enable_tcp: false,
            socket_path: None,
        }
    }
}

impl Builder {
    common_builder_methods!();

    pub fn with_socket_path(mut self, path: &Path) -> Self {
        self.socket_path = Some(path.to_path_buf());
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

        let env = env::Builder::new(subscriber).build()?;

        let mut span = self.span.unwrap_or_else(tracing::span::Span::current);
        if span.is_none() {
            span = tracing::debug_span!("server");
        }

        let path = self.socket_path.ok_or(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unix domain socket path is required",
        ))?;
        let receiver = uds::Receiver::new(&path, &env)?;
        let server = Server { receiver, span };

        Ok(server)
    }
}
