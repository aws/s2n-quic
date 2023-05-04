// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{buffer::default as buffer, io::tokio::Clock, socket::std as socket};
use s2n_quic_core::{
    endpoint::Endpoint,
    event::{self, EndpointPublisher as _},
    inet::SocketAddress,
    io::event_loop::select::{self, Select},
    path::MaxMtu,
    time::{
        clock::{ClockWithTimer as _, Timer as _},
        Clock as ClockTrait,
    },
};
use std::{convert::TryInto, io, io::ErrorKind};
use tokio::runtime::Handle;
use turmoil::net::UdpSocket;

impl crate::socket::std::Socket for UdpSocket {
    type Error = io::Error;

    fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, Option<SocketAddress>), Self::Error> {
        let (len, addr) = self.try_recv_from(buf)?;
        Ok((len, Some(addr.into())))
    }

    fn send_to(&self, buf: &[u8], addr: &SocketAddress) -> Result<usize, Self::Error> {
        let addr: std::net::SocketAddr = (*addr).into();
        self.try_send_to(buf, addr)
    }
}

pub type PathHandle = socket::Handle;

#[derive(Default)]
pub struct Io {
    builder: Builder,
}

impl Io {
    pub fn builder() -> Builder {
        Builder::default()
    }

    pub fn new<A: turmoil::ToSocketAddrs + Send + Sync + 'static>(addr: A) -> io::Result<Self> {
        let builder = Builder::default().with_address(addr)?;
        Ok(Self { builder })
    }

    async fn setup<E: Endpoint<PathHandle = PathHandle>>(
        self,
        mut endpoint: E,
    ) -> io::Result<(Instance<E>, SocketAddress)> {
        let Builder {
            handle: _,
            socket,
            addr,
            max_mtu,
        } = self.builder;

        endpoint.set_max_mtu(max_mtu);

        let clock = Clock::default();

        let socket = if let Some(socket) = socket {
            socket
        } else if let Some(addr) = addr {
            UdpSocket::bind(&*addr).await?
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing bind address",
            ));
        };

        let rx_buffer = buffer::Buffer::new_with_mtu(max_mtu.into());
        let mut rx = socket::Queue::new(rx_buffer);

        let tx_buffer = buffer::Buffer::new_with_mtu(max_mtu.into());
        let tx = socket::Queue::new(tx_buffer);

        let local_addr: SocketAddress = socket.local_addr()?.into();

        // tell the queue the local address so it can fill it in on each message
        rx.set_local_address(local_addr.into());

        let instance = Instance {
            clock,
            socket,
            rx,
            tx,
            endpoint,
        };

        Ok((instance, local_addr))
    }

    pub fn start<E: Endpoint<PathHandle = PathHandle>>(
        mut self,
        endpoint: E,
    ) -> io::Result<(tokio::task::JoinHandle<()>, SocketAddress)> {
        let handle = if let Some(handle) = self.builder.handle.take() {
            handle
        } else {
            Handle::try_current().map_err(|err| std::io::Error::new(io::ErrorKind::Other, err))?
        };

        let guard = handle.enter();

        let task = handle.spawn(async move {
            let (instance, _local_addr) = self.setup(endpoint).await.unwrap();

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

        // TODO this is a potentially async operation - can we get this here?
        let local_addr = Default::default();

        Ok((task, local_addr))
    }
}

#[derive(Default)]
pub struct Builder {
    handle: Option<Handle>,
    socket: Option<UdpSocket>,
    addr: Option<Box<dyn turmoil::ToSocketAddrs + Send + Sync + 'static>>,
    max_mtu: MaxMtu,
}

impl Builder {
    #[must_use]
    pub fn with_handle(mut self, handle: Handle) -> Self {
        self.handle = Some(handle);
        self
    }

    /// Sets the local address for the runtime to listen on.
    ///
    /// NOTE: this method is mutually exclusive with `with_socket`
    pub fn with_address<A: turmoil::ToSocketAddrs + Send + Sync + 'static>(
        mut self,
        addr: A,
    ) -> io::Result<Self> {
        debug_assert!(self.socket.is_none(), "socket has already been set");
        self.addr = Some(Box::new(addr));
        Ok(self)
    }

    /// Sets the socket used for sending and receiving for the runtime.
    ///
    /// NOTE: this method is mutually exclusive with `with_address`
    pub fn with_socket(mut self, socket: UdpSocket) -> io::Result<Self> {
        debug_assert!(self.addr.is_none(), "address has already been set");
        self.socket = Some(socket);
        Ok(self)
    }

    /// Sets the largest maximum transmission unit (MTU) that can be sent on a path
    pub fn with_max_mtu(mut self, max_mtu: u16) -> io::Result<Self> {
        self.max_mtu = max_mtu
            .try_into()
            .map_err(|err| io::Error::new(ErrorKind::InvalidInput, format!("{err}")))?;
        Ok(self)
    }

    pub fn build(self) -> io::Result<Io> {
        Ok(Io { builder: self })
    }
}

