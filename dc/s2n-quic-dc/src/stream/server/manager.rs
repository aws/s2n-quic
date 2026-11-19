// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! dcQUIC stream server forwarding accepted streams over Unix Domain Sockets.
//!
//! This can be used when a single host has many application processes and wants to share the
//! dcQUIC credential state across them. Right now, only dcQUIC streams over TCP are supported.

use crate::{
    event,
    stream::{
        environment::tokio::{self as env, Environment},
        runtime::tokio as runtime,
        server::{
            accept,
            tokio::{
                common_builder_methods, manager_builder_methods, tcp, Handshake, DEFAULT_BACKLOG,
                MAX_TCP_WORKERS,
            },
        },
        socket,
    },
};
use core::num::{NonZeroU16, NonZeroUsize};
use s2n_quic_core::ensure;
use std::{
    io,
    net::SocketAddr,
    os::unix::net::UnixDatagram,
    path::{Path, PathBuf},
    time::Duration,
};
use tracing::Instrument as _;

#[derive(Clone)]
pub struct Server<H: Handshake + Clone, S: event::Subscriber + Clone> {
    local_addr: SocketAddr,
    handshake: H,
    env: Environment<S>,
    acceptor_rt: runtime::Shared<S>,
}

impl<H: Handshake + Clone, S: event::Subscriber + Clone> Server<H, S> {
    #[inline]
    pub fn new(acceptor_addr: SocketAddr, handshake: &H, subscriber: S) -> io::Result<Self> {
        Builder::default()
            .with_address(acceptor_addr)
            .build(handshake.clone(), subscriber)
    }

    pub fn builder() -> Builder {
        Builder::default()
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
    pub fn acceptor_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.local_addr)
    }

    #[inline]
    pub fn handshake_addr(&self) -> io::Result<SocketAddr> {
        Ok(self.handshake.local_addr())
    }
}

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
    socket_path: Option<PathBuf>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            backlog: None,
            workers: None,
            // FIXME: Don't default to a fixed port?
            #[expect(
                clippy::unwrap_used,
                reason = "parsing a compile-time constant socket address that is known valid"
            )]
            acceptor_addr: "[::]:4444".parse().unwrap(),
            span: None,
            enable_udp: true,
            enable_tcp: false,
            linger: None,
            accept_flavor: Default::default(),
            send_buffer: None,
            recv_buffer: None,
            reuse_addr: None,
            socket_path: None,
        }
    }
}

impl Builder {
    common_builder_methods!();
    manager_builder_methods!();

    pub fn with_socket_path(mut self, path: &Path) -> Self {
        self.socket_path = Some(path.to_path_buf());
        self
    }

    pub fn build<H: Handshake + Clone, S: event::Subscriber + Clone>(
        self,
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

        let socket_path = self.socket_path.ok_or(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unix domain socket path is required",
        ))?;

        #[expect(
            clippy::unwrap_used,
            reason = "converting the constant 1 to NonZeroUsize is infallible since 1 is non-zero"
        )]
        let concurrency: usize = self.workers.unwrap_or_else(|| {
            std::thread::available_parallelism()
                .unwrap_or_else(|_| 1.try_into().unwrap())
                .into()
        });

        let backlog: usize = self.backlog.map(NonZeroU16::get).unwrap_or(DEFAULT_BACKLOG) as usize;

        // If we've not enabled UDP support, clamp the # of threads we spawn to the TCP worker
        // count. No reason to spawn extra threads that would largely just sit idle.
        let acceptor_concurrency = if self.enable_udp {
            concurrency
        } else {
            concurrency.clamp(1, MAX_TCP_WORKERS)
        };

        // split the backlog between all of the workers
        // this is only used in TCP, so clamp division to maximum TCP worker concurrency
        let backlog = backlog
            .div_ceil(acceptor_concurrency.clamp(0, MAX_TCP_WORKERS))
            .max(1);

        if self.enable_tcp {
            // Each TCP acceptor connects its own sender in `spawn_tcp`, which needs a runtime to
            // register with. Connecting a probe here reports a missing or unbound receiver
            // socket before any runtime exists.
            let probe = UnixDatagram::unbound()?;
            probe.connect(&socket_path)?;
        }

        let mut bind = Bind {
            enable_tcp: self.enable_tcp,
            enable_udp: self.enable_udp,
            concurrency: acceptor_concurrency,
            local_addr: self.acceptor_addr,
            send_buffer: self.send_buffer,
            recv_buffer: self.recv_buffer,
            reuse_addr: self.reuse_addr.unwrap_or(false),
            udp_sockets: Vec::new(),
            tcp_listeners: Vec::new(),
        };
        bind.bind()?;

        // FIXME: This is configuring runtimes that likely won't be used as the manager doesn't do
        // dataplane I/O. For now, do the easy thing and configure just one thread per threadpool.
        // In the future we'll want to get rid of them entirely.
        let env = env::Builder::new(subscriber)
            .with_threads(1)
            .with_thread_name_prefix("mgr-dc".into());

        let enable_udp_pool = true;

        if self.enable_udp && enable_udp_pool {
            // TODO UDP
        }

        let env = env.build()?;

        if self.enable_udp && enable_udp_pool {
            // TODO UDP
        }

        let acceptor_rt: runtime::Shared<S> = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("mgr-acceptor")
            .worker_threads(acceptor_concurrency)
            .build()?
            .into();

        let mut span = self.span.unwrap_or_else(tracing::span::Span::current);

        if span.is_none() {
            span = tracing::debug_span!("server");
        }

        let mut server = Server {
            local_addr: bind.local_addr,
            handshake,
            env,
            acceptor_rt,
        };

        Start {
            accept_flavor: self.accept_flavor,
            linger: self.linger,
            backlog,
            server: &mut server,
            span,
            next_id: 0,
            socket_path,
        }
        .start(bind.udp_sockets, bind.tcp_listeners)?;

        Ok(server)
    }
}

