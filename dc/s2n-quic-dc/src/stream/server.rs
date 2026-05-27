// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! stream Server: inbound connection acceptance
//!
//! The Server is constructed from an `Arc<Endpoint>` and a PSK server provider. It provides
//! channel-based acceptor registration via
//! [`register_acceptor`](Server::register_acceptor), returning a
//! [`Receiver`](crate::acceptor::channel::Receiver) that yields pending streams.

use crate::{
    acceptor::channel as accept_channel,
    stream::{endpoint::Endpoint, Stream},
};
use s2n_quic_core::varint::VarInt;
use std::{io, sync::Arc};

/// Server for accepting inbound `s2n-quic-dc` stream connections.
///
/// `Server` wraps a shared [`Endpoint`] and a PSK provider to register named acceptors.
/// Each acceptor is identified by a [`VarInt`] ID; the client must supply the same ID when
/// calling [`Client::connect`](crate::stream::Client::connect).
///
/// `Server` is cheap to clone: it holds `Arc` references to the endpoint and PSK provider.
///
/// # Expectations and guarantees
///
/// - Multiple acceptors with different IDs can coexist on one endpoint.
/// - Each acceptor ID can only be registered once; a second registration for the same ID
///   returns an error.
/// - An acceptor is automatically cleaned up when all receivers are dropped and a
///   background cleanup pass runs.
///
/// # Footguns
///
/// - The PSK provider's map **must** be the same `Arc` instance as the endpoint's map.
///   [`new`](Self::new) panics if they differ.
///
/// # Example
///
/// ```ignore
/// use s2n_quic_dc::stream::{Server, Stream};
/// use s2n_quic_core::varint::VarInt;
///
/// async fn accept_loop(
///     server: Server,
///     acceptor_id: VarInt,
/// ) -> std::io::Result<()> {
///     let config = s2n_quic_dc::acceptor::channel::Config::default();
///     let mut rx = server.register_acceptor(acceptor_id, config)?;
///     while let Some(stream) = rx.recv().await {
///         tokio::spawn(async move {
///             handle_stream(stream).await;
///         });
///     }
///     Ok(())
/// }
/// ```
#[derive(Clone)]
pub struct Server {
    endpoint: Arc<Endpoint>,
    #[allow(dead_code)]
    psk: crate::psk::server::Provider,
}

impl Server {
    /// Creates a new `Server` from a shared [`Endpoint`] and PSK provider.
    ///
    /// # Panics
    ///
    /// Panics if the PSK provider's map is not the same `Arc` instance as the endpoint's map.
    /// Both must point to the same shared path-secret store.
    pub fn new(endpoint: Arc<Endpoint>, psk: crate::psk::server::Provider) -> Self {
        assert_eq!(
            endpoint.path_secret_map,
            *psk.map(),
            "PSK provider map must be the same instance as the endpoint map"
        );
        Self { endpoint, psk }
    }

    /// Registers a channel-based acceptor and returns a receiver.
    ///
    /// Incoming streams are placed in a bounded queue.
    ///
    /// Cloning the receiver scales out acceptance across multiple tasks: incoming streams
    /// are distributed across all live receivers using pick-two load balancing.
    ///
    /// The acceptor is automatically cleaned up when all receivers are dropped and a
    /// background cleanup pass runs.
    ///
    /// `config` controls the per-receiver queue capacity and eviction policy for streams
    /// that arrive faster than the application can accept them.
    ///
    /// # Errors
    ///
    /// Returns `AddrInUse` if `acceptor_id` is already registered.
    pub fn register_acceptor(
        &self,
        acceptor_id: VarInt,
        config: accept_channel::Config,
    ) -> io::Result<accept_channel::Receiver<Stream>> {
        self.endpoint
            .acceptor_registry
            .register(acceptor_id, config)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("acceptor ID ({acceptor_id}) already registered"),
                )
            })
    }
}
