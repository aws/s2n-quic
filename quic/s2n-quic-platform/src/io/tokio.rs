// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{buffer::default as buffer, features::gso, socket::default as socket};
use cfg_if::cfg_if;
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use futures::future::{Fuse, FutureExt};
use pin_project::pin_project;
use s2n_quic_core::{
    endpoint::{CloseError, Endpoint},
    event::{self, EndpointPublisher as _},
    inet::{self, SocketAddress},
    path::MaxMtu,
    time::Clock as ClockTrait,
};
use std::{convert::TryInto, io, io::ErrorKind};
use tokio::{net::UdpSocket, runtime::Handle};

pub type PathHandle = socket::Handle;

mod clock;
use clock::Clock;

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

    pub fn start<E: Endpoint<PathHandle = PathHandle>>(
        self,
        mut endpoint: E,
    ) -> io::Result<(tokio::task::JoinHandle<()>, SocketAddress)> {
        let Builder {
            handle,
            rx_socket,
            tx_socket,
            recv_addr,
            send_addr,
            recv_buffer_size,
            send_buffer_size,
            max_mtu,
            max_segments,
            reuse_port,
        } = self.builder;

        endpoint.set_max_mtu(max_mtu);

        let clock = Clock::default();

        let mut publisher = event::EndpointPublisherSubscriber::new(
            event::builder::EndpointMeta {
                endpoint_type: E::ENDPOINT_TYPE,
                timestamp: clock.get_time(),
            },
            None,
            endpoint.subscriber(),
        );

        publisher.on_platform_feature_configured(event::builder::PlatformFeatureConfigured {
            configuration: event::builder::PlatformFeatureConfiguration::MaxMtu {
                mtu: max_mtu.into(),
            },
        });

        publisher.on_platform_feature_configured(event::builder::PlatformFeatureConfigured {
            configuration: event::builder::PlatformFeatureConfiguration::Gso {
                max_segments: max_segments.into(),
            },
        });

        let handle = if let Some(handle) = handle {
            handle
        } else {
            Handle::try_current().map_err(|err| std::io::Error::new(io::ErrorKind::Other, err))?
        };

        let guard = handle.enter();

        let rx_socket = if let Some(rx_socket) = rx_socket {
            // ensure the socket is non-blocking
            rx_socket.set_nonblocking(true)?;
            rx_socket
        } else if let Some(recv_addr) = recv_addr {
            bind(recv_addr, reuse_port)?
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing bind address",
            ));
        };

        let tx_socket = if let Some(tx_socket) = tx_socket {
            // ensure the socket is non-blocking
            tx_socket.set_nonblocking(true)?;
            tx_socket
        } else if let Some(send_addr) = send_addr {
            bind(send_addr, reuse_port)?
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

        fn convert_addr_to_std(addr: socket2::SockAddr) -> io::Result<std::net::SocketAddr> {
            addr.as_socket().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "invalid domain for socket")
            })
        }

        #[allow(unused_variables)] // some platform builds won't use these so ignore warnings
        let (tx_addr, rx_addr) = (
            convert_addr_to_std(tx_socket.local_addr()?)?,
            convert_addr_to_std(rx_socket.local_addr()?)?,
        );

        //= https://www.rfc-editor.org/rfc/rfc9000#section-14
        //# UDP datagrams MUST NOT be fragmented at the IP layer.

        //= https://www.rfc-editor.org/rfc/rfc9000#section-14
        //# In IPv4 [IPv4], the Don't Fragment (DF) bit MUST be set if possible, to
        //# prevent fragmentation on the path.

        //= https://www.rfc-editor.org/rfc/rfc8899#section-3
        //# In IPv4, a probe packet MUST be sent with the Don't
        //# Fragment (DF) bit set in the IP header and without network layer
        //# endpoint fragmentation.

        //= https://www.rfc-editor.org/rfc/rfc8899#section-4.5
        //# A PL implementing this specification MUST suspend network layer
        //# processing of outgoing packets that enforces a PMTU
        //# [RFC1191][RFC8201] for each flow utilizing DPLPMTUD and instead use
        //# DPLPMTUD to control the size of packets that are sent by a flow.
        #[cfg(s2n_quic_platform_mtu_disc)]
        {
            use std::os::unix::io::AsRawFd;
            if tx_addr.is_ipv4() {
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
            } else {
                libc!(setsockopt(
                    tx_socket.as_raw_fd(),
                    libc::IPPROTO_IPV6,
                    libc::IPV6_MTU_DISCOVER,
                    &libc::IP_PMTUDISC_PROBE as *const _ as _,
                    core::mem::size_of_val(&libc::IP_PMTUDISC_PROBE) as _,
                ))?;
            }
        }

        // Set up the RX socket to pass ECN information
        #[cfg(s2n_quic_platform_tos)]
        {
            use std::os::unix::io::AsRawFd;
            let enabled: libc::c_int = 1;

            // This option needs to be enabled regardless of domain (IPv4 vs IPv6), except on mac
            if rx_addr.is_ipv4() || !cfg!(any(target_os = "macos", target_os = "ios")) {
                libc!(setsockopt(
                    rx_socket.as_raw_fd(),
                    libc::IPPROTO_IP,
                    libc::IP_RECVTOS,
                    &enabled as *const _ as _,
                    core::mem::size_of_val(&enabled) as _,
                ))?;
            }

            if rx_addr.is_ipv6() {
                libc!(setsockopt(
                    rx_socket.as_raw_fd(),
                    libc::IPPROTO_IPV6,
                    libc::IPV6_RECVTCLASS,
                    &enabled as *const _ as _,
                    core::mem::size_of_val(&enabled) as _,
                ))?;
            }
        }
        publisher.on_platform_feature_configured(event::builder::PlatformFeatureConfigured {
            configuration: event::builder::PlatformFeatureConfiguration::Ecn {
                enabled: cfg!(s2n_quic_platform_tos),
            },
        });

        // Set up the RX socket to pass information about the local address and interface
        #[cfg(s2n_quic_platform_pktinfo)]
        {
            use std::os::unix::io::AsRawFd;
            let enabled: libc::c_int = 1;

            if rx_addr.is_ipv4() {
                libc!(setsockopt(
                    rx_socket.as_raw_fd(),
                    libc::IPPROTO_IP,
                    libc::IP_PKTINFO,
                    &enabled as *const _ as _,
                    core::mem::size_of_val(&enabled) as _,
                ))?;
            } else {
                libc!(setsockopt(
                    rx_socket.as_raw_fd(),
                    libc::IPPROTO_IPV6,
                    libc::IPV6_RECVPKTINFO,
                    &enabled as *const _ as _,
                    core::mem::size_of_val(&enabled) as _,
                ))?;
            }
        }

        cfg_if! {
            if #[cfg(any(s2n_quic_platform_socket_msg, s2n_quic_platform_socket_mmsg))] {
                let mut rx = socket::Queue::<buffer::Buffer>::new(buffer::Buffer::default(), max_segments.into());
                let tx = socket::Queue::<buffer::Buffer>::new(buffer::Buffer::default(), max_segments.into());
            } else {
                let mut rx = socket::Queue::default();
                let tx = socket::Queue::default();
            }
        }

        // tell the queue the local address so it can fill it in on each message
        rx.set_local_address({
            let addr: inet::SocketAddress = rx_addr.into();
            addr.into()
        });

        let instance = Instance {
            clock,
            rx_socket: rx_socket.into(),
            tx_socket: tx_socket.into(),
            rx,
            tx,
            endpoint,
        };

        let local_addr = instance.rx_socket.local_addr()?.into();

        let task = handle.spawn(async move {
            if let Err(err) = instance.event_loop().await {
                let debug = format!("A fatal IO error occurred ({:?}): {}", err.kind(), err);
                if cfg!(test) {
                    panic!("{}", debug);
                } else {
                    eprintln!("{}", debug);
                }
            }
        });

        drop(guard);

        Ok((task, local_addr))
    }
}

