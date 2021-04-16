// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{buffer::default as buffer, socket::default as socket};
use cfg_if::cfg_if;
use core::{
    future::Future,
    task::{Context, Poll},
    time::Duration,
};
use s2n_quic_core::{
    endpoint::Endpoint,
    inet::SocketAddress,
    time::{self, Clock as ClockTrait},
};
use std::io;
use tokio::{net::UdpSocket, runtime::Handle, time::Instant};

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
    type Error = io::Error;

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

    pub fn new<A: std::net::ToSocketAddrs>(addr: A) -> io::Result<Self> {
        let address = addr.to_socket_addrs()?.next().expect("missing address");
        let builder = Builder::default().with_address(address)?;
        Ok(Self { builder })
    }

    pub fn start<E: Endpoint>(self, endpoint: E) -> io::Result<tokio::task::JoinHandle<()>> {
        let Builder {
            handle,
            socket,
            addr,
        } = self.builder;

        let handle = if let Some(handle) = handle {
            handle
        } else {
            Handle::try_current().map_err(|err| std::io::Error::new(io::ErrorKind::Other, err))?
        };

        let guard = handle.enter();

        let socket = if let Some(socket) = socket {
            socket
        } else if !addr.is_empty() {
            bind(&addr[..])?
        } else {
            bind(("::", 0))?
        }
        .into();

        let instance = Instance {
            clock: Clock::default(),
            socket,
            rx: Default::default(),
            tx: Default::default(),
            endpoint,
        };

        let task = handle.spawn(async move {
            if let Err(err) = instance.event_loop().await {
                eprintln!("A fatal IO error occurred ({:?}): {}", err.kind(), err);
            }
        });

        drop(guard);

        Ok(task)
    }
}

fn bind<A: std::net::ToSocketAddrs>(addr: A) -> io::Result<socket2::Socket> {
    use socket2::{Domain, Protocol, Socket, Type};

    let domain = if cfg!(feature = "ipv6") {
        Domain::ipv6()
    } else {
        Domain::ipv4()
    };
    let socket_type = Type::dgram();
    let protocol = Some(Protocol::udp());

    cfg_if! {
        // Set non-blocking mode in a single syscall if supported
        if #[cfg(any(
            target_os = "android",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "linux",
            target_os = "netbsd",
            target_os = "openbsd"
        ))] {
            let socket_type = socket_type.non_blocking();
            let socket = Socket::new(domain, socket_type, protocol)?;
        } else {
            let socket = Socket::new(domain, socket_type, protocol)?;
            socket.set_nonblocking(true)?;
        }
    };

    #[cfg(feature = "ipv6")]
    socket.set_only_v6(false)?;

    socket.set_reuse_address(true)?;

    for addr in addr.to_socket_addrs()? {
        let addr = if cfg!(feature = "ipv6") {
            use ::std::net::SocketAddr;

            let addr: SocketAddress = addr.into();
            let addr: SocketAddr = addr.to_ipv6_mapped().into();
            addr
        } else {
            addr
        }
        .into();

        socket.bind(&addr)?;
    }

    Ok(socket)
}

#[derive(Debug, Default)]
pub struct Builder {
    handle: Option<Handle>,
    socket: Option<socket2::Socket>,
    addr: Vec<std::net::SocketAddr>,
}

impl Builder {
    pub fn with_handle(mut self, handle: Handle) -> Self {
        self.handle = Some(handle);
        self
    }

    /// Sets the local address for the runtime
    ///
    /// NOTE: this method is mutually exclusive with `with_socket`
    pub fn with_address(mut self, addr: std::net::SocketAddr) -> io::Result<Self> {
        debug_assert!(self.socket.is_none(), "socket has already been set");
        self.addr.push(addr);
        Ok(self)
    }

