// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    clock::tokio::Clock,
    event,
    stream::{
        runtime::{tokio as runtime, ArcHandle},
        socket::{self, Socket as _},
        TransportFeatures,
    },
};
use s2n_quic_core::{
    ensure,
    inet::{SocketAddress, Unspecified},
};
use s2n_quic_platform::features;
use std::{io, net::UdpSocket, sync::Arc};
use tokio::{io::unix::AsyncFd, net::TcpStream};

#[derive(Clone)]
pub struct Builder<Sub> {
    clock: Option<Clock>,
    gso: Option<features::Gso>,
    socket_options: Option<socket::Options>,
    reader_rt: Option<runtime::Shared<Sub>>,
    writer_rt: Option<runtime::Shared<Sub>>,
    thread_name_prefix: Option<String>,
    threads: Option<usize>,
}

impl<Sub> Default for Builder<Sub> {
    fn default() -> Self {
        Self {
            clock: None,
            gso: None,
            socket_options: None,
            reader_rt: None,
            writer_rt: None,
            thread_name_prefix: None,
            threads: None,
        }
    }
}

impl<Sub> Builder<Sub>
where
    Sub: event::Subscriber,
{
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = Some(threads);
        self
    }

    #[inline]
    pub fn build(self) -> io::Result<Environment<Sub>> {
        let clock = self.clock.unwrap_or_default();
        let gso = self.gso.unwrap_or_default();
        let socket_options = self.socket_options.unwrap_or_default();

        let thread_name_prefix = self.thread_name_prefix.as_deref().unwrap_or("dc_quic");

        let make_rt = |suffix: &str, threads: Option<usize>| {
            let mut builder = tokio::runtime::Builder::new_multi_thread();
            if let Some(threads) = threads {
                builder.worker_threads(threads);
            }
            Ok(builder
                .enable_all()
                .thread_name(format!("{thread_name_prefix}::{suffix}"))
                .build()?
                .into())
        };

        let reader_rt = self
            .reader_rt
            .map(<io::Result<_>>::Ok)
            .unwrap_or_else(|| make_rt("reader", self.threads))?;
        let writer_rt = self
            .writer_rt
            .map(<io::Result<_>>::Ok)
            .unwrap_or_else(|| make_rt("writer", self.threads))?;

        Ok(Environment {
            clock,
            gso,
            socket_options,
            reader_rt,
            writer_rt,
        })
    }
}

#[derive(Clone)]
pub struct Environment<Sub> {
    clock: Clock,
    gso: features::Gso,
    socket_options: socket::Options,
    reader_rt: runtime::Shared<Sub>,
    writer_rt: runtime::Shared<Sub>,
}

impl<Sub> Default for Environment<Sub>
where
    Sub: event::Subscriber,
{
    #[inline]
    fn default() -> Self {
        Self::builder().build().unwrap()
    }
}

impl<Sub> Environment<Sub> {
    #[inline]
    pub fn builder() -> Builder<Sub> {
        Default::default()
    }
}

impl<Sub> super::Environment for Environment<Sub>
where
    Sub: event::Subscriber,
{
    type Clock = Clock;
    type Subscriber = Sub;

    #[inline]
    fn clock(&self) -> &Self::Clock {
        &self.clock
    }

    #[inline]
    fn gso(&self) -> features::Gso {
        self.gso.clone()
    }

    #[inline]
    fn reader_rt(&self) -> ArcHandle<Self::Subscriber> {
        self.reader_rt.handle()
    }

    #[inline]
    fn spawn_reader<F: 'static + Send + std::future::Future<Output = ()>>(&self, f: F) {
        self.reader_rt.spawn(f);
    }

    #[inline]
    fn writer_rt(&self) -> ArcHandle<Self::Subscriber> {
        self.writer_rt.handle()
    }

    #[inline]
    fn spawn_writer<F: 'static + Send + std::future::Future<Output = ()>>(&self, f: F) {
        self.writer_rt.spawn(f);
    }
}

#[derive(Clone, Copy, Debug)]
pub struct UdpUnbound(pub SocketAddress);