fn bind<A: std::net::ToSocketAddrs>(addr: A, reuse_port: bool) -> io::Result<socket2::Socket> {
    use socket2::{Domain, Protocol, Socket, Type};

    let addr = addr.to_socket_addrs()?.next().ok_or_else(|| {
        std::io::Error::new(
            io::ErrorKind::InvalidInput,
            "the provided bind address was empty",
        )
    })?;

    let domain = Domain::for_address(addr);
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

    // allow ipv4 to also connect
    if addr.is_ipv6() {
        socket.set_only_v6(false)?;
    }

    socket.set_reuse_address(true)?;

    #[cfg(unix)]
    socket.set_reuse_port(reuse_port)?;

    // mark the variable as "used" regardless of platform support
    let _ = reuse_port;

    socket.bind(&addr.into())?;

    Ok(socket)
}

#[derive(Debug, Default)]
pub struct Builder {
    handle: Option<Handle>,
    rx_socket: Option<socket2::Socket>,
    tx_socket: Option<socket2::Socket>,
    recv_addr: Option<std::net::SocketAddr>,
    send_addr: Option<std::net::SocketAddr>,
    recv_buffer_size: Option<usize>,
    send_buffer_size: Option<usize>,
    max_mtu: MaxMtu,
    max_segments: gso::MaxSegments,
    reuse_port: bool,
}

