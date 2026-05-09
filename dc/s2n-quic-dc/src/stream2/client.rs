// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream2 Client: outbound connection establishment
//!
//! The Client is constructed from an Arc<Endpoint> and a PSK client provider. It provides
//! connect() which performs a handshake if needed and returns a Stream. The Client holds its
//! own clone of the queue allocator to avoid synchronization on the hot path.
//!
//! Flow initialization is lazy: connect() allocates local queues and returns immediately.
//! The Writer sends FlowInit on the first write, potentially with early data.

pub mod rpc;

use crate::{
    flow::{self, queue},
    psk,
    stream2::{
        endpoint::{ControlMsg, Endpoint, StreamMsg},
        Stream,
    },
};
use s2n_quic::server::Name;
use s2n_quic_core::varint::VarInt;
use std::{
    io,
    net::SocketAddr,
    sync::{atomic::Ordering, Arc},
};

/// Client for making outbound stream2 connections
#[derive(Clone)]
pub struct Client {
    endpoint: Arc<Endpoint>,
    psk: psk::client::Provider,
    server_name: Name,
    queue_allocator: queue::Allocator<StreamMsg, ControlMsg, flow::Handle>,
}

impl Client {
    /// Create a new Client from a shared Endpoint and PSK provider
    ///
    /// # Panics
    ///
    /// Panics if the PSK provider's map is not the same instance as the endpoint's map.
    pub fn new(endpoint: Arc<Endpoint>, psk: psk::client::Provider, server_name: Name) -> Self {
        assert_eq!(
            endpoint.path_secret_map,
            *psk.map(),
            "PSK provider map must be the same instance as the endpoint map"
        );
        let queue_allocator = endpoint.queue_allocator.clone();
        Self {
            endpoint,
            psk,
            server_name,
            queue_allocator,
        }
    }

    /// Connect to a peer, returning a Stream
    ///
    /// Performs a TLS handshake if no path secret exists for the peer yet. Allocates local
    /// flow queues and returns immediately - the actual FlowInit packet is sent lazily on
    /// the first write (with optional early data).
    ///
    /// `acceptor_id` identifies which acceptor on the server should handle this stream.
    pub async fn connect(&mut self, peer: SocketAddr, acceptor_id: VarInt) -> io::Result<Stream> {
        let (peer, _kind) = self
            .psk
            .handshake_with_entry(peer, self.server_name.clone())
            .await?;

        let path_secret_entry = peer.into_raw();

        // Allocate endpoint-wide stream ID
        let stream_id =
            VarInt::new(self.endpoint.next_stream_id.fetch_add(1, Ordering::Relaxed)).unwrap();

        let handle = flow::Handle::client(stream_id, path_secret_entry.clone());

        let (queue_control, queue_stream) = self.queue_allocator.alloc_or_grow(handle, None);

        let writer = crate::stream2::Writer::new_client(
            self.endpoint.wheel_input_tx.clone(),
            path_secret_entry.clone(),
            self.endpoint.gso.clone(),
            stream_id,
            acceptor_id,
            queue_control,
        );

        let reader = crate::stream2::Reader::new_client(
            self.endpoint.wheel_input_tx.clone(),
            path_secret_entry,
            stream_id,
            queue_stream,
        );

        Ok(Stream::new(reader, writer))
    }

    /// Perform an RPC over a new stream: send the request and collect the response
    pub async fn rpc<Req, Res>(
        &mut self,
        peer: SocketAddr,
        acceptor_id: VarInt,
        request: Req,
        response: Res,
    ) -> io::Result<Res::Output>
    where
        Req: rpc::Request,
        Res: rpc::Response,
    {
        let stream = self.connect(peer, acceptor_id).await?;
        rpc::from_stream(stream, request, response).await
    }
}
