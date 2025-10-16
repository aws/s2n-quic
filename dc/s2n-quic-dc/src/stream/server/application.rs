// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    stream::{
        application::Builder as StreamBuilder,
        environment::{
            tokio::{self as env, Environment},
            Environment as _,
        },
        runtime::tokio as runtime,
        server::{
            accept, stats,
            tokio::{common_builder_methods, uds::Worker},
        },
        socket,
    },
    sync::mpmc,
};
use core::num::{NonZeroU16, NonZeroUsize};
use s2n_quic_core::ensure;
use std::{
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
};
use tracing::Instrument as _;

#[derive(Clone)]
pub struct Server<S: event::Subscriber + Clone> {
    streams: accept::Receiver<S>,
    stats: stats::Sender,
    /// This field retains a reference to the runtime being used
    #[allow(dead_code)]
    env: Environment<S>,
    #[allow(dead_code)]
    acceptor_rt: runtime::Shared<S>,
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
        accept::accept(&self.streams, &self.stats).await
    }
}

/// Default to the SOMAXCONN, similar to rust:
/// https://github.com/rust-lang/rust/blob/28a58f2fa7f0c46b8fab8237c02471a915924fe5/library/std/src/os/unix/net/listener.rs#L104
const DEFAULT_BACKLOG: u16 = libc::SOMAXCONN as _;

pub struct Builder {
    backlog: Option<NonZeroU16>,
    workers: Option<usize>,
    span: Option<tracing::Span>,
    enable_udp: bool,
    enable_tcp: bool,
    accept_flavor: accept::Flavor,
    socket_path: Option<PathBuf>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            backlog: None,
            workers: None,
            span: None,
            enable_udp: true,
            enable_tcp: false,
            accept_flavor: Default::default(),
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

        let concurrency: usize = self.workers.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .unwrap_or_else(|_| 1.try_into().unwrap())
                .into()
        });

        let backlog: usize = self.backlog.map(NonZeroU16::get).unwrap_or(DEFAULT_BACKLOG) as usize;
        let (stream_sender, stream_receiver) = mpmc::new::<StreamBuilder<S>>(backlog);

        let env = env::Builder::new(subscriber)
            .with_threads(concurrency)
            .with_acceptor(stream_sender.clone());

        let env = env.build()?;

        // TODO is it better to spawn one current_thread runtime per concurrency?
        let acceptor_rt: runtime::Shared<S> = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("uds_worker")
            .worker_threads(concurrency)
            .build()?
            .into();

        let mut span = self.span.unwrap_or_else(tracing::span::Span::current);

        if span.is_none() {
            span = tracing::debug_span!("server");
        }

        let (stats_sender, stats_worker, stats) = stats::channel();

        acceptor_rt.spawn(stats_worker.run(env.clock().clone()));

        // Spawn the queue pruner task
        if matches!(self.accept_flavor, accept::Flavor::Lifo) {
            let env = env.clone();
            let channel = stream_receiver.downgrade();
            let stats = stats.clone();

            acceptor_rt.spawn(accept::Pruner::default().run(env, channel, stats));
        }

        let mut server = Server {
            streams: stream_receiver,
            stats: stats_sender,
            env,
            acceptor_rt,
        };

        let path = self.socket_path.ok_or(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unix domain socket path is required",
        ))?;

        Start {
            enable_tcp: self.enable_tcp,
            enable_udp: self.enable_udp,
            accept_flavor: self.accept_flavor,
            server: &mut server,
            stream_sender,
            span,
            socket_path: path,
        }
        .start()?;

        Ok(server)
    }
}

struct Start<'a, S: event::Subscriber + Clone> {
    enable_tcp: bool,
    enable_udp: bool,
    accept_flavor: accept::Flavor,
    server: &'a mut Server<S>,
    stream_sender: accept::Sender<S>,
    span: tracing::Span,
    socket_path: PathBuf,
}

impl<S: event::Subscriber + Clone> Start<'_, S> {
    #[inline]
    fn start(&mut self) -> io::Result<()> {
        let _acceptor = self.server.acceptor_rt.enter();

        self.spawn_worker()?;

        Ok(())
    }

    #[inline]
    fn spawn_worker(&mut self) -> io::Result<()> {
        for protocol in [socket::Protocol::Udp, socket::Protocol::Tcp] {
            match protocol {
                socket::Protocol::Udp => ensure!(self.enable_udp, continue),
                socket::Protocol::Tcp => ensure!(self.enable_tcp, continue),
                _ => continue,
            }
            match protocol {
                socket::Protocol::Udp => {
                    //TODO: udp worker
                }
                socket::Protocol::Tcp => {
                    self.spawn_tcp_worker()?;
                }
                _ => continue,
            }
        }

        Ok(())
    }

    #[inline]
    fn spawn_tcp_worker(&mut self) -> io::Result<()> {
        let socket_worker = Worker::new(
            &self.socket_path,
            &self.server.env,
            &self.stream_sender,
            self.accept_flavor,
        )?;
        let worker_task = socket_worker.run();

        if self.span.is_disabled() {
            self.server.acceptor_rt.spawn(worker_task);
        } else {
            self.server
                .acceptor_rt
                .spawn(worker_task.instrument(self.span.clone()));
        }

        Ok(())
    }
}
