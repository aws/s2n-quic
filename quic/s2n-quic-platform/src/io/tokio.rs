// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{buffer::default as buffer, socket::default as socket};
use cfg_if::cfg_if;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use futures::future::{Fuse, FutureExt};
use pin_project::pin_project;
use s2n_quic_core::{
    endpoint::{CloseError, Endpoint},
    inet::SocketAddress,
    path::MaxMtu,
    time::{self, Clock as ClockTrait},
};
#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;
use std::{convert::TryInto, io, io::ErrorKind};
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
        let builder = Builder::default().with_receive_address(address)?;
        Ok(Self { builder })
    }

    pub fn start<E: Endpoint>(self, mut endpoint: E) -> io::Result<tokio::task::JoinHandle<()>> {
        let Builder {
            handle,
            rx_socket,
            tx_socket,
            recv_addr,
            send_addr,
            recv_buffer_size,
            send_buffer_size,
            max_mtu,
        } = self.builder;

        endpoint.set_max_mtu(max_mtu);

        let handle = if let Some(handle) = handle {
            handle
        } else {
            Handle::try_current().map_err(|err| std::io::Error::new(io::ErrorKind::Other, err))?
        };

        let guard = handle.enter();

        let rx_socket = if let Some(rx_socket) = rx_socket {
            rx_socket
        } else if !recv_addr.is_empty() {
            bind(&recv_addr[..])?
        } else {
            bind(("::", 0))?
        };

        let tx_socket = if let Some(tx_socket) = tx_socket {
            tx_socket
        } else if !send_addr.is_empty() {
            bind(&send_addr[..])?
        } else {
            // No tx_socket or send address was specified, so the tx socket
            // will be a handle to the rx socket.
            rx_socket.try_clone()?
        };

        if let Some(size) = send_buffer_size {
            tx_socket.set_send_buffer_size(size)?;
        }

        if let Some(size) = recv_buffer_size {
            rx_socket.set_recv_buffer_size(size)?;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14
        //# UDP datagrams MUST NOT be fragmented at the IP layer.

        //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#14
        //# In IPv4 ([IPv4]), the DF bit MUST be set if possible, to prevent
        //# fragmentation on the path.

        //= https://tools.ietf.org/rfc/rfc8899.txt#3
        //# In IPv4, a probe packet MUST be sent with the Don't
        //# Fragment (DF) bit set in the IP header and without network layer
        //# endpoint fragmentation.

        //= https://tools.ietf.org/rfc/rfc8899.txt#4.5
        //# A PL implementing this specification MUST suspend network layer
        //# processing of outgoing packets that enforces a PMTU
        //# [RFC1191][RFC8201] for each flow utilizing DPLPMTUD and instead use
        //# DPLPMTUD to control the size of packets that are sent by a flow.
        #[cfg(target_os = "linux")]
        {
            // IP_PMTUDISC_PROBE setting will set the DF (Don't Fragment) flag
            // while also ignoring the Path MTU. This means packets will not
            // be fragmented, and the EMSGSIZE error will not be returned for
            // packets larger than the Path MTU according to the kernel.
            libc!(setsockopt(
                tx_socket.as_raw_fd(),
                libc::IPPROTO_IP,
                libc::IP_MTU_DISCOVER,
                &libc::IP_PMTUDISC_PROBE as *const _ as _,
                core::mem::size_of_val(&libc::IP_PMTUDISC_PROBE) as _,
            ))?;
            #[cfg(feature = "ipv6")]
            libc!(setsockopt(
                tx_socket.as_raw_fd(),
                libc::IPPROTO_IPV6,
                libc::IPV6_MTU_DISCOVER,
                &libc::IP_PMTUDISC_PROBE as *const _ as _,
                core::mem::size_of_val(&libc::IP_PMTUDISC_PROBE) as _,
            ))?;
        }

        let instance = Instance {
            clock: Clock::default(),
            rx_socket: rx_socket.into(),
            tx_socket: tx_socket.into(),
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
        Domain::IPV6
    } else {
        Domain::IPV4
    };
    let socket_type = Type::DGRAM;
    let protocol = Some(Protocol::UDP);

    cfg_if! {
        // Set non-blocking mode in a single syscall if supported
        if #[cfg(any(
            target_os = "android",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "fuchsia",
            target_os = "illumos",
            target_os = "linux",
            target_os = "netbsd",
            target_os = "openbsd"
        ))] {
            let socket_type = socket_type.nonblocking();
            let socket = Socket::new(domain, socket_type, protocol)?;
        } else {
            let socket = Socket::new(domain, socket_type, protocol)?;
            socket.set_nonblocking(true)?;
        }
    };

    #[cfg(feature = "ipv6")]
    socket.set_only_v6(false)?;

    socket.set_reuse_address(true)?;

    #[cfg(unix)]
    socket.set_reuse_port(true)?;

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
    rx_socket: Option<socket2::Socket>,
    tx_socket: Option<socket2::Socket>,
    recv_addr: Vec<std::net::SocketAddr>,
    send_addr: Vec<std::net::SocketAddr>,
    recv_buffer_size: Option<usize>,
    send_buffer_size: Option<usize>,
    max_mtu: MaxMtu,
}

