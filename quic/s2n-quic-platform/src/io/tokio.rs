// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{features::gso, message::default as message, socket, syscall};
use s2n_quic_core::{
    endpoint::Endpoint,
    event::{self, EndpointPublisher as _},
    inet::{self, SocketAddress},
    io::event_loop::EventLoop,
    path::MaxMtu,
    time::Clock as ClockTrait,
};
use std::{convert::TryInto, io, io::ErrorKind};
use tokio::runtime::Handle;

mod builder;
mod clock;
mod task;
#[cfg(test)]
mod tests;

pub type PathHandle = message::Handle;
pub use builder::Builder;
pub(crate) use clock::Clock;

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

        // try to use the tokio runtime handle if provided
        let handle = if let Some(handle) = handle {
            handle
        } else {
            Handle::try_current().map_err(|err| std::io::Error::new(io::ErrorKind::Other, err))?
        };

        let guard = handle.enter();

        let rx_socket = if let Some(rx_socket) = rx_socket {
            rx_socket
        } else if let Some(recv_addr) = recv_addr {
            syscall::bind_udp(recv_addr, reuse_port)?
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "missing bind address",
            ));
        };

        let rx_addr = convert_addr_to_std(rx_socket.local_addr()?)?;

        let tx_socket = if let Some(tx_socket) = tx_socket {
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

        // Configure the socket with GRO
        let gro_enabled = syscall::configure_gro(&rx_socket);

        publisher.on_platform_feature_configured(event::builder::PlatformFeatureConfigured {
            configuration: event::builder::PlatformFeatureConfiguration::Gro {
                enabled: gro_enabled,
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

        let rx = {
            // if GRO is enabled, then we need to provide the syscall with the maximum size buffer
            let payload_len = if gro_enabled {
                u16::MAX
            } else {
                max_mtu.into()
            } as u32;

            // 8Mb - TODO make this configurable: https://github.com/aws/s2n-quic/issues/1811
            let rx_buffer_size = 8 * (1 << 20);
            let entries = rx_buffer_size / payload_len;
            let entries = if entries.is_power_of_two() {
                entries
            } else {
                // round up to the nearest power of two, since the ring buffers require it
                entries.next_power_of_two()
            };

            let mut consumers = vec![];

            let (producer, consumer) = socket::ring::pair(entries, payload_len);
            consumers.push(consumer);

            // spawn a task that actually reads from the socket into the ring buffer
            handle.spawn(task::rx(rx_socket, producer));

            // construct the RX side for the endpoint event loop
            let max_mtu = MaxMtu::try_from(payload_len as u16).unwrap();
            let addr: inet::SocketAddress = rx_addr.into();
            socket::io::rx::Rx::new(consumers, max_mtu, addr.into())
        };

        let tx = {
            let gso = crate::features::Gso::from(max_segments);

            // compute the payload size for each message from the number of GSO segments we can
            // fill
            let payload_len = {
                let max_mtu: u16 = max_mtu.into();
                (max_mtu as u32 * gso.max_segments() as u32).min(u16::MAX as u32)
            };

            // 8Mb - TODO make this configurable: https://github.com/aws/s2n-quic/issues/1811
            let tx_buffer_size = 8 * (1 << 20);
            let entries = tx_buffer_size / payload_len;
            let entries = if entries.is_power_of_two() {
                entries
            } else {
                // round up to the nearest power of two, since the ring buffers require it
                entries.next_power_of_two()
            };

            let mut producers = vec![];

            let (producer, consumer) = socket::ring::pair(entries, payload_len);
            producers.push(producer);

            // spawn a task that actually flushes the ring buffer to the socket
            handle.spawn(task::tx(tx_socket, consumer, gso.clone()));

            // construct the TX side for the endpoint event loop
            socket::io::tx::Tx::new(producers, gso, max_mtu)
        };

        // Notify the endpoint of the MTU that we chose
        endpoint.set_max_mtu(max_mtu);

        let task = handle.spawn(
            EventLoop {
                endpoint,
                clock,
                rx,
                tx,
            }
            .start(),
        );

        drop(guard);

        Ok((task, rx_addr.into()))
    }
}

fn convert_addr_to_std(addr: socket2::SockAddr) -> io::Result<std::net::SocketAddr> {
    addr.as_socket()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid domain for socket"))
}