impl Builder {
    #[must_use]
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
        self.recv_addr = Some(addr);
        Ok(self)
    }

    /// Sets the local address for the runtime to transmit from. If no send address
    /// or tx socket is specified, the receive_address will be used for transmitting.
    ///
    /// NOTE: this method is mutually exclusive with `with_tx_socket`
    pub fn with_send_address(mut self, addr: std::net::SocketAddr) -> io::Result<Self> {
        debug_assert!(self.tx_socket.is_none(), "tx socket has already been set");
        self.send_addr = Some(addr);
        Ok(self)
    }

    /// Sets the socket used for receiving for the runtime. If no tx_socket or send address is
    /// specified, this socket will be used for transmitting.
    ///
    /// NOTE: this method is mutually exclusive with `with_receive_address`
    pub fn with_rx_socket(mut self, socket: std::net::UdpSocket) -> io::Result<Self> {
        debug_assert!(
            self.recv_addr.is_none(),
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
            self.send_addr.is_none(),
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

    /// Disables Generic Segmentation Offload (GSO)
    ///
    /// By default, GSO will be used unless the platform does not support it or an attempt to use
    /// GSO fails. If it is known that GSO is not available, set this option to explicitly disable it.
    pub fn with_gso_disabled(mut self) -> io::Result<Self> {
        self.max_segments = 1.try_into().expect("1 is always a valid MaxSegments value");
        Ok(self)
    }

    /// Enables the port reuse (SO_REUSEPORT) socket option
    pub fn with_reuse_port(mut self) -> io::Result<Self> {
        if !cfg!(unix) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reuse_port is not supported on the current platform",
            ));
        }
        self.reuse_port = true;
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

impl<E: Endpoint<PathHandle = PathHandle>> Instance<E> {
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

        let mut timer = clock.timer();

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

            let (rx_result, tx_result, timeout_expired, application_wakeup) =
                if let Ok(res) = Select::new(rx_task, tx_task, &mut wakeups, &mut timer).await {
                    res
                } else {
                    // The endpoint has shut down
                    return Ok(());
                };

            let subscriber = endpoint.subscriber();
            let mut publisher = event::EndpointPublisherSubscriber::new(
                event::builder::EndpointMeta {
                    endpoint_type: E::ENDPOINT_TYPE,
                    timestamp: clock.get_time(),
                },
                None,
                subscriber,
            );

            publisher.on_platform_event_loop_wakeup(event::builder::PlatformEventLoopWakeup {
                timeout_expired,
                rx_ready: rx_result.is_some(),
                tx_ready: tx_result.is_some(),
                application_wakeup,
            });

            if let Some(guard) = tx_result {
                if let Ok(result) = guard?.try_io(|socket| tx.tx(socket, &mut publisher)) {
                    result?;
                }
            }

            if let Some(guard) = rx_result {
                if let Ok(result) = guard?.try_io(|socket| rx.rx(socket, &mut publisher)) {
                    result?;
                }
                endpoint.receive(&mut rx.rx_queue(), &clock);
            }