impl Builder {
    pub fn with_handle(mut self, handle: Handle) -> Self {
        self.handle = Some(handle);
        self
    }

    /// Sets the local address for the runtime to listen on. If no send address
    /// or tx socket is specified, this address will also be used for transmitting from.
    ///
    /// NOTE: this method is mutually exclusive with `with_rx_socket`
    pub fn with_receive_address(mut self, addr: std::net::SocketAddr) -> io::Result<Self> {
        debug_assert!(self.rx_socket.is_none(), "rx socket has already been set");
        self.recv_addr.push(addr);
        Ok(self)
    }

    /// Sets the local address for the runtime to transmit from. If no send address
    /// or tx socket is specified, the receive_address will be used for transmitting.
    ///
    /// NOTE: this method is mutually exclusive with `with_tx_socket`
    pub fn with_send_address(mut self, addr: std::net::SocketAddr) -> io::Result<Self> {
        debug_assert!(self.tx_socket.is_none(), "tx socket has already been set");
        self.send_addr.push(addr);
        Ok(self)
    }

    /// Sets the socket used for receiving for the runtime. If no tx_socket or send address is
    /// specified, this socket will be used for transmitting.
    ///
    /// NOTE: this method is mutually exclusive with `with_receive_address`
    pub fn with_rx_socket(mut self, socket: std::net::UdpSocket) -> io::Result<Self> {
        debug_assert!(
            self.recv_addr.is_empty(),
            "recv address has already been set"
        );
        self.rx_socket = Some(socket.into());
        Ok(self)
    }

    /// Sets the socket used for transmitting on for the runtime. If no tx_socket or send address is
    /// specified, the rx_socket will be used for transmitting.
    ///
    /// NOTE: this method is mutually exclusive with `with_send_address`
    pub fn with_tx_socket(mut self, socket: std::net::UdpSocket) -> io::Result<Self> {
        debug_assert!(
            self.send_addr.is_empty(),
            "send address has already been set"
        );
        self.tx_socket = Some(socket.into());
        Ok(self)
    }

    /// Sets the size of the operating system’s send buffer associated with the tx socket
    pub fn with_send_buffer_size(mut self, send_buffer_size: usize) -> io::Result<Self> {
        self.send_buffer_size = Some(send_buffer_size);
        Ok(self)
    }

    /// Sets the size of the operating system’s receive buffer associated with the rx socket
    pub fn with_recv_buffer_size(mut self, recv_buffer_size: usize) -> io::Result<Self> {
        self.recv_buffer_size = Some(recv_buffer_size);
        Ok(self)
    }

    /// Sets the largest maximum transmission unit (MTU) that can be sent on a path
    pub fn with_max_mtu(mut self, max_mtu: u16) -> io::Result<Self> {
        self.max_mtu = max_mtu
            .try_into()
            .map_err(|err| io::Error::new(ErrorKind::InvalidInput, format!("{}", err)))?;
        Ok(self)
    }

    pub fn build(self) -> io::Result<Io> {
        Ok(Io { builder: self })
    }
}

#[derive(Debug)]
struct Instance<E> {
    clock: Clock,
    rx_socket: std::net::UdpSocket,
    tx_socket: std::net::UdpSocket,
    rx: socket::Queue<buffer::Buffer>,
    tx: socket::Queue<buffer::Buffer>,
    endpoint: E,
}

impl<E: Endpoint> Instance<E> {
    async fn event_loop(self) -> io::Result<()> {
        let Self {
            clock,
            rx_socket,
            tx_socket,
            mut rx,
            mut tx,
            mut endpoint,
        } = self;

        cfg_if! {
            if #[cfg(any(s2n_quic_platform_socket_msg, s2n_quic_platform_socket_mmsg))] {
                let rx_socket = tokio::io::unix::AsyncFd::new(rx_socket)?;
                let tx_socket = tokio::io::unix::AsyncFd::new(tx_socket)?;
            } else {
                let rx_socket = async_fd_shim::AsyncFd::new(rx_socket)?;
                let tx_socket = async_fd_shim::AsyncFd::new(tx_socket)?;
            }
        }