/// Binds the acceptor sockets, which doesn't need a runtime.
struct Bind {
    enable_tcp: bool,
    enable_udp: bool,
    concurrency: usize,
    local_addr: SocketAddr,
    send_buffer: Option<usize>,
    recv_buffer: Option<usize>,
    reuse_addr: bool,
    udp_sockets: Vec<std::net::UdpSocket>,
    tcp_listeners: Vec<std::net::TcpListener>,
}

impl Bind {
    #[inline]
    fn bind(&mut self) -> io::Result<()> {
        // check if we need to find a port for which both types are free
        if self.enable_tcp && self.enable_udp && self.local_addr.port() == 0 {
            // find a port and bind the initial listeners
            self.bind_initial_wildcard_pair()?;
            // bind the rest of the concurrency
            self.bind_count(self.concurrency - 1, 1)?;
        } else {
            // otherwise bind things as normal
            self.bind_count(self.concurrency, 0)?;
        }

        debug_assert_ne!(self.local_addr.port(), 0, "a port should be selected");

        Ok(())
    }

    #[inline]
    fn bind_initial_wildcard_pair(&mut self) -> io::Result<()> {
        debug_assert!(self.enable_tcp);
        debug_assert!(self.enable_udp);

        let (local_addr, udp_socket, tcp_socket) =
            super::spawn_initial_wildcard_pair(self.local_addr, |addr| self.socket_opts(addr))?;
        self.local_addr = local_addr;
        self.udp_sockets.push(udp_socket);
        self.push_tcp(tcp_socket)?;
        Ok(())
    }

    #[inline]
    fn bind_count(&mut self, count: usize, already_running: usize) -> io::Result<()> {
        for protocol in [socket::Protocol::Udp, socket::Protocol::Tcp] {
            match protocol {
                socket::Protocol::Udp => ensure!(self.enable_udp, continue),
                socket::Protocol::Tcp => ensure!(self.enable_tcp, continue),
                _ => continue,
            }

            for idx in 0..count {
                match protocol {
                    socket::Protocol::Udp => {
                        let socket = self.socket_opts(self.local_addr).build_udp()?;
                        self.udp_sockets.push(socket);
                    }
                    socket::Protocol::Tcp => {
                        if idx + already_running >= MAX_TCP_WORKERS {
                            continue;
                        }

                        let socket = self.socket_opts(self.local_addr).build_tcp_listener()?;
                        self.push_tcp(socket)?;
                    }
                    _ => continue,
                }
            }
        }

        Ok(())
    }

    #[inline]
    fn push_tcp(&mut self, socket: std::net::TcpListener) -> io::Result<()> {
        // if this is the first socket being bound then update the local address
        if self.local_addr.port() == 0 {
            self.local_addr = socket.local_addr()?;
        }

        self.tcp_listeners.push(socket);
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
}

/// Spawns the acceptors for sockets bound by [`Bind`] onto the server's runtimes.
struct Start<'a, H: Handshake + Clone, S: event::Subscriber + Clone> {
    accept_flavor: accept::Flavor,
    backlog: usize,
    server: &'a mut Server<H, S>,
    span: tracing::Span,
    next_id: usize,
    linger: Option<Duration>,
    socket_path: PathBuf,
}

impl<H: Handshake + Clone, S: event::Subscriber + Clone> Start<'_, H, S> {
    #[inline]
    fn start(
        &mut self,
        udp_sockets: Vec<std::net::UdpSocket>,
        tcp_listeners: Vec<std::net::TcpListener>,
    ) -> io::Result<()> {
        let _acceptor = self.server.acceptor_rt.enter();

        for socket in udp_sockets {
            self.spawn_udp(socket)?;
        }

        for socket in tcp_listeners {
            self.spawn_tcp(socket)?;
        }

        Ok(())
    }

    #[inline]
    fn spawn_udp(&mut self, _socket: std::net::UdpSocket) -> io::Result<()> {
        // TODO UDP

        Ok(())
    }

    #[inline]
    fn spawn_tcp(&mut self, socket: std::net::TcpListener) -> io::Result<()> {
        let socket = unsafe {
            // SAFETY: `socket` is owned by the AsyncFd, so its fd is stable until drop.
            tokio::io::unix::AsyncFd::register(socket)
        }?;
        let id = self.id();

        let socket_behavior = tcp::worker::SocketBehavior::new(&self.socket_path)?;
        let acceptor = tcp::Acceptor::new(
            id,
            socket,
            &self.server.env,
            self.server.handshake.map(),
            self.backlog,
            self.accept_flavor,
            self.linger,
            socket_behavior,
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
