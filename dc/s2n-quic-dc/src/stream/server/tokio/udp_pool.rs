// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::accept;
use crate::{
    credentials::Credentials,
    event::{self, EndpointPublisher, IntoEvent, Subscriber},
    msg,
    path::secret,
    socket::recv::{descriptor, router::Router},
    stream::{
        endpoint,
        environment::{
            tokio::{self as env, Environment},
            udp, Environment as _,
        },
        recv::{
            self,
            dispatch::{Allocator as Queues, Dispatch},
        },
        server::{self, InitialPacket},
        socket::{Ext as _, Socket},
    },
    sync::mpsc,
};
use core::ops::ControlFlow;
use s2n_quic_core::{inet::SocketAddress, time::Clock, varint::VarInt};
use schnellru::LruMap;
use std::io;
use tracing::debug;

type Handshake = LruMap<CredentialsHashable, VarInt>;

#[derive(Debug, PartialEq, Eq)]
struct CredentialsHashable(Credentials);

impl core::hash::Hash for CredentialsHashable {
    #[inline]
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        let [a, b, c, d, e, f, g, h] = self.0.id.to_hash().to_le_bytes();
        let [i, j, k, l, m, n, o, p] = self.0.key_id.as_u64().to_le_bytes();
        state.write(&[a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p]);
    }
}

pub struct Acceptor<S, Sub>
where
    S: Socket,
    Sub: Subscriber + Clone,
{
    sender: accept::Sender<Sub>,
    socket: S,
    packets: mpsc::Receiver<descriptor::Filled>,
    handshake: Handshake,
    dispatch: Dispatch,
    queues: Queues,
    env: Environment<Sub>,
    secrets: secret::Map,
    accept_flavor: accept::Flavor,
    subscriber: Sub,
}

impl<S, Sub> Acceptor<S, Sub>
where
    S: Socket,
    Sub: Subscriber + Clone,
{
    #[inline]
    pub fn new(
        id: usize,
        socket: S,
        packets: mpsc::Receiver<descriptor::Filled>,
        sender: &accept::Sender<Sub>,
        env: &Environment<Sub>,
        secrets: &secret::Map,
        accept_flavor: accept::Flavor,
        subscriber: Sub,
    ) -> Self {
        let acceptor = Self {
            sender: sender.clone(),
            socket,
            packets,
            handshake: Default::default(),
            env: env.clone(),
            secrets: secrets.clone(),
            accept_flavor,
            subscriber,
        };

        if let Ok(addr) = acceptor.socket.local_addr() {
            let addr: SocketAddress = addr.into();
            let local_address = addr.into_event();
            acceptor
                .publisher()
                .on_acceptor_udp_started(event::builder::AcceptorUdpStarted { id, local_address });
        }

        acceptor
    }

    pub async fn run(mut self) {
        loop {
            match self.accept_one().await {
                Ok(ControlFlow::Continue(())) => continue,
                Ok(ControlFlow::Break(())) => break,
                Err(error) => {
                    self.publisher()
                        .on_acceptor_udp_io_error(event::builder::AcceptorUdpIoError {
                            error: &error,
                        });
                }
            }
        }
    }

    async fn accept_one(&mut self) -> io::Result<ControlFlow<()>> {
        let packet = self.recv_packet().await?;

        let now = self.env.clock().get_time();
        let publisher = publisher(&self.subscriber, &now);

        let server::handshake::Outcome::Created {
            receiver: handshake,
        } = self.handshake.handle(&packet, &mut self.recv_buffer)
        else {
            return Ok(ControlFlow::Continue(()));
        };

        let remote_addr = self.recv_buffer.remote_address();

        let meta = event::api::ConnectionMeta {
            id: 0, // TODO use an actual connection ID
            timestamp: now.into_event(),
        };
        let info = event::api::ConnectionInfo {};

        let subscriber_ctx = self.subscriber.create_connection_context(&meta, &info);

        // TODO allocate a queue for this stream
        let queue_id = VarInt::ZERO;
        let recv_buffer = recv::buffer::Local::new(self.recv_buffer.take(), Some(handshake));
        let recv_buffer = recv::buffer::Either::A(recv_buffer);

        let peer = env::UdpOwned(remote_addr, recv_buffer);

        let stream = match endpoint::accept_stream(
            now,
            &self.env,
            peer,
            &packet,
            queue_id,
            &self.secrets,
            self.subscriber.clone(),
            subscriber_ctx,
            None,
        ) {
            Ok(stream) => stream,
            Err(error) => {
                tracing::trace!("send_start");

                let addr = msg::addr::Addr::new(remote_addr);
                let ecn = Default::default();
                let buffer = &[io::IoSlice::new(&error.secret_control)];

                // ignore any errors since this is just for responding to invalid connect attempts
                let _ = self.socket.try_send(&addr, ecn, buffer);

                tracing::trace!("send_finish");
                return Err(error.error);
            }
        };

        {
            let remote_address: SocketAddress = stream.shared.read_remote_addr();
            let remote_address = &remote_address;
            let credential_id = &*stream.shared.credentials().id;
            let stream_id = stream.shared.application().stream_id.into_varint().as_u64();
            publisher.on_acceptor_udp_stream_enqueued(event::builder::AcceptorUdpStreamEnqueued {
                remote_address,
                credential_id,
                stream_id,
            });
        }

        let res = match self.accept_flavor {
            accept::Flavor::Fifo => self.sender.send_back(stream),
            accept::Flavor::Lifo => self.sender.send_front(stream),
        };

        match res {
            Ok(prev) => {
                if let Some(stream) = prev {
                    stream.prune(
                        event::builder::AcceptorStreamPruneReason::AcceptQueueCapacityExceeded,
                    );
                }

                Ok(ControlFlow::Continue(()))
            }
            Err(_) => {
                debug!("application accept queue dropped; shutting down");
                Ok(ControlFlow::Break(()))
            }
        }
    }

    async fn recv_packet(&mut self) -> io::Result<server::InitialPacket> {
        loop {
            // discard any pending packets
            self.recv_buffer.clear();
            self.socket.recv_buffer(&mut self.recv_buffer).await?;

            let remote_address = self.recv_buffer.remote_address();
            let remote_address = &remote_address;
            let packet = server::InitialPacket::peek(&mut self.recv_buffer, 16);

            let publisher = self.publisher();
            publisher.on_acceptor_udp_datagram_received(
                event::builder::AcceptorUdpDatagramReceived {
                    remote_address,
                    len: self.recv_buffer.payload_len(),
                },
            );

            match packet {
                Ok(packet) => {
                    publisher.on_acceptor_udp_packet_received(
                        event::builder::AcceptorUdpPacketReceived {
                            remote_address,
                            credential_id: &*packet.credentials.id,
                            stream_id: packet.stream_id.into_varint().as_u64(),
                            payload_len: packet.payload_len,
                            is_zero_offset: packet.is_zero_offset,
                            is_retransmission: packet.is_retransmission,
                            is_fin: packet.is_fin,
                            is_fin_known: packet.is_fin_known,
                        },
                    );

                    return Ok(packet);
                }
                Err(error) => {
                    publisher.on_acceptor_udp_packet_dropped(
                        event::builder::AcceptorUdpPacketDropped {
                            remote_address,
                            reason: error.into_event(),
                        },
                    );

                    continue;
                }
            }
        }
    }

    fn publisher(&self) -> event::EndpointPublisherSubscriber<Sub> {
        publisher(&self.subscriber, self.env.clock())
    }
}