        /// Even if there is no progress to be made, wake up the task at least once a second
        const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);

        let mut prev_time = Instant::now() + DEFAULT_TIMEOUT;
        let sleep = tokio::time::sleep_until(prev_time);
        tokio::pin!(sleep);

        loop {
            // Poll for readability if we have free slots available
            let rx_interest = rx.free_len() > 0;
            let rx_task = async {
                if rx_interest {
                    rx_socket.readable().await
                } else {
                    futures::future::pending().await
                }
            };

            // Poll for writablity if we have occupied slots available
            let tx_interest = tx.occupied_len() > 0;
            let tx_task = async {
                if tx_interest {
                    tx_socket.writable().await
                } else {
                    futures::future::pending().await
                }
            };

            let wakeups = endpoint.wakeups(&clock);
            // pin the wakeups future so we don't have to move it into the Select future.
            tokio::pin!(wakeups);

            let select = Select::new(rx_task, tx_task, &mut wakeups, &mut sleep);

            let mut reset_clock = false;

            if let Ok((rx_result, tx_result, timeout_result)) = select.await {
                if let Some(guard) = rx_result {
                    if let Ok(result) = guard?.try_io(|socket| rx.rx(socket)) {
                        result?;
                    }
                    endpoint.receive(&mut rx.rx_queue(), &clock);
                }

                if let Some(guard) = tx_result {
                    if let Ok(result) = guard?.try_io(|socket| tx.tx(socket)) {
                        result?;
                    }
                }

                // if the timer expired ensure it is reset to the default timeout
                if timeout_result {
                    reset_clock = true;
                }
            } else {
                // The endpoint has shut down
                return Ok(());
            }

            endpoint.transmit(&mut tx.tx_queue(), &clock);

            if let Some(delay) = endpoint.timeout() {
                let delay = unsafe {
                    // Safety: the same clock epoch is being used
                    delay.as_duration()
                };

                // floor the delay to milliseconds to reduce timer churn
                let delay = Duration::from_millis(delay.as_millis() as u64);

                // add the delay to the clock's epoch
                let next_time = clock.0 + delay;

                // if the clock has changed let the sleep future know
                if next_time != prev_time {
                    sleep.as_mut().reset(next_time);
                    prev_time = next_time;
                }
            } else if reset_clock {
                sleep.as_mut().reset(Instant::now() + DEFAULT_TIMEOUT);
            }
        }
    }
}

/// The main event loop future for selecting readiness of sub-tasks
///
/// This future ensures all sub-tasks are polled fairly by yielding once
/// after completing any of the sub-tasks. This is especially important when the TX queue is
/// flushed quickly and we never get notified of the RX socket having packets to read.
#[pin_project]
struct Select<Rx, Tx, Wakeup, Sleep>
where
    Rx: Future,
    Tx: Future,
    Wakeup: Future,
    Sleep: Future,
{
    #[pin]
    rx: Fuse<Rx>,
    rx_out: Option<Rx::Output>,
    #[pin]
    tx: Fuse<Tx>,
    tx_out: Option<Tx::Output>,
    #[pin]
    wakeup: Fuse<Wakeup>,
    #[pin]
    sleep: Sleep,
    is_ready: bool,
}

impl<Rx, Tx, Wakeup, Sleep> Select<Rx, Tx, Wakeup, Sleep>
where
    Rx: Future,
    Tx: Future,
    Wakeup: Future,
    Sleep: Future,
{
    #[inline(always)]
    fn new(rx: Rx, tx: Tx, wakeup: Wakeup, sleep: Sleep) -> Self {
        Self {
            rx: rx.fuse(),
            rx_out: None,
            tx: tx.fuse(),
            tx_out: None,
            wakeup: wakeup.fuse(),
            sleep,
            is_ready: false,
        }
    }
}

type SelectResult<Rx, Tx> = Result<(Option<Rx>, Option<Tx>, bool), CloseError>;

