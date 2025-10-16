// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    path::secret,
    stream::{
        application::Builder as StreamBuilder,
        environment::{
            tokio::{self as env, Environment},
            udp as udp_pool, Environment as _,
        },
        runtime::tokio as runtime,
        server::{accept, stats},
        socket,
    },
    sync::mpmc,
};
use core::num::{NonZeroU16, NonZeroUsize};
use s2n_quic_core::ensure;
use std::{io, net::SocketAddr, time::Duration};
use tokio::io::unix::AsyncFd;
use tracing::{trace, Instrument as _};

// The kernel contends on fdtable lock when calling accept to locate free file
// descriptors, so even if we don't contend on the underlying socket (due to
// REUSEPORT) it still ends up expensive to have large amounts of threads trying to
// accept() within a single process. Clamp concurrency to at most 4 threads
// executing the TCP acceptor tasks accordingly.
//
// With UDP there's ~no lock contention for receiving packets on separate UDP sockets,
// so we don't clamp concurrency in that case.
pub const MAX_TCP_WORKERS: usize = 4;

pub mod tcp;
pub mod udp;
pub mod uds;

// This trait is a solution to abstract local_addr and map methods
pub trait Handshake: Clone {
    fn local_addr(&self) -> SocketAddr;

    fn map(&self) -> &secret::Map;
}

impl Handshake for crate::psk::server::Provider {
    fn local_addr(&self) -> SocketAddr {
        self.local_addr()
    }

    fn map(&self) -> &secret::Map {
        self.map()
    }
}

#[derive(Clone)]
pub struct Server<H: Handshake + Clone, S: event::Subscriber + Clone> {
    streams: accept::Receiver<S>,
    local_addr: SocketAddr,
    handshake: H,
    stats: stats::Sender,
    /// This field retains a reference to the runtime being used
    #[allow(dead_code)]
    env: Environment<S>,
    #[allow(dead_code)]
    acceptor_rt: runtime::Shared<S>,
}

impl<H: Handshake + Clone, S: event::Subscriber + Clone> Server<H, S> {
    #[inline]
    pub fn new(acceptor_addr: SocketAddr, handshake: H, subscriber: S) -> io::Result<Self> {
        Builder::default()
            .with_address(acceptor_addr)
            .build(handshake, subscriber)
    }

    pub fn builder() -> Builder {
        Builder::default()
    }

    pub fn drop_state(&self) {
        self.handshake.map().drop_state()
    }

    pub fn handshake_state(&self) -> &H {
        &self.handshake
    }

    /// Should generally only be used for advanced users.
    ///
    /// This should not be used for spawning heavy-weight work (e.g., request processing), and is
    /// generally best used for tiny tasks which intermediate to some other runtime. For example,
    /// it can work well for having some small processing to then send into another channel.
    pub fn acceptor_rt(&self) -> tokio::runtime::Handle {
        (*self.acceptor_rt).clone()
    }

    #[inline]
    pub async fn accept(&self) -> io::Result<(crate::stream::application::Stream<S>, SocketAddr)> {
        accept::accept(&self.streams, &self.stats).await
    }

    #[inline]
    pub fn acceptor_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local_addr)
    }

    #[inline]
    pub fn handshake_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.handshake.local_addr())
    }
}

/// Default to the SOMAXCONN, similar to rust:
/// https://github.com/rust-lang/rust/blob/28a58f2fa7f0c46b8fab8237c02471a915924fe5/library/std/src/os/unix/net/listener.rs#L104
const DEFAULT_BACKLOG: u16 = libc::SOMAXCONN as _;

pub struct Builder {
    backlog: Option<NonZeroU16>,
    workers: Option<usize>,
    acceptor_addr: SocketAddr,
    span: Option<tracing::Span>,
    enable_udp: bool,
    enable_tcp: bool,
    accept_flavor: accept::Flavor,
    linger: Option<Duration>,
    send_buffer: Option<usize>,
    recv_buffer: Option<usize>,
    reuse_addr: Option<bool>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            backlog: None,
            workers: None,
            // FIXME: Don't default to a fixed port?
            acceptor_addr: "[::]:4444".parse().unwrap(),
            span: None,
            enable_udp: true,
            enable_tcp: false,
            linger: None,
            accept_flavor: Default::default(),
            send_buffer: None,
            recv_buffer: None,
            reuse_addr: None,
        }
    }
}