impl<Sub> super::Peer<Environment<Sub>> for UdpUnbound
where
    Sub: event::Subscriber,
{
    type WorkerSocket = AsyncFd<Arc<UdpSocket>>;

    #[inline]
    fn features(&self) -> TransportFeatures {
        TransportFeatures::UDP
    }

    #[inline]
    fn with_source_control_port(&mut self, port: u16) {
        self.0.set_port(port);
    }

    #[inline]
    fn setup(self, env: &Environment<Sub>) -> super::Result<super::SocketSet<Self::WorkerSocket>> {
        let mut options = env.socket_options.clone();
        let remote_addr = self.0;

        match remote_addr {
            SocketAddress::IpV6(_) if options.addr.is_ipv4() => {
                let addr: SocketAddress = options.addr.into();
                if addr.ip().is_unspecified() {
                    options.addr.set_ip(std::net::Ipv6Addr::UNSPECIFIED.into());
                } else {
                    let addr = addr.to_ipv6_mapped();
                    options.addr = addr.into();
                }
            }
            SocketAddress::IpV4(_) if options.addr.is_ipv6() => {
                let addr: SocketAddress = options.addr.into();
                if addr.ip().is_unspecified() {
                    options.addr.set_ip(std::net::Ipv4Addr::UNSPECIFIED.into());
                } else {
                    let addr = addr.unmap();
                    // ensure the local IP maps to v4, otherwise it won't bind correctly
                    ensure!(
                        matches!(addr, SocketAddress::IpV4(_)),
                        Err(io::ErrorKind::Unsupported.into())
                    );
                    options.addr = addr.into();
                }
            }
            _ => {}
        }

        let socket::Pair { writer, reader } = socket::Pair::open(options)?;

        let writer = Arc::new(writer);
        let reader = Arc::new(reader);

        let read_worker = {
            let _guard = env.reader_rt.enter();
            AsyncFd::new(reader.clone())?
        };

        let write_worker = {
            let _guard = env.writer_rt.enter();
            AsyncFd::new(writer.clone())?
        };

        // if we're on a platform that requires two different ports then we need to create
        // a socket for the writer as well
        let multi_port = read_worker.local_port()? != write_worker.local_port()?;

        let source_control_port = write_worker.local_port()?;

        // if the reader port is different from the writer then tell the peer
        let source_stream_port = if multi_port {
            Some(read_worker.local_port()?)
        } else {
            None
        };

        let application: Box<dyn socket::application::Builder> = if multi_port {
            Box::new(socket::application::builder::UdpPair { reader, writer })
        } else {
            Box::new(reader)
        };

        let read_worker = Some(read_worker);
        let write_worker = Some(write_worker);

        Ok(super::SocketSet {
            application,
            read_worker,
            write_worker,
            remote_addr,
            source_control_port,
            source_stream_port,
        })
    }
}

/// A socket that is already registered with the application runtime
pub struct TcpRegistered(pub TcpStream);

impl<Sub> super::Peer<Environment<Sub>> for TcpRegistered
where
    Sub: event::Subscriber,
{
    type WorkerSocket = TcpStream;

    fn features(&self) -> TransportFeatures {
        TransportFeatures::TCP
    }

    #[inline]
    fn with_source_control_port(&mut self, port: u16) {
        let _ = port;
    }

    #[inline]
    fn setup(self, _env: &Environment<Sub>) -> super::Result<super::SocketSet<Self::WorkerSocket>> {
        let remote_addr = self.0.peer_addr()?.into();
        let source_control_port = self.0.local_addr()?.port();
        let application = Box::new(self.0);
        Ok(super::SocketSet {
            application,
            read_worker: None,
            write_worker: None,
            remote_addr,
            source_control_port,
            source_stream_port: None,
        })
    }
}

/// A socket that should be reregistered with the application runtime
pub struct TcpReregistered(pub TcpStream, pub SocketAddress);

impl<Sub> super::Peer<Environment<Sub>> for TcpReregistered
where
    Sub: event::Subscriber,
{
    type WorkerSocket = TcpStream;

    fn features(&self) -> TransportFeatures {
        TransportFeatures::TCP
    }

    #[inline]
    fn with_source_control_port(&mut self, port: u16) {
        let _ = port;
    }

    #[inline]
    fn setup(self, _env: &Environment<Sub>) -> super::Result<super::SocketSet<Self::WorkerSocket>> {
        let remote_addr = self.1;
        let source_control_port = self.0.local_addr()?.port();
        let application = Box::new(self.0.into_std()?);
        Ok(super::SocketSet {
            application,
            read_worker: None,
            write_worker: None,
            remote_addr,
            source_control_port,
            source_stream_port: None,
        })
    }
}