impl<Rx, Tx, Wakeup, Sleep> Future for Select<Rx, Tx, Wakeup, Sleep>
where
    Rx: Future,
    Tx: Future,
    Wakeup: Future<Output = Result<usize, CloseError>>,
    Sleep: Future,
{
    type Output = SelectResult<Rx::Output, Tx::Output>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        let mut should_wake = *this.is_ready;

        if let Poll::Ready(wakeup) = this.wakeup.poll(cx) {
            should_wake = true;
            if let Err(err) = wakeup {
                return Poll::Ready(Err(err));
            }
        }

        if let Poll::Ready(v) = this.rx.poll(cx) {
            should_wake = true;
            *this.rx_out = Some(v);
        }

        if let Poll::Ready(v) = this.tx.poll(cx) {
            should_wake = true;
            *this.tx_out = Some(v);
        }

        let mut timeout_result = false;

        if this.sleep.poll(cx).is_ready() {
            timeout_result = true;
            should_wake = true;
            // A ready from the sleep future should not yield, as it's unlikely that any of the
            // other tasks will yield on this loop.
            *this.is_ready = true;
        }

        // if none of the subtasks are ready, return
        if !should_wake {
            return Poll::Pending;
        }

        if core::mem::replace(this.is_ready, true) {
            Poll::Ready(Ok((this.rx_out.take(), this.tx_out.take(), timeout_result)))
        } else {
            // yield once so the other futures have the chance to wake up
            // before returning
            cx.waker().wake_by_ref();
            Poll::Pending
        }
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
            rx::{self, Entry as _},
            tx,
        },
        time::{Clock, Timestamp},
    };
    use std::{collections::BTreeMap, net::SocketAddr};

    struct TestEndpoint {
        addr: SocketAddress,
        messages: BTreeMap<u32, Option<Timestamp>>,
        now: Option<Timestamp>,
    }

    impl TestEndpoint {
        fn new(addr: SocketAddress) -> Self {
            let messages = (1000..2000).map(|id| (id, None)).collect();
            Self {
                addr,
                messages,
                now: None,
            }
        }
    }

    impl Endpoint for TestEndpoint {
        fn transmit<Tx: tx::Queue, C: Clock>(&mut self, queue: &mut Tx, clock: &C) {
            let now = clock.get_time();
            self.now = Some(now);

            for (id, tx_time) in &mut self.messages {
                match tx_time {
                    Some(time)
                        if now.saturating_duration_since(*time) < Duration::from_millis(50) =>
                    {
                        continue
                    }
                    _ => {
                        let payload1 = id.to_be_bytes();
                        let mut payload = [0; 1024];
                        payload[..4].copy_from_slice(&payload1[..]);
                        let msg = (self.addr, Vec::from(&payload[..]));
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

        fn receive<Rx: rx::Queue, C: Clock>(&mut self, queue: &mut Rx, clock: &C) {
            let now = clock.get_time();
            self.now = Some(now);
            let entries = queue.as_slice_mut();
            let len = entries.len();
            for entry in entries {
                let payload: &[u8] = entry.payload_mut();
                if payload.len() != 1024 {
                    panic!("invalid payload {:?}", payload);
                }
                let id = payload[..4].try_into().unwrap();
                let id = u32::from_be_bytes(id);
                self.messages.remove(&id);
            }
            queue.finish(len);
        }

        fn poll_wakeups<C: Clock>(
            &mut self,
            _cx: &mut Context<'_>,
            clock: &C,
        ) -> Poll<Result<usize, CloseError>> {
            let now = clock.get_time();
            self.now = Some(now);

            if self.messages.is_empty() {
                return Err(CloseError).into();
            }

            Poll::Pending
        }

        fn timeout(&self) -> Option<Timestamp> {
            self.now.map(|now| now + Duration::from_millis(50))
        }

        fn set_max_mtu(&mut self, _max_mtu: MaxMtu) {
            // noop
        }
    }

    async fn test<A: std::net::ToSocketAddrs>(
        receive_addr: A,
        send_addr: Option<A>,
    ) -> io::Result<()> {
        let rx_socket = bind(receive_addr)?;
        let rx_socket: std::net::UdpSocket = rx_socket.into();
        let mut addr = rx_socket.local_addr()?;

        if let std::net::IpAddr::V6(v6) = addr.ip() {
            if let Some(a) = v6.to_ipv4() {
                addr = SocketAddr::new(std::net::IpAddr::V4(a), addr.port());
            }
        }

        let mut io_builder = Io::builder().with_rx_socket(rx_socket)?;

        if let Some(addr) = send_addr {
            let tx_socket = bind(addr)?;
            let tx_socket: std::net::UdpSocket = tx_socket.into();
            io_builder = io_builder.with_tx_socket(tx_socket)?
        }

        let io = io_builder.build()?;

        let endpoint = TestEndpoint::new(addr.into());

        io.start(endpoint)?.await?;

        Ok(())
    }

    #[tokio::test]
    async fn ipv4_test() -> io::Result<()> {
        test("127.0.0.1:0", None).await
    }

    #[tokio::test]
    async fn ipv4_two_socket_test() -> io::Result<()> {
        test("127.0.0.1:0", Some("127.0.0.1:0")).await
    }

    #[cfg(feature = "ipv6")]
    #[tokio::test]
    async fn ipv6_test() -> io::Result<()> {
        match test(("::1", 0), None).await {
            Err(err) if err.kind() == io::ErrorKind::AddrNotAvailable => {
                eprintln!("The current environment does not support IPv6; skipping");
                Ok(())
            }
            other => other,
        }
    }

    #[cfg(feature = "ipv6")]
    #[tokio::test]
    async fn ipv6_two_socket_test() -> io::Result<()> {
        match test(("::1", 0), Some(("::1", 0))).await {
            Err(err) if err.kind() == io::ErrorKind::AddrNotAvailable => {
                eprintln!("The current environment does not support IPv6; skipping");
                Ok(())
            }
            other => other,
        }
    }
}
