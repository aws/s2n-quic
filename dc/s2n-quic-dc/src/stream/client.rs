// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! stream Client: outbound connection establishment
//!
//! The Client is constructed from an `Arc<Endpoint>` and a PSK client provider. It provides
//! [`connect`](Client::connect) which performs a handshake if needed and returns a [`Stream`].
//! The Client holds its own clone of the queue allocator to avoid synchronization on the hot path.
//!
//! Flow initialization is lazy: `connect` allocates local queues and returns immediately.
//! The [`Writer`](crate::stream::Writer) sends `FlowInit` on the first write, potentially
//! with early data.

use crate::{
    flow, psk,
    stream::{
        endpoint::{msg, Endpoint},
        Reader, Stream, Writer,
    },
};
use s2n_quic::server::Name;
use s2n_quic_core::varint::VarInt;
use std::{
    io,
    net::SocketAddr,
    sync::{atomic::Ordering, Arc},
};

pub mod rpc;

/// Client for making outbound `s2n-quic-dc` stream connections.
///
/// `Client` wraps a shared [`Endpoint`] and a PSK provider to open bidirectional [`Stream`]s
/// to a remote server. Each call to [`connect`](Self::connect) performs a TLS handshake if
/// no path secret exists yet, then allocates local queues and returns a ready-to-use stream.
///
/// `Client` is cheap to clone: it holds an `Arc` to the shared endpoint and an independent
/// copy of the queue allocator, so hot-path connection creation does not require global
/// synchronization.
///
/// # Expectations and guarantees
///
/// - Every clone shares the same underlying endpoint and path-secret map.
/// - Flow initialization is lazy. The [`Writer`](crate::stream::Writer) sends `FlowInit`
///   (with optional early data) on the first write after `connect` returns.
/// - The TLS handshake, if needed, is performed inside `connect` and is transparent to
///   the caller.
///
/// # Footguns
///
/// - The PSK provider's map **must** be the same `Arc` instance as the endpoint's map.
///   [`new`](Self::new) panics if they differ.
/// - `connect` only ensures a path secret exists. It does not guarantee the server is
///   reachable or that the stream will complete successfully.
///
/// # Example
///
/// ```ignore
/// use s2n_quic_dc::stream::{Client, Stream};
/// use s2n_quic_core::varint::VarInt;
/// use std::net::SocketAddr;
///
/// async fn open_stream(
///     mut client: Client,
///     server: SocketAddr,
/// ) -> std::io::Result<Stream> {
///     let acceptor_id = VarInt::from_u8(0);
///     client.connect(server, acceptor_id).await
/// }
/// ```
#[derive(Clone)]
pub struct Client {
    endpoint: Arc<Endpoint>,
    psk: psk::client::Provider,
    server_name: Name,
    queue_allocator: msg::queue::Allocator,
}

impl Client {
    /// Creates a new `Client` from a shared [`Endpoint`] and PSK provider.
    ///
    /// `server_name` is the TLS server name used during handshakes with the peer.
    ///
    /// # Panics
    ///
    /// Panics if the PSK provider's map is not the same `Arc` instance as the endpoint's map.
    /// Both must point to the same shared path-secret store.
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

    /// Opens a new bidirectional stream to `peer`.
    ///
    /// If no path secret already exists for `peer`, a TLS handshake is performed first.
    /// Once a path secret is available, local queues are allocated and a [`Stream`] is
    /// returned immediately.
    ///
    /// `acceptor_id` identifies which acceptor on the server should handle this stream.
    ///
    /// # Semantics
    ///
    /// - Flow initialization is lazy. The [`Writer`](crate::stream::Writer) sends `FlowInit`
    ///   (possibly with early data) on the first write.
    /// - A successful return does not mean the server has accepted the stream yet; it only
    ///   means local setup succeeded.
    ///
    /// # Footguns
    ///
    /// - If the TLS handshake fails, this returns an error and no stream is created.
    /// - Using an `acceptor_id` not registered on the server causes the server to reject
    ///   the stream, which surfaces as a later write or read error.
    pub async fn connect(&mut self, peer: SocketAddr, acceptor_id: VarInt) -> io::Result<Stream> {
        let (peer, _kind) = self
            .psk
            .handshake_with_entry(peer, self.server_name.clone())
            .await?;

        let path_secret_entry = peer.into_raw();
        let now = crate::time::now();
        let now = crate::time::precision::Timestamp::from(now);
        if path_secret_entry.is_dead_during_cooldown(now, self.endpoint.dead_peer_cooldown) {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "peer is in dead cooldown window",
            ));
        }

        let stream_id =
            VarInt::new(self.endpoint.next_stream_id.fetch_add(1, Ordering::Relaxed)).unwrap();

        let handle = flow::Handle::client(stream_id, path_secret_entry.clone());

        let (queue_control, queue_stream) = self.queue_allocator.alloc_or_grow(handle, None);

        let writer = Writer::new_client(
            self.endpoint.frame_tx.clone(),
            path_secret_entry.clone(),
            stream_id,
            acceptor_id,
            queue_control,
        );

        let reader = Reader::new_client(
            self.endpoint.frame_tx.clone(),
            path_secret_entry,
            stream_id,
            queue_stream,
        );

        Ok(Stream::new(reader, writer))
    }

    /// Performs a single-round-trip RPC: sends `request` and collects `response`.
    ///
    /// Opens a stream to `peer` (with `acceptor_id`), writes the full request payload
    /// with FIN, then reads the full response and returns the caller-chosen output type.
    /// The stream is consumed after the exchange.
    ///
    /// # Footguns
    ///
    /// - The response buffer provided by [`Response::provide_storage`] must have sufficient
    ///   capacity. An error is returned if storage can never grow to fit the server's reply.
    /// - If the request write or response read fails, the stream is dropped without a clean
    ///   shutdown.
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