impl<S, Sub> Router for Acceptor<S, Sub>
where
    S: Socket,
    Sub: Subscriber + Clone,
{
    #[inline]
    fn is_open(&self) -> bool {
        true
    }

    #[inline]
    fn dispatch_stream_packet(
        &mut self,
        tag: crate::packet::stream::Tag,
        id: crate::packet::stream::Id,
        credentials: crate::credentials::Credentials,
        segment: descriptor::Filled,
    ) {
        let packet = InitialPacket {
            credentials,
            stream_id: id,
            source_control_port: todo!(),
            source_stream_port: todo!(),
            payload_len: todo!(),
            is_zero_offset: todo!(),
            is_retransmission: todo!(),
            is_fin: todo!(),
            is_fin_known: todo!(),
        };

        let now = self.env.clock().get_time();

        let credentials = CredentialsHashable(credentials);
        if let Some(queue_id) = self.handshake.get(&credentials) {
            if self.dispatch.send_stream(*queue_id, segment).is_err() {
                // if the dispatch didn't work then remove it from the LRU
                let _ = self.handshake.remove(&credentials);
            }
            return;
        }

        let (control, stream) = self.queues.alloc_or_grow();
        let queue_id = control.queue_id();

        let subscriber_ctx = self.subscriber.create_connection_context(&meta, &info);

        let remote_addr = segment.remote_address().get();

        let peer = udp::PoolSocket {
            peer_addr: remote_addr,
            control,
            stream,
            socket,
        };

        let stream = match endpoint::accept_stream(
            now,
            &self.env,
            peer,
            &packet,
            queue_id,
            &self.secrets,
            self.subscriber.clone(),
            subscriber_ctx,
            None,
        ) {
            Ok(stream) => stream,
            Err(error) => {
                tracing::trace!("send_start");

                let addr = msg::addr::Addr::new(remote_addr);
                let ecn = Default::default();
                let buffer = &[io::IoSlice::new(&error.secret_control)];

                // ignore any errors since this is just for responding to invalid connect attempts
                let _ = self.socket.try_send(&addr, ecn, buffer);

                tracing::trace!("send_finish");
                return;
            }
        };

        // cache the mapping
        let _ = self.handshake.insert(credentials, queue_id);

        {
            let remote_address: SocketAddress = stream.shared.read_remote_addr();
            let remote_address = &remote_address;
            let credential_id = &*stream.shared.credentials().id;
            let stream_id = stream.shared.application().stream_id.into_varint().as_u64();
            publisher.on_acceptor_udp_stream_enqueued(event::builder::AcceptorUdpStreamEnqueued {
                remote_address,
                credential_id,
                stream_id,
            });
        }

        let res = match self.accept_flavor {
            accept::Flavor::Fifo => self.sender.send_back(stream),
            accept::Flavor::Lifo => self.sender.send_front(stream),
        };

        match res {
            Ok(prev) => {
                if let Some(stream) = prev {
                    stream.prune(
                        event::builder::AcceptorStreamPruneReason::AcceptQueueCapacityExceeded,
                    );
                }
            }
            Err(_) => {
                debug!("application accept queue dropped; shutting down");
                self.is_open = false;
            }
        }
    }
}

fn publisher<'a, Sub: Subscriber, C: Clock>(
    subscriber: &'a Sub,
    clock: &C,
) -> event::EndpointPublisherSubscriber<'a, Sub> {
    let timestamp = clock.get_time().into_event();

    event::EndpointPublisherSubscriber::new(
        event::builder::EndpointMeta { timestamp },
        None,
        subscriber,
    )
}
