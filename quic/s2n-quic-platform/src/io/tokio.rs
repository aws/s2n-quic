// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{buffer::default as buffer, socket::default as socket};
use core::time::Duration;
use s2n_quic_core::{
    endpoint::Endpoint,
    inet::SocketAddress,
    io::{rx, tx},
    time::{self, Clock as ClockTrait},
};
use tokio::{io::Interest, net::UdpSocket, runtime::Handle, time::Instant};

#[derive(Debug)]
struct Clock(Instant);

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock {
    pub fn new() -> Self {
        Self(Instant::now())
    }
}

impl ClockTrait for Clock {
    fn get_time(&self) -> time::Timestamp {
        let duration = self.0.elapsed();
        unsafe {
            // Safety: time duration is only derived from a single `Instant`
            time::Timestamp::from_duration(duration)
        }
    }
}

impl crate::socket::std::Socket for UdpSocket {
    type Error = std::io::Error;

    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, Option<SocketAddress>), Self::Error> {
        let (len, addr) = self.try_recv_from(buf)?;
        Ok((len, Some(addr.into())))
    }

    fn send_to(&self, buf: &[u8], addr: &SocketAddress) -> Result<usize, Self::Error> {
        self.try_send_to(buf, (*addr).into())
    }
}

#[derive(Debug, Default)]
pub struct Io {
    builder: Builder,
}

impl Io {
    pub fn builder() -> Builder {
        Builder::default()
    }

    pub fn new<A: std::net::ToSocketAddrs>(addr: A) -> std::io::Result<Self> {
        let address = addr.to_socket_addrs()?.next().expect("missing address");
        let builder = Builder::default().with_address(address)?;
        Ok(Self { builder })
    }

    pub fn start<E: Endpoint>(self, endpoint: E) -> std::io::Result<()> {
        let Builder {
            handle,
            socket,
            addr,
        } = self.builder;

        let handle = if let Some(handle) = handle {
            handle
        } else {
            Handle::try_current()
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?
        };

        let guard = handle.enter();

        let socket = if let Some(socket) = socket {
            socket
        } else if let Some(addr) = addr {
            std::net::UdpSocket::bind(addr)?
        } else {
            std::net::UdpSocket::bind(("::", 0))?
        };

        let socket = UdpSocket::from_std(socket)?;

        let instance = Instance {
            clock: Clock::default(),
            socket,
            rx: Default::default(),
            tx: Default::default(),
            endpoint,
        };

        handle.spawn(async move {
            if let Err(err) = instance.event_loop().await {
                eprintln!("A fatal IO error occurred: #{}", err);
            }
        });

        drop(guard);

        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct Builder {
    handle: Option<Handle>,
    socket: Option<std::net::UdpSocket>,
    addr: Option<std::net::SocketAddr>,
}

impl Builder {
    pub fn with_handle(mut self, handle: Handle) -> Self {
        self.handle = Some(handle);
        self
    }

    pub fn with_address(mut self, addr: std::net::SocketAddr) -> std::io::Result<Self> {
        debug_assert!(self.socket.is_none(), "socket has already been set");
        self.addr = Some(addr);
        Ok(self)
    }

    pub fn with_socket(mut self, socket: std::net::UdpSocket) -> std::io::Result<Self> {
        debug_assert!(self.addr.is_none(), "address has already been set");
        self.socket = Some(socket);
        Ok(self)
    }

    pub fn build(self) -> std::io::Result<Io> {
        Ok(Io { builder: self })
    }
}

#[derive(Debug)]
struct Instance<E> {
    clock: Clock,
    socket: UdpSocket,
    rx: socket::Queue<buffer::Buffer>,
    tx: socket::Queue<buffer::Buffer>,
    endpoint: E,
}

impl<E: Endpoint> Instance<E> {
    async fn event_loop(self) -> std::io::Result<()> {
        let Self {
            clock,
            socket,
            mut rx,
            mut tx,
            mut endpoint,
        } = self;

        /// Even if there is no progress to be made, wake up the task at least once a second
        const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);

        let sleep = tokio::time::sleep(DEFAULT_TIMEOUT);
        tokio::pin!(sleep);

        loop {
            let mut interests = Interest::READABLE;

            // express write interest if we have at least 1 message in the queue
            if !tx::Tx::is_empty(&tx) {
                interests |= Interest::WRITABLE;
            }

            tokio::select! {
                ready = socket.ready(interests) => {
                    let ready = ready?;

                    if ready.is_writable() {
                        tx.tx(&socket)?;
                    }

                    if ready.is_readable() {
                        rx.rx(&socket)?;

                        if !rx::Rx::is_empty(&rx) {
                            endpoint.receive(&mut rx, clock.get_time());
                        }
                    }
                }
                _ = endpoint.wakeups(clock.get_time()) => {
                    // do nothing; the wakeups are handled inside the endpoint
                }
                _ = &mut sleep => {
                    // do nothing; timer expiration is handled by `transmit`
                }
            };

            endpoint.transmit(&mut tx, clock.get_time());

            let next_time = if let Some(delay) = endpoint.timeout() {
                clock.0 + unsafe { delay.as_duration() }
            } else {
                Instant::now() + DEFAULT_TIMEOUT
            };

            sleep.as_mut().reset(next_time);
        }
    }
}
