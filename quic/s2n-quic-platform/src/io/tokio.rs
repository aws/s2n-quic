// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::select::{self, Select};
use crate::{buffer::default as buffer, features::gso, socket::default as socket, syscall};
use cfg_if::cfg_if;
use s2n_quic_core::{
    endpoint::Endpoint,
    event::{self, EndpointPublisher as _},
    inet::{self, SocketAddress},
    path::MaxMtu,
    time::{
        clock::{ClockWithTimer as _, Timer as _},
        Clock as ClockTrait,
    },
};
use std::{convert::TryInto, io, io::ErrorKind};
use tokio::{net::UdpSocket, runtime::Handle};

pub type PathHandle = socket::Handle;

mod clock;
pub(crate) use clock::Clock;

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
            mut max_mtu,
            max_segments,
            reuse_port,
        } = self.builder;

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
            syscall::bind_udp(recv_addr, reuse_port)?
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
            syscall::bind_udp(send_addr, reuse_port)?
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

        // Configure MTU discovery
        if !syscall::configure_mtu_disc(&tx_socket) {
            // disable MTU probing if we can't prevent fragmentation
            max_mtu = MaxMtu::MIN;
        }

        publisher.on_platform_feature_configured(event::builder::PlatformFeatureConfigured {
            configuration: event::builder::PlatformFeatureConfiguration::MaxMtu {
                mtu: max_mtu.into(),
            },
        });

        // Configure packet info CMSG
        syscall::configure_pktinfo(&rx_socket);

        // Configure TOS/ECN
        let tos_enabled = syscall::configure_tos(&rx_socket);

        publisher.on_platform_feature_configured(event::builder::PlatformFeatureConfigured {
            configuration: event::builder::PlatformFeatureConfiguration::Ecn {
                enabled: tos_enabled,
            },
        });

        let rx_buffer = buffer::Buffer::new_with_mtu(max_mtu.into());
        let tx_buffer = buffer::Buffer::new_with_mtu(max_mtu.into());
        cfg_if! {
            if #[cfg(any(s2n_quic_platform_socket_msg, s2n_quic_platform_socket_mmsg))] {
                let mut rx = socket::Queue::<buffer::Buffer>::new(rx_buffer, max_segments.into());
                let tx = socket::Queue::<buffer::Buffer>::new(tx_buffer, max_segments.into());
            } else {
                // If you are using an LSP to jump into this code, it will
                // probably take you to the wrong implementation. socket.rs does
                // compile time swaps of socket implementations. This queue is
                // actually in socket/std.rs, not socket/mmsg.rs
                let mut rx = socket::Queue::new(rx_buffer);
                let tx = socket::Queue::new(tx_buffer);
            }
        }

        // tell the queue the local address so it can fill it in on each message
        rx.set_local_address({
            let addr: inet::SocketAddress = rx_addr.into();
            addr.into()
        });

        // Notify the endpoint of the MTU that we chose
        endpoint.set_max_mtu(max_mtu);

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
                let debug = format!("A fatal IO error occurred ({:?}): {err}", err.kind());
                if cfg!(test) {
                    panic!("{debug}");
                } else {
                    eprintln!("{debug}");
                }
            }
        });

        drop(guard);

        Ok((task, local_addr))
    }
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
            .map_err(|err| io::Error::new(ErrorKind::InvalidInput, format!("{err}")))?;
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

            let timer_ready = timer.ready();

            let select::Outcome {
                rx_result,
                tx_result,
                timeout_expired,
                application_wakeup,
            } = if let Ok(res) = Select::new(rx_task, tx_task, &mut wakeups, timer_ready).await {
                res
            } else {
                // The endpoint has shut down
                return Ok(());
            };

            let wakeup_timestamp = clock.get_time();
            let subscriber = endpoint.subscriber();
            let mut publisher = event::EndpointPublisherSubscriber::new(
                event::builder::EndpointMeta {
                    endpoint_type: E::ENDPOINT_TYPE,
                    timestamp: wakeup_timestamp,
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

            let timeout = endpoint.timeout();

            if let Some(timeout) = timeout {
                timer.update(timeout);
            }

            let timestamp = clock.get_time();
            let subscriber = endpoint.subscriber();
            let mut publisher = event::EndpointPublisherSubscriber::new(
                event::builder::EndpointMeta {
                    endpoint_type: E::ENDPOINT_TYPE,
                    timestamp,
                },
                None,
                subscriber,
            );

            // notify the application that we're going to sleep
            let timeout = timeout.map(|t| t.saturating_duration_since(timestamp));
            publisher.on_platform_event_loop_sleep(event::builder::PlatformEventLoopSleep {
                timeout,
                processing_duration: timestamp.saturating_duration_since(wakeup_timestamp),
            });
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
    use core::{
        convert::TryInto,
        task::{Context, Poll},
    };
    use s2n_quic_core::{
        endpoint::{self, CloseError},
        event,
        inet::SocketAddress,
        io::{rx, tx},
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

            queue.for_each(|_header, payload| {
                assert_eq!(payload.len(), 4, "invalid payload {:?}", payload);

                let id = (&*payload).try_into().unwrap();
                let id = u32::from_be_bytes(id);
                self.messages.remove(&id);
            });
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
        let rx_socket = syscall::bind_udp(receive_addr, false)?;
        let rx_socket: std::net::UdpSocket = rx_socket.into();
        let addr = rx_socket.local_addr()?;

        let mut io_builder = Io::builder().with_rx_socket(rx_socket)?;

        if let Some(addr) = send_addr {
            let tx_socket = syscall::bind_udp(addr, false)?;
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