macro_rules! common_builder_methods {
    () => {
        pub fn with_protocol(mut self, protocol: socket::Protocol) -> Self {
            match protocol {
                socket::Protocol::Udp => {
                    self.enable_udp = true;
                    self.enable_tcp = false
                }
                socket::Protocol::Tcp => {
                    self.enable_udp = false;
                    self.enable_tcp = true;
                }
                _ => {
                    self.enable_udp = false;
                    self.enable_tcp = false;
                }
            }
            self
        }

        pub fn with_udp(mut self, enabled: bool) -> Self {
            self.enable_udp = enabled;
            self
        }

        pub fn with_tcp(mut self, enabled: bool) -> Self {
            self.enable_tcp = enabled;
            self
        }
    };
}
macro_rules! manager_builder_methods {
    () => {
        pub fn with_address(mut self, addr: SocketAddr) -> Self {
            self.acceptor_addr = addr;
            self
        }

        pub fn with_backlog(mut self, backlog: NonZeroU16) -> Self {
            self.backlog = Some(backlog);
            self
        }

        pub fn with_workers(mut self, workers: NonZeroUsize) -> Self {
            self.workers = Some(workers.into());
            self
        }

        pub fn with_linger(mut self, linger: Duration) -> Self {
            self.linger = Some(linger);
            self
        }

        pub fn with_send_buffer(mut self, bytes: usize) -> Self {
            self.send_buffer = Some(bytes);
            self
        }

        pub fn with_recv_buffer(mut self, bytes: usize) -> Self {
            self.recv_buffer = Some(bytes);
            self
        }

        /// Sets the reuse address option for the OS socket handle.
        ///
        /// This allows the application to bind to a previously used local address.
        /// In TCP, this can be useful when a closed socket is in the `TIME_WAIT` state and the application
        /// would like to reuse that address immediately.
        /// On Linux packets are routed to the most recently bound socket.
        ///
        /// See `SO_REUSEADDR` for more information.
        pub fn with_reuse_addr(mut self, enabled: bool) -> Self {
            self.reuse_addr = Some(enabled);
            self
        }
    };
}

impl Builder {
    common_builder_methods!();
    manager_builder_methods!();

    pub fn build<H: Handshake + Clone, S: event::Subscriber + Clone>(
        mut self,
        handshake: H,
        subscriber: S,
    ) -> io::Result<Server<H, S>> {
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

        let mut env = env::Builder::new(subscriber)
            .with_threads(concurrency)
            .with_acceptor(stream_sender.clone());

        let enable_udp_pool = true;

        if self.enable_udp && enable_udp_pool {
            // configure the socket options with the specified address
            let mut options = socket::Options::new(self.acceptor_addr);

            options.send_buffer = self.send_buffer;
            options.recv_buffer = self.recv_buffer;

            env = env.with_socket_options(options);

            let mut pool = udp_pool::Config::new(handshake.map().clone());

            pool.reuse_port = concurrency > 1;
            pool.accept_flavor = self.accept_flavor;

            env = env.with_pool(pool);
        }

        let env = env.build()?;

        if self.enable_udp && enable_udp_pool {
            // update the address with the selected port
            self.acceptor_addr = env.pool_addr().unwrap();
            // don't use the owned socket acceptor
            self.enable_udp = false;
        }

        // TODO is it better to spawn one current_thread runtime per concurrency?
        let acceptor_rt: runtime::Shared<S> = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("acceptor")
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
            local_addr: self.acceptor_addr,
            handshake,
            stats: stats_sender,
            env,
            acceptor_rt,
        };

        // split the backlog between all of the workers
        //
        // this is only used in TCP, so clamp division to maximum TCP worker concurrency
        let backlog = backlog
            .div_ceil(concurrency.clamp(0, MAX_TCP_WORKERS))
            .max(1);

        Start {
            enable_tcp: self.enable_tcp,
            enable_udp: self.enable_udp,
            accept_flavor: self.accept_flavor,
            linger: self.linger,
            backlog,
            concurrency,
            server: &mut server,
            stream_sender,
            span,
            next_id: 0,
            send_buffer: self.send_buffer,
            recv_buffer: self.recv_buffer,
            reuse_addr: self.reuse_addr.unwrap_or(false),
        }
        .start()?;

        Ok(server)
    }
}

struct Start<'a, H: Handshake + Clone, S: event::Subscriber + Clone> {
    enable_tcp: bool,
    enable_udp: bool,
    accept_flavor: accept::Flavor,
    backlog: usize,
    concurrency: usize,
    server: &'a mut Server<H, S>,
    stream_sender: accept::Sender<S>,
    span: tracing::Span,
    next_id: usize,
    linger: Option<Duration>,
    send_buffer: Option<usize>,
    recv_buffer: Option<usize>,
    reuse_addr: bool,
}