struct Instance<E> {
    clock: Clock,
    socket: turmoil::net::UdpSocket,
    rx: socket::Queue<buffer::Buffer>,
    tx: socket::Queue<buffer::Buffer>,
    endpoint: E,
}

impl<E: Endpoint<PathHandle = PathHandle>> Instance<E> {
    async fn event_loop(self) -> io::Result<()> {
        let Self {
            clock,
            socket,
            mut rx,
            mut tx,
            mut endpoint,
        } = self;

        let mut timer = clock.timer();

        loop {
            // Poll for readability if we have free slots available
            let rx_interest = rx.free_len() > 0;
            let rx_task = async {
                if rx_interest {
                    socket.readable().await
                } else {
                    futures::future::pending().await
                }
            };

            // Poll for writablity if we have occupied slots available
            let tx_interest = tx.occupied_len() > 0;
            let tx_task = async {
                if tx_interest {
                    socket.writable().await
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

            if tx_result.is_some() {
                tx.tx(&socket, &mut publisher)?;
            }

            if rx_result.is_some() {
                rx.rx(&socket, &mut publisher)?;
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
        time::{timer::Provider as _, Clock, Duration, Timer, Timestamp},
    };
    use std::collections::BTreeMap;

    struct TestEndpoint {
        addr: SocketAddress,
        tx_message_id: u32,
        rx_messages: BTreeMap<u32, Timestamp>,
        total_messages: u32,
        subscriber: NoopSubscriber,
        close_timer: Timer,
    }

    impl TestEndpoint {
        fn new(addr: SocketAddress) -> Self {
            Self {
                addr,
                tx_message_id: 0,
                rx_messages: BTreeMap::new(),
                total_messages: 1000,
                subscriber: Default::default(),
                close_timer: Default::default(),
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
            _clock: &C,
        ) {
            while self.tx_message_id < self.total_messages {
                let payload = self.tx_message_id.to_be_bytes();
                let addr = PathHandle::from_remote_address(self.addr.into());
                let msg = (addr, payload);
                if queue.push(msg).is_ok() {
                    self.tx_message_id += 1;
                } else {
                    // no more capacity
                    return;
                }
            }
        }

        fn receive<Rx: rx::Queue<Handle = PathHandle>, C: Clock>(
            &mut self,
            queue: &mut Rx,
            clock: &C,
        ) {
            let now = clock.get_time();
            queue.for_each(|_header, payload| {
                assert_eq!(payload.len(), 4, "invalid payload {:?}", payload);

                let id = (&*payload).try_into().unwrap();
                let id = u32::from_be_bytes(id);
                self.rx_messages.insert(id, now);
            });
        }

        fn poll_wakeups<C: Clock>(
            &mut self,
            _cx: &mut Context<'_>,
            clock: &C,
        ) -> Poll<Result<usize, CloseError>> {
            let now = clock.get_time();

            if self.close_timer.poll_expiration(now).is_ready() {
                assert!(self.rx_messages.len() as u32 * 4 > self.total_messages);
                return Err(CloseError).into();
            }

            if !self.close_timer.is_armed()
                && self.total_messages <= self.tx_message_id
                && !self.rx_messages.is_empty()
            {
                self.close_timer.set(now + Duration::from_millis(100));
            }

            Poll::Pending
        }

        fn timeout(&self) -> Option<Timestamp> {
            self.close_timer.next_expiration()
        }

        fn set_max_mtu(&mut self, _max_mtu: MaxMtu) {
            // noop
        }

        fn subscriber(&mut self) -> &mut Self::Subscriber {
            &mut self.subscriber
        }
    }

    fn bind(port: u16) -> std::net::SocketAddr {
        use std::net::Ipv4Addr;
        (Ipv4Addr::UNSPECIFIED, port).into()
    }

    #[test]
    fn sim_test() -> io::Result<()> {
        use turmoil::lookup;

        let mut sim = turmoil::Builder::new().build();

        sim.client("client", async move {
            let io = Io::builder().with_address(bind(123))?.build()?;

            let endpoint = TestEndpoint::new((lookup("server"), 456).into());

            let (task, _) = io.start(endpoint)?;

            task.await?;

            Ok(())
        });

        sim.client("server", async move {
            let io = Io::builder().with_address(bind(456))?.build()?;

            let endpoint = TestEndpoint::new((lookup("client"), 123).into());

            let (task, _) = io.start(endpoint)?;

            task.await?;

            Ok(())
        });

        sim.run().unwrap();

        Ok(())
    }
}