    /// Sets the socket used for the runtime
    ///
    /// NOTE: this method is mutually exclusive with `with_address`
    pub fn with_socket(mut self, socket: std::net::UdpSocket) -> io::Result<Self> {
        debug_assert!(self.addr.is_empty(), "address has already been set");
        self.socket = Some(socket.into());
        Ok(self)
    }

    pub fn build(self) -> io::Result<Io> {
        Ok(Io { builder: self })
    }
}

#[derive(Debug)]
struct Instance<E> {
    clock: Clock,
    socket: std::net::UdpSocket,
    rx: socket::Queue<buffer::Buffer>,
    tx: socket::Queue<buffer::Buffer>,
    endpoint: E,
}

impl<E: Endpoint> Instance<E> {
    async fn event_loop(self) -> io::Result<()> {
        let Self {
            clock,
            socket,
            mut rx,
            mut tx,
            mut endpoint,
        } = self;

        cfg_if! {
            if #[cfg(any(s2n_quic_platform_socket_msg, s2n_quic_platform_socket_mmsg))] {
                let socket = tokio::io::unix::AsyncFd::new(socket)?;
            } else {
                let socket = async_fd_shim::AsyncFd::new(socket)?;
            }
        }

        /// Even if there is no progress to be made, wake up the task at least once a second
        const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);

        let sleep = tokio::time::sleep(DEFAULT_TIMEOUT);
        let mut prev_time = Instant::now() + DEFAULT_TIMEOUT;
        tokio::pin!(sleep);

        loop {
            // Poll for writablity if we have occupied slots available
            let tx_interest = tx.occupied_len() > 0;
            let tx_task = async {
                if tx_interest {
                    socket.writable().await
                } else {
                    Pending::new().await
                }
            };

            // Poll for readability if we have free slots available
            let rx_interest = rx.free_len() > 0;
            let rx_task = async {
                if rx_interest {
                    socket.readable().await
                } else {
                    Pending::new().await
                }
            };

            tokio::select! {
                guard = tx_task => {
                    if let Ok(result) = guard?.try_io(|socket| tx.tx(socket)) {
                        result?;
                    }
                }
                guard = rx_task => {
                    if let Ok(result) = guard?.try_io(|socket| rx.rx(socket)) {
                        result?;
                    }
                    endpoint.receive(&mut rx, clock.get_time());
                }
                result = endpoint.wakeups(clock.get_time()) => {
                    // The endpoint has shut down
                    if result.is_err() {
                        return Ok(())
                    }
                }
                _ = &mut sleep => {
                    // do nothing; timer expiration is handled by `transmit`
                }
            };

            endpoint.transmit(&mut tx, clock.get_time());

            if let Some(delay) = endpoint.timeout() {
                let next_time = clock.0 + unsafe { delay.as_duration() };
                if next_time != prev_time {
                    sleep.as_mut().reset(next_time);
                    prev_time = next_time;
                }
            };
        }
    }
}

#[derive(Debug)]
pub struct Pending<T>(core::marker::PhantomData<T>);

impl<T> Pending<T> {
    fn new() -> Self {
        Self(Default::default())
    }
}

impl<T> Future for Pending<T> {
    type Output = T;

    fn poll(self: core::pin::Pin<&mut Self>, _: &mut Context<'_>) -> Poll<T> {
        Poll::Pending
    }
}

/// A shim for the AsyncFd API
///
/// Tokio only provides the AsyncFd interface for unix platforms so for
/// other platforms that don't support the msg/mmsg APIs we use a small
/// shim to reduce the differences between each implementation.
mod async_fd_shim {
    #![allow(dead_code)]

    use super::*;

    pub struct AsyncFd(tokio::net::UdpSocket);

    impl AsyncFd {
        pub fn new(socket: std::net::UdpSocket) -> io::Result<Self> {
            let socket = tokio::net::UdpSocket::from_std(socket)?;
            Ok(Self(socket))
        }

        pub async fn readable(&self) -> io::Result<TryIo<'_>> {
            self.0.readable().await?;
            Ok(TryIo(&self.0))
        }