impl<H: Handshake + Clone, S: event::Subscriber + Clone> Start<'_, H, S> {
    #[inline]
    fn start(&mut self) -> io::Result<()> {
        let _acceptor = self.server.acceptor_rt.enter();

        // check if we need to find a port for which both types are free
        if self.enable_tcp && self.enable_udp && self.server.local_addr.port() == 0 {
            // find a port and spawn the initial listeners
            self.spawn_initial_wildcard_pair()?;
            // spawn the rest of the concurrency
            self.spawn_count(self.concurrency - 1, 1)?;
        } else {
            // otherwise spawn things as normal
            self.spawn_count(self.concurrency, 0)?;
        }

        debug_assert_ne!(
            self.server.local_addr.port(),
            0,
            "a port should be selected"
        );

        Ok(())
    }

    #[inline]
    fn spawn_initial_wildcard_pair(&mut self) -> io::Result<()> {
        debug_assert!(self.enable_tcp);
        debug_assert!(self.enable_udp);
        debug_assert_eq!(self.server.local_addr.port(), 0);

        // try 10 times before bailing
        for iteration in 0..10 {
            trace!(wildcard_search_iteration = iteration);
            let udp_socket = self.socket_opts(self.server.local_addr).build_udp()?;
            let local_addr = udp_socket.local_addr()?;
            trace!(candidate = %local_addr);
            match self.socket_opts(local_addr).build_tcp_listener() {
                Ok(tcp_socket) => {
                    trace!(selected = %local_addr);
                    // we found a port that both protocols can use
                    self.server.local_addr = local_addr;
                    self.spawn_udp(udp_socket)?;
                    self.spawn_tcp(tcp_socket)?;
                    return Ok(());
                }
                Err(err) if err.kind() == io::ErrorKind::AddrInUse => {
                    // try to find another address
                    continue;
                }
                // bubble up all other error types
                Err(err) => return Err(err),
            }
        }

        // we couldn't find a free port so return and error
        Err(io::ErrorKind::AddrInUse.into())
    }

    #[inline]
    fn spawn_count(&mut self, count: usize, already_running: usize) -> io::Result<()> {
        for protocol in [socket::Protocol::Udp, socket::Protocol::Tcp] {
            match protocol {
                socket::Protocol::Udp => ensure!(self.enable_udp, continue),
                socket::Protocol::Tcp => ensure!(self.enable_tcp, continue),
                _ => continue,
            }

            for idx in 0..count {
                match protocol {
                    socket::Protocol::Udp => {
                        let socket = self.socket_opts(self.server.local_addr).build_udp()?;
                        self.spawn_udp(socket)?;
                    }
                    socket::Protocol::Tcp => {
                        if idx + already_running >= MAX_TCP_WORKERS {
                            continue;
                        }

                        let socket = self
                            .socket_opts(self.server.local_addr)
                            .build_tcp_listener()?;
                        self.spawn_tcp(socket)?;
                    }
                    _ => continue,
                }
            }
        }

        Ok(())
    }

    #[inline]
    fn socket_opts(&self, local_addr: SocketAddr) -> socket::Options {
        let mut options = socket::Options::new(local_addr);

        // Explicitly do **not** set the socket backlog to self.backlog. While we split the
        // configured backlog amongst our in-process queues as concurrency increases, it doesn't
        // make sense to shrink the kernel backlogs -- that just causes packet drops and generally
        // bad behavior.
        //
        // This is especially true for TCP where we don't have workers matching concurrency.
        options.send_buffer = self.send_buffer;
        options.recv_buffer = self.recv_buffer;
        options.reuse_address = self.reuse_addr;

        // if we have more than one thread then we'll need to use reuse port
        if self.concurrency > 1 {
            // if the application is wanting to bind to a random port then we need to set
            // reuse_port after
            if local_addr.port() == 0 {
                options.reuse_port = socket::ReusePort::AfterBind;
            } else {
                options.reuse_port = socket::ReusePort::BeforeBind;
            }
        }

        options
    }

    #[inline]
    fn spawn_udp(&mut self, socket: std::net::UdpSocket) -> io::Result<()> {
        // if this is the first socket being spawned then update the local address
        if self.server.local_addr.port() == 0 {
            self.server.local_addr = socket.local_addr()?;
        }

        let socket = AsyncFd::new(socket)?;
        let id = self.id();

        let acceptor = udp::Acceptor::new(
            id,
            socket,
            &self.stream_sender,
            &self.server.env,
            self.server.handshake.map(),
            self.accept_flavor,
        )
        .run();

        if self.span.is_disabled() {
            self.server.acceptor_rt.spawn(acceptor);
        } else {
            self.server
                .acceptor_rt
                .spawn(acceptor.instrument(self.span.clone()));
        }

        Ok(())
    }

    #[inline]
    fn spawn_tcp(&mut self, socket: std::net::TcpListener) -> io::Result<()> {
        // if this is the first socket being spawned then update the local address
        if self.server.local_addr.port() == 0 {
            self.server.local_addr = socket.local_addr()?;
        }

        let socket = tokio::io::unix::AsyncFd::new(socket)?;
        let id = self.id();
        let channel_behavior = tcp::worker::DefaultBehavior::new(&self.stream_sender);
        let acceptor = tcp::Acceptor::new(
            id,
            socket,
            &self.server.env,
            self.server.handshake.map(),
            self.backlog,
            self.accept_flavor,
            self.linger,
            channel_behavior,
        )?
        .run();

        if self.span.is_disabled() {
            self.server.acceptor_rt.spawn(acceptor);
        } else {
            self.server
                .acceptor_rt
                .spawn(acceptor.instrument(self.span.clone()));
        }

        Ok(())
    }

    fn id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
}

pub(crate) use common_builder_methods;
pub(crate) use manager_builder_methods;
