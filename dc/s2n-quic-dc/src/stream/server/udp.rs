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
        socket,
    },
};
use s2n_quic_core::{
    event::IntoEvent,
    inet::{ExplicitCongestionNotification, SocketAddress},
    time::Clock,
    varint::VarInt,
};
use schnellru::LruMap;
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
    credential_cache: CredentialCache,
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
        credential_cache_size: u32,
    ) -> Self {
        let dispatch = queues.dispatcher();
        let credential_cache = create_credential_cache(credential_cache_size);
        let packet = InitialPacket::empty();
        Self {
            sender,
            env,
            secrets,
            accept_flavor,
            credential_cache,
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
        let credentials = CredentialsHashable(credentials);
        if let Some(queue_id) = self.credential_cache.get(&credentials) {
            tracing::trace!(%queue_id, "credential_cache_hit");
            if self.dispatch.send_stream(*queue_id, segment).is_err() {
                // if the dispatch didn't work then remove it from the LRU
                let _ = self.credential_cache.remove(&credentials);
            }
            return;
        }

        let peer_addr = segment.remote_address().get();

        let (control, stream) = self.queues.alloc_or_grow();
        let queue_id = control.queue_id();
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

        // remember the associated queue_id for the credentials
        self.credential_cache.insert(credentials, queue_id);

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

type CredentialCache = LruMap<CredentialsHashable, VarInt>;

fn create_credential_cache(max_length: u32) -> CredentialCache {
    use schnellru::RandomState;
    let limits = schnellru::ByLength::new(max_length);
    // we need to use random state since the key is completely controlled by the peer
    let random = RandomState::default();
    LruMap::with_hasher(limits, random)
}

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
