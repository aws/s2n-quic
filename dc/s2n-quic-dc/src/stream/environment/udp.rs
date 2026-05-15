// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    busy_poll,
    credentials::Credentials,
    packet::{secret_control::flow_reset, Packet},
    path::secret::Map,
    socket::pool::{self, descriptor},
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
    sync::mpsc::{self, Capacity},
};
use s2n_codec::{DecoderBufferMut, DecoderParameterizedValueMut};
use s2n_quic_core::inet::{IpAddress, IpV4Address, IpV6Address, SocketAddress, Unspecified};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub enum Workers {
    BusyPoll(busy_poll::Pool),
    /// Use the environment to spawn workers
    Environment(Option<usize>),
}

impl Default for Workers {
    fn default() -> Self {
        Self::Environment(None)
    }
}

impl Workers {
    pub fn len(&self) -> Option<usize> {
        match self {
            Self::BusyPoll(pool) => Some(pool.len()),
            Self::Environment(count) => *count,
        }
    }

    pub(crate) fn set_default(&mut self, count: usize) {
        if matches!(self, Self::Environment(None)) {
            *self = Self::Environment(Some(count));
        }
    }
}

impl From<busy_poll::Pool> for Workers {
    fn from(value: busy_poll::Pool) -> Self {
        Self::BusyPoll(value)
    }
}

impl From<usize> for Workers {
    fn from(value: usize) -> Self {
        Self::Environment(Some(value))
    }
}

#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Config {
    pub reuse_port: bool,
    pub stream_recv_queue: Capacity,
    pub control_recv_queue: Capacity,
    pub accept_flavor: accept::Flavor,
    pub send_workers: Workers,
    pub recv_workers: Workers,
    pub map: Map,
    // Send worker configuration
    pub max_gigabits_per_second: f64,
    pub priority_levels: usize,
    pub flow_priority: Option<u8>,
}

impl Config {
    pub fn new(map: Map) -> Self {
        Self {
            reuse_port: false,
            // TODO tune these defaults
            stream_recv_queue: Capacity {
                max: 1 << 18,
                initial: 256,
            },

            // set the control queue depth shallow, since we really only need the most recent ones
            control_recv_queue: Capacity { max: 8, initial: 8 },

            accept_flavor: accept::Flavor::default(),

            send_workers: Workers::Environment(None),
            recv_workers: Workers::Environment(None),

            map,

            // Send worker defaults
            max_gigabits_per_second: 5.0,
            priority_levels: 1,
            flow_priority: None,
        }
    }

    pub(crate) fn rx_packet_pool(&self) -> pool::Pool {
        pool::Pool::new(u16::MAX)
    }

    pub(crate) fn socket_count(&self) -> usize {
        self.send_workers
            .len()
            .unwrap_or(1)
            .max(self.recv_workers.len().unwrap_or(1))
            .max(1)
    }

    pub(crate) fn tx_packet_pool(&self) -> pool::Pool {
        pool::Pool::new(crate::msg::segment::MAX_UDP_PAYLOAD)
    }

