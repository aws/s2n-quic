// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{accept, InitialPacket};
use crate::{
    credentials::Credentials,
    event::{self, EndpointPublisher as _, Subscriber},
    msg,
    packet::stream,
    path::secret,
    socket::recv::{descriptor, router::Router},
    stream::{
        endpoint,
        environment::{udp, Environment},
        recv::dispatch::{Allocator, Dispatch},
        socket, TransportFeatures,
    },
};
use s2n_quic_core::{
    event::IntoEvent,
    inet::{ExplicitCongestionNotification, SocketAddress},
    time::Clock,
};
use std::{io, sync::Arc};
use tracing::debug;

pub struct Acceptor<Env, S, W>
where
    Env: Environment,
    S: socket::application::Application,
    W: socket::Socket,
{
    sender: accept::Sender<Env::Subscriber>,
    env: Env,
    secrets: secret::Map,
    accept_flavor: accept::Flavor,
    dispatch: Dispatch,
    queues: Allocator,
    is_open: bool,
    packet: InitialPacket,
    application_socket: Arc<S>,
    worker_socket: Arc<W>,
}

impl<Env, S, W> Acceptor<Env, S, W>
where
    Env: Environment,
    S: socket::application::Application,
    W: socket::Socket,
{
    pub fn new(
        env: Env,
        sender: accept::Sender<Env::Subscriber>,
        secrets: secret::Map,
        accept_flavor: accept::Flavor,
        queues: Allocator,
        application_socket: Arc<S>,
        worker_socket: Arc<W>,
    ) -> Self {
        let dispatch = queues.dispatcher();
        let packet = InitialPacket::empty();
        Self {
            sender,
            env,
            secrets,
            accept_flavor,
            dispatch,
            queues,
            is_open: true,
            packet,
            application_socket,
            worker_socket,
        }
    }
}

impl<Env, S, W> Router for Acceptor<Env, S, W>
where
    Env: Environment,
    Env::Subscriber: Clone,
    S: socket::application::Application,
    W: socket::Socket,
{
    #[inline]
    fn is_open(&self) -> bool {
        self.is_open
    }

    #[inline]
    fn handle_stream_packet(
        &mut self,
        _remote_address: SocketAddress,
        _ecn: ExplicitCongestionNotification,
        packet: stream::decoder::Packet,
    ) {
        self.packet = packet.into();
    }

    #[inline]
    fn dispatch_stream_packet(
        &mut self,
        _tag: stream::Tag,
        _id: stream::Id,
        credentials: Credentials,
        segment: descriptor::Filled,
    ) {
        // check to see if these credentials are associated with an active stream
        if let Some(queue_id) = self.dispatch.queue_id_for_key(&credentials) {
            tracing::trace!(%queue_id, "credential_cache_hit");
            let _ = self.dispatch.send_stream(queue_id, segment);
            return;
        }

        let peer_addr = segment.remote_address().get();

        let (control, stream) = self.queues.alloc_or_grow(Some(&credentials));
        // inject the packet into the stream queue
        let _ = stream.push(segment);

        let now = self.env.clock().get_time();
        let meta = event::api::ConnectionMeta {
            id: 0, // TODO use an actual connection ID
            timestamp: now.into_event(),
        };
        let info = event::api::ConnectionInfo {};
        let subscriber_ctx = self
            .env
            .subscriber()
            .create_connection_context(&meta, &info);

        let application_socket = self.application_socket.clone();
        let worker_socket = self.worker_socket.clone();

        let peer = udp::Pooled {
            peer_addr,
            control,
            stream,
            application_socket,
            worker_socket,
        };

        let mut secret_control = vec![];
        let (crypto, parameters) = match endpoint::derive_stream_credentials(
            &self.packet,
            &self.secrets,
            &TransportFeatures::UDP,
            &mut secret_control,
        ) {
            Ok(result) => result,
            Err(_error) => {
                if !secret_control.is_empty() {
                    let addr = msg::addr::Addr::new(peer_addr);
                    let ecn = Default::default();
                    let buffer = &[io::IoSlice::new(&secret_control)];
                    let _ = self.worker_socket.try_send(&addr, ecn, buffer);
                }
                return;
            }
        };

        // TODO is it better to accept this inline or send it off to another queue?
        //      maybe only delegate to another task when the receiver becomes overloaded?

        let stream = match endpoint::accept_stream(
            now,
            &self.env,
            peer,
            &self.packet,
            &self.secrets,
            subscriber_ctx,
            None,
            crypto,
            parameters,
            secret_control,
        ) {
            Ok(stream) => stream,
            Err(error) => {
                tracing::trace!("send_start");

                if !error.secret_control.is_empty() {
                    let addr = msg::addr::Addr::new(peer_addr);
                    let ecn = Default::default();
                    let buffer = &[io::IoSlice::new(&error.secret_control)];

                    // ignore any errors since this is just for responding to invalid connect attempts
                    let _ = self.worker_socket.try_send(&addr, ecn, buffer);
                }

                tracing::trace!("send_finish");
                return;
            }
        };

        {
            let remote_address: SocketAddress = stream.shared.remote_addr();
            let remote_address = &remote_address;
            let creds = stream.shared.credentials();
            let credential_id = &creds.id[..];
            let stream_id = creds.key_id.as_u64();
            self.env
                .endpoint_publisher_with_time(now)
                .on_acceptor_udp_stream_enqueued(event::builder::AcceptorUdpStreamEnqueued {
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