        pub async fn writable(&self) -> io::Result<TryIo<'_>> {
            self.0.writable().await?;
            Ok(TryIo(&self.0))
        }
    }

    pub struct TryIo<'a>(&'a tokio::net::UdpSocket);

    impl<'a> TryIo<'a> {
        pub fn try_io<R>(
            &mut self,
            f: impl FnOnce(&tokio::net::UdpSocket) -> io::Result<R>,
        ) -> Result<io::Result<R>, ()> {
            match f(self.0) {
                Ok(v) => Ok(Ok(v)),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => Err(()),
                Err(err) => Ok(Err(err)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::convert::TryInto;
    use s2n_quic_core::{
        endpoint::CloseError,
        inet::SocketAddress,
        io::{
            rx::{self, Entry as _, Queue as _},
            tx::{self, Queue as _},
        },
        time::Timestamp,
    };
    use std::collections::BTreeMap;

    struct TestEndpoint {
        addr: SocketAddress,
        messages: BTreeMap<u32, Option<Timestamp>>,
        now: Option<Timestamp>,
    }

    impl TestEndpoint {
        fn new(addr: SocketAddress) -> Self {
            let messages = (0..1000).map(|id| (id, None)).collect();
            Self {
                addr,
                messages,
                now: None,
            }
        }
    }

    impl Endpoint for TestEndpoint {
        fn transmit<'tx, Tx: tx::Tx<'tx>>(&mut self, tx: &'tx mut Tx, now: Timestamp) {
            self.now = Some(now);
            let mut queue = tx.queue();
            for (id, tx_time) in &mut self.messages {
                match tx_time {
                    Some(time)
                        if now.saturating_duration_since(*time) < Duration::from_millis(50) =>
                    {
                        continue
                    }
                    _ => {
                        let payload = id.to_be_bytes();
                        let msg = (self.addr, payload);
                        if queue.push(msg).is_ok() {
                            *tx_time = Some(now);
                        } else {
                            // no more capacity
                            return;
                        }
                    }
                }
            }
        }

        fn receive<'rx, Rx: rx::Rx<'rx>>(&mut self, rx: &'rx mut Rx, now: Timestamp) {
            self.now = Some(now);
            let mut queue = rx.queue();
            let entries = queue.as_slice_mut();
            let len = entries.len();
            for entry in entries {
                let payload: &[u8] = entry.payload_mut();
                let payload = payload.try_into().unwrap();
                let id = u32::from_be_bytes(payload);
                self.messages.remove(&id);
            }
            queue.finish(len);
        }

        fn poll_wakeups(
            &mut self,
            _cx: &mut Context<'_>,
            now: Timestamp,
        ) -> Poll<Result<usize, CloseError>> {
            self.now = Some(now);

            if self.messages.is_empty() {
                return Err(CloseError).into();
            }

            Poll::Pending
        }

        fn timeout(&self) -> Option<Timestamp> {
            self.now.map(|now| now + Duration::from_millis(50))
        }
    }

    async fn test<A: std::net::ToSocketAddrs>(addr: A) -> io::Result<()> {
        let socket = bind(addr)?;
        let socket: std::net::UdpSocket = socket.into();
        let addr = socket.local_addr()?;

        let io = Io::builder().with_socket(socket)?.build()?;

        let endpoint = TestEndpoint::new(addr.into());

        io.start(endpoint)?.await?;

        Ok(())
    }

    #[tokio::test]
    async fn ipv4_test() -> io::Result<()> {
        test("127.0.0.1:0").await
    }

    #[cfg(feature = "ipv6")]
    #[tokio::test]
    async fn ipv6_test() -> io::Result<()> {
        match test(("::1", 0)).await {
            Err(err) if err.kind() == io::ErrorKind::AddrNotAvailable => {
                eprintln!("The current environment does not support IPv6; skipping");
                Ok(())
            }
            other => other,
        }
    }
}