    pub fn unroutable_packets<S>(
        &self,
        socket: S,
    ) -> (
        mpsc::Sender<descriptor::Filled>,
        impl core::future::Future<Output = ()> + Send + Sync + 'static,
    )
    where
        S: Send + Sync + 'static + crate::stream::socket::Socket,
    {
        let (tx, rx) = mpsc::new::<descriptor::Filled>(u16::MAX as usize);
        let map = self.map.clone();
        let task = async move {
            let mut out_buffer = [0u8; 1500];

            tracing::debug!("unroutable_packets task started");

            while let Ok(mut descriptor) = rx.recv_front().await {
                tracing::debug!(remote_addr = %descriptor.remote_address().get(), "unroutable_packets: processing descriptor");
                let peer = descriptor.remote_address().get().into();
                let buffer = DecoderBufferMut::new(descriptor.payload_mut());
                let Ok((packet, _)) = Packet::decode_parameterized_mut(16, buffer) else {
                    tracing::debug!("unroutable_packets: failed to decode packet");
                    continue;
                };

                let params = match packet {
                    Packet::Stream(packet) => {
                        let credentials = *packet.credentials();
                        packet
                            .source_queue_id()
                            .map(|queue_id| (credentials, queue_id, flow_reset::Trigger::Stream))
                    }
                    Packet::Datagram(packet) => {
                        // datagrams are not routable
                        let _ = packet;
                        None
                    }
                    Packet::Control(packet) => {
                        let credentials = *packet.credentials();
                        packet
                            .source_queue_id()
                            .map(|queue_id| (credentials, queue_id, flow_reset::Trigger::Control))
                    }
                    Packet::FlowReset(packet) => {
                        // Don't reply to flow reset packets to avoid looping
                        let _ = packet;
                        None
                    }
                    Packet::StaleKey(packet) => {
                        let _ = map.handle_stale_key_packet(&packet, &peer);
                        None
                    }
                    Packet::ReplayDetected(packet) => {
                        let _ = map.handle_replay_detected_packet(&packet, &peer);
                        None
                    }
                    Packet::UnknownPathSecret(packet) => {
                        let _ = map.handle_unknown_path_secret_packet(&packet, &peer);
                        None
                    }
                };

                let Some((credentials, queue_id, trigger)) = params else {
                    tracing::debug!("unroutable_packets: no source_queue_id, skipping");
                    continue;
                };

                tracing::debug!(%credentials, %queue_id, "unroutable_packets: sending FlowReset");

                let packet = crate::packet::secret_control::FlowReset {
                    credentials,
                    wire_version: crate::packet::WireVersion::ZERO,
                    queue_id,
                    code: crate::stream::error::Kind::FLOW_RESET_CODE.into(),
                    trigger,
                };

                let Some(len) = map.sign_flow_reset_packet(&packet, &mut out_buffer) else {
                    tracing::debug!("unroutable_packets: sign_flow_reset_packet returned None");
                    continue;
                };

                let remote_addr = descriptor.remote_address();
                let ecn = Default::default();
                let buffer = &out_buffer[..len];
                let buffer = &[std::io::IoSlice::new(buffer)];
                let result = socket.try_send(remote_addr, ecn, buffer);
                tracing::debug!(%remote_addr, len, ?result, "unroutable_packets: try_send FlowReset");
            }
        };
        (tx, task)
    }
}

#[derive(Debug)]
pub struct Pooled<S: socket::application::Application, W: socket::Socket> {
    pub peer_addr: SocketAddress,
    pub control: Control,
    pub stream: Stream,
    pub application_socket: Arc<S>,
    pub worker_socket: Arc<W>,
    pub transmission_pool: pool::Pool,
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
    fn setup(
        self,
        _env: &E,
        _credentials: &Credentials,
    ) -> SetupResult<Self::ReadWorkerSocket, Self::WriteWorkerSocket> {
        let mut remote_addr = self.peer_addr;
        let control = self.control;
        let stream = self.stream;
        let queue_id = control.queue_id();
        debug_assert_eq!(queue_id, stream.queue_id());

        let local_addr: SocketAddress = self.worker_socket.local_addr()?.into();
        let application = Box::new(self.application_socket);
        let read_worker = Some(self.worker_socket.clone());
        let write_worker = Some((self.worker_socket, buffer::Channel::new(control)));

        #[inline]
        fn ipv6_loopback() -> IpV6Address {
            std::net::Ipv6Addr::LOCALHOST.into()
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
                return Err(std::io::Error::other("IPv6 not supported on a IPv4 socket"))
            }
            (IpAddress::Ipv6(_), IpAddress::Ipv6(_)) => {}
        }

        let socket = SocketSet {
            application,
            read_worker,
            write_worker,
            transmission_pool: self.transmission_pool,
            remote_addr,
            local_queue_id: Some(queue_id),
        };

        let recv_buffer = RecvBuffer::B(buffer::Channel::new(stream));

        Ok((socket, recv_buffer))
    }
}