            endpoint.transmit(&mut tx.tx_queue(), &clock);

            if let Some(delay) = endpoint.timeout() {
                timer.update(delay);
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
        }
    }
}

type SelectResult<Rx, Tx> = Result<(Option<Rx>, Option<Tx>, bool, bool), CloseError>;

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

        let mut should_wake = false;
        let mut application_wakeup = false;

        if let Poll::Ready(wakeup) = this.wakeup.poll(cx) {
            should_wake = true;
            application_wakeup = true;
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
        }

        // if none of the subtasks are ready, return
        if !should_wake {
            return Poll::Pending;
        }

        Poll::Ready(Ok((
            this.rx_out.take(),
            this.tx_out.take(),
            timeout_result,
            application_wakeup,
        )))
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
        endpoint::{self, CloseError},
        event,
        inet::SocketAddress,
        io::{
            rx::{self, Entry as _},
            tx,
        },
        path::Handle as _,
        time::{Clock, Duration, Timestamp},
    };
    use std::collections::BTreeMap;

    struct TestEndpoint {
        addr: SocketAddress,
        messages: BTreeMap<u32, Option<Timestamp>>,
        now: Option<Timestamp>,
        subscriber: NoopSubscriber,
    }

    impl TestEndpoint {
        fn new(addr: SocketAddress) -> Self {
            let messages = (0..1000).map(|id| (id, None)).collect();
            Self {
                addr,
                messages,
                now: None,
                subscriber: Default::default(),
            }
        }
    }

    #[derive(Debug, Default)]
    struct NoopSubscriber;

    impl event::Subscriber for NoopSubscriber {
        type ConnectionContext = ();

        fn create_connection_context(
            &mut self,
            _meta: &event::api::ConnectionMeta,
            _info: &event::api::ConnectionInfo,
        ) -> Self::ConnectionContext {
        }
    }

    impl Endpoint for TestEndpoint {
        type PathHandle = PathHandle;
        type Subscriber = NoopSubscriber;

        const ENDPOINT_TYPE: endpoint::Type = endpoint::Type::Server;

        fn transmit<Tx: tx::Queue<Handle = PathHandle>, C: Clock>(
            &mut self,
            queue: &mut Tx,
            clock: &C,
        ) {
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
                        let payload = id.to_be_bytes();
                        let addr = PathHandle::from_remote_address(self.addr.into());
                        let msg = (addr, payload);
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

        fn receive<Rx: rx::Queue<Handle = PathHandle>, C: Clock>(
            &mut self,
            queue: &mut Rx,
            clock: &C,
        ) {
            let now = clock.get_time();
            self.now = Some(now);
            let local_address = queue.local_address();
            let entries = queue.as_slice_mut();
            let len = entries.len();
            for entry in entries {
                if let Some((_header, payload)) = entry.read(&local_address) {
                    assert_eq!(payload.len(), 4, "invalid payload {:?}", payload);

                    let id = (&payload[..]).try_into().unwrap();
                    let id = u32::from_be_bytes(id);
                    self.messages.remove(&id);
                }
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

        fn subscriber(&mut self) -> &mut Self::Subscriber {
            &mut self.subscriber
        }
    }

    async fn test<A: std::net::ToSocketAddrs>(
        receive_addr: A,
        send_addr: Option<A>,
    ) -> io::Result<()> {
        let rx_socket = bind(receive_addr, false)?;
        let rx_socket: std::net::UdpSocket = rx_socket.into();
        let addr = rx_socket.local_addr()?;

        let mut io_builder = Io::builder().with_rx_socket(rx_socket)?;

        if let Some(addr) = send_addr {
            let tx_socket = bind(addr, false)?;
            let tx_socket: std::net::UdpSocket = tx_socket.into();
            io_builder = io_builder.with_tx_socket(tx_socket)?
        }

        let io = io_builder.build()?;

        let endpoint = TestEndpoint::new(addr.into());

        let (task, local_addr) = io.start(endpoint)?;

        let local_addr: std::net::SocketAddr = local_addr.into();

        assert_eq!(local_addr, addr);

        task.await?;

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
