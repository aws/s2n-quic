// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    path::secret::Map,
    stream::{
        environment::{Environment, Peer, SetupResult, SocketSet},
        recv::{
            buffer,
            dispatch::{Control, Stream},
            shared::RecvBuffer,
        },
        server::accept,
        socket, TransportFeatures,
    },
    sync::mpsc::Capacity,
};
use s2n_quic_core::inet::{IpAddress, IpV4Address, IpV6Address, SocketAddress, Unspecified};
use std::sync::Arc;

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Config {
    pub blocking: bool,
    pub reuse_port: bool,
    pub stream_queue: Capacity,
    pub control_queue: Capacity,
    pub max_packet_size: u16,
    pub packet_count: usize,
    pub accept_flavor: accept::Flavor,
    pub workers: Option<usize>,
    pub map: Map,
}

impl Config {
    pub fn new(map: Map) -> Self {
        Self {
            blocking: false,
            reuse_port: false,
            // TODO tune these defaults
            stream_queue: Capacity {
                max: 4096,
                initial: 256,
            },

            // set the control queue depth shallow, since we really only need the most recent ones
            control_queue: Capacity { max: 8, initial: 8 },

            // Allocate 1MB at a time
            max_packet_size: u16::MAX,
            packet_count: 16,

            accept_flavor: accept::Flavor::default(),

            workers: None,
            map,
        }
    }
}

#[derive(Debug)]
pub struct Pooled<S: socket::application::Application, W: socket::Socket> {
    pub peer_addr: SocketAddress,
    pub control: Control,
    pub stream: Stream,
    pub application_socket: Arc<S>,
    pub worker_socket: Arc<W>,
}

impl<E, S, W> Peer<E> for Pooled<S, W>
where
    E: Environment,
    S: socket::application::Application + 'static,
    W: socket::Socket + 'static,
{
    type ReadWorkerSocket = Arc<W>;
    type WriteWorkerSocket = (Arc<W>, buffer::Channel<Control>);

    #[inline]
    fn features(&self) -> TransportFeatures {
        TransportFeatures::UDP
    }

    #[inline]
    fn setup(self, _env: &E) -> SetupResult<Self::ReadWorkerSocket, Self::WriteWorkerSocket> {
        let mut remote_addr = self.peer_addr;
        let control = self.control;
        let stream = self.stream;
        let queue_id = control.queue_id();

        let local_addr: SocketAddress = self.worker_socket.local_addr()?.into();
        let application = Box::new(self.application_socket);
        let read_worker = Some(self.worker_socket.clone());
        let write_worker = Some((self.worker_socket, buffer::Channel::new(control)));

        fn ipv6_loopback() -> IpV6Address {
            let mut octets = [0; 16];
            octets[15] = 1;
            IpV6Address::new(octets)
        }

        match (remote_addr.ip(), local_addr.ip()) {
            (IpAddress::Ipv4(v4), IpAddress::Ipv4(_)) if v4.is_unspecified() => {
                // if remote addr is unspecified then it needs to be localhost instead
                remote_addr = IpV4Address::new([127, 0, 0, 1])
                    .with_port(remote_addr.port())
                    .into();
            }
            (IpAddress::Ipv4(v4), IpAddress::Ipv6(_)) if v4.is_unspecified() => {
                // if v4 is unspecified then use v6 loopback
                remote_addr = ipv6_loopback().with_port(remote_addr.port()).into();
            }
            (IpAddress::Ipv6(v6), IpAddress::Ipv6(_)) if v6.is_unspecified() => {
                // if v6 is unspecified then use v6 loopback
                remote_addr = ipv6_loopback().with_port(remote_addr.port()).into();
            }
            (IpAddress::Ipv4(_), IpAddress::Ipv4(_)) => {}
            (IpAddress::Ipv4(v4), IpAddress::Ipv6(_)) => {
                // use an IPv6-mapped addr if we're listening on a V6 socket
                remote_addr = v4.to_ipv6_mapped().with_port(remote_addr.port()).into();
            }
            (IpAddress::Ipv6(_), IpAddress::Ipv4(_)) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "IPv6 not supported on a IPv4 socket",
                ))
            }
            (IpAddress::Ipv6(_), IpAddress::Ipv6(_)) => {}
        }

        let socket = SocketSet {
            application,
            read_worker,
            write_worker,
            remote_addr,
            source_queue_id: Some(queue_id),
        };

        let recv_buffer = RecvBuffer::B(buffer::Channel::new(stream));

        Ok((socket, recv_buffer))
    }
}
