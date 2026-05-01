// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! dcQUIC streams over UDP dispatch from a central [`Acceptor`] to the per-stream sockets.
//!
//! This is typically not used directly but rather wrapped in a Server from one of the other
//! modules ([`super::tokio`] for most production applications).

use super::{accept, InitialPacket};
use crate::{
    credentials::Credentials,
    event::{self, EndpointPublisher as _, Subscriber},
    msg,
    packet::stream,
    path::secret,
    socket::{
        pool::{self, descriptor},
        recv::router::Router,
    },
    stream::{
        endpoint,
        environment::{udp, Environment},
        load_balance::PickTwo,
        recv::dispatch::{Allocator, Dispatch},
        socket, TransportFeatures,
    },
    sync::mpsc,
};
use s2n_quic_core::{
    event::IntoEvent,
    inet::{ExplicitCongestionNotification, SocketAddress},
    time::Clock,
    varint::VarInt,
};
use std::{io, sync::Arc};
use tracing::debug;

pub struct Acceptor<Env, S, W, R>
where
    Env: Environment,
    S: socket::application::Application,
    W: socket::Socket,
    R: socket::Socket,
{
    sender: accept::Sender<Env::Subscriber>,
    env: Env,
    secrets: secret::Map,
    accept_flavor: accept::Flavor,
    dispatch: Dispatch,
    queues: Allocator,
    is_open: bool,
    packet: InitialPacket,
    transmission_pool: pool::Pool,
    application_sockets: Box<[Arc<S>]>,
    worker_socket: Arc<W>,
    secret_socket: R,
    load_balancer: PickTwo,
}

impl<Env, S, W, R> Acceptor<Env, S, W, R>
where
    Env: Environment,
    S: socket::application::Application,
    W: socket::Socket,
    R: socket::Socket,
{
    pub fn new(
        env: Env,
        sender: accept::Sender<Env::Subscriber>,
        secrets: secret::Map,
        accept_flavor: accept::Flavor,
        queues: Allocator,
        application_sockets: Box<[Arc<S>]>,
        worker_socket: Arc<W>,
        secret_socket: R,
        transmission_pool: pool::Pool,
        unroutable_packets: mpsc::Sender<descriptor::Filled>,
    ) -> Self {
        let dispatch = queues.dispatcher(unroutable_packets);
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
            transmission_pool,
            application_sockets,
            worker_socket,
            secret_socket,
            load_balancer: PickTwo::new(),
        }
    }
}

impl<Env, S, W, R> Router for Acceptor<Env, S, W, R>
where
    Env: Environment + 'static,
    Env::Subscriber: Clone,
    S: socket::application::Application,
    W: socket::Socket,
    R: socket::Socket,
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
        let peer_addr = segment.remote_address().get();

        #[cfg(debug_assertions)]
        let _span = tracing::warn_span!("stream", %peer_addr, flow_id = %credentials).entered();

        tracing::debug!(%peer_addr, flow_id = %credentials, "routing zero queue");

        // check to see if these credentials are associated with an active stream
        if let Some(queue_id) = self.dispatch.queue_id_for_key(&credentials) {
            tracing::trace!(%queue_id, "credential_cache_hit");
            let _ = self
                .dispatch
                .send_stream(queue_id, Some(&credentials), segment);
            return;
        }

        let (control, stream) = self.queues.alloc_or_grow(&credentials);

        debug_assert_ne!(control.queue_id(), VarInt::ZERO);

        // inject the packet into the stream queue
        let _ = stream.push(segment);

        let now = self.env.clock().get_time();
        let meta = event::api::ConnectionMeta {
            id: event::next_connection_id(),
            timestamp: now.into_event(),
        };
        let peer_addr_sa: SocketAddress = peer_addr;
        let info = event::api::ConnectionInfo {
            credential_id: &*credentials.id,
            key_id: credentials.key_id.as_u64(),
            remote_address: (&peer_addr_sa).into_event(),
            is_server: true,
        };
        let subscriber_ctx = self
            .env
            .subscriber()
            .create_connection_context(&meta, &info);

        // Use "pick 2" load balancing to select an application socket
        let idx = self.load_balancer.select(
            &self.application_sockets,
            |socket| Arc::strong_count(socket),
            |upper_bound| rand::random_range(..upper_bound),
        );
        let application_socket = self.application_sockets[idx].clone();
        let worker_socket = self.worker_socket.clone();
        let transmission_pool = self.transmission_pool.clone();

        let peer = udp::Pooled {
            peer_addr,
            control,
            stream,
            application_socket,
            worker_socket,
            transmission_pool,
        };

        let mut secret_control = vec![];
        let (crypto, parameters) = match endpoint::derive_stream_credentials(
            &self.packet,
            &self.secrets,
            &TransportFeatures::UDP,
            &mut secret_control,
        ) {
            Ok(result) => result,
            Err(error) => {
                tracing::debug!(?error, "failed to derive stream credentials");

                if !secret_control.is_empty() {
                    let addr = msg::addr::Addr::new(peer_addr);
                    let ecn = Default::default();
                    let buffer = &[io::IoSlice::new(&secret_control)];
                    let _ = self.secret_socket.try_send(&addr, ecn, buffer);
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

        let stream = stream.into();
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

    #[inline]
    fn handle_control_packet(
        &mut self,
        _remote_address: SocketAddress,
        _ecn: ExplicitCongestionNotification,
        _packet: crate::packet::control::decoder::Packet<&mut [u8]>,
    ) {
    }

    #[inline]
    fn dispatch_control_packet(
        &mut self,
        packet: crate::packet::control::decoder::Packet<descriptor::Filled>,
    ) {
        let credentials = *packet.credentials();
        let segment = packet.into_parts().1;

        // check to see if these credentials are associated with an active stream
        if let Some(queue_id) = self.dispatch.queue_id_for_key(&credentials) {
            tracing::trace!(%queue_id, "credential_cache_hit");
            let _ = self
                .dispatch
                .send_control(queue_id, Some(&credentials), segment);
            return;
        }
    }
}
