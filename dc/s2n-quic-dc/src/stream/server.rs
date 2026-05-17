// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! stream Server: inbound connection acceptance
//!
//! The Server is constructed from an `Arc<Endpoint>` and a PSK server provider. It provides
//! acceptor registration with two modes:
//!
//! 1. **Channel acceptor** ([`register_acceptor_channel`](Server::register_acceptor_channel)):
//!    Returns a [`Receiver`](crate::acceptor::channel::Receiver) that yields pending streams
//!    that must be validated. The acceptor handle is managed internally — dropping all receivers
//!    unregisters the acceptor.
//!
//! 2. **Direct acceptor** ([`register_acceptor`](Server::register_acceptor)): Takes an
//!    `impl Acceptor<PendingValidation>` directly for full control over flow handling.

use crate::{
    acceptor::{self, channel as accept_channel},
    flow::queue::AutoWake,
    stream::{
        endpoint::{Endpoint, Error},
        PendingValidation,
    },
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
/// - An acceptor is automatically unregistered when all of its receivers (channel mode) or
///   its [`Handle`](crate::acceptor::Handle) (direct mode) are dropped.
///
/// # Footguns
///
/// - The PSK provider's map **must** be the same `Arc` instance as the endpoint's map.
///   [`new`](Self::new) panics if they differ.
/// - Streams arrive as [`PendingValidation`]; callers must call
///   [`validate`](PendingValidation::validate) before reading from or writing to the stream.
///
/// # Example
///
/// ```ignore
/// use s2n_quic_dc::stream::{PendingValidation, Server, Stream};
/// use s2n_quic_core::varint::VarInt;
///
/// async fn accept_loop(
///     server: Server,
///     acceptor_id: VarInt,
/// ) -> std::io::Result<()> {
///     let config = s2n_quic_dc::acceptor::channel::Config::default();
///     let mut rx = server.register_acceptor_channel(acceptor_id, config)?;
///     while let Some(pending) = rx.recv().await {
///         tokio::spawn(async move {
///             if let Ok(stream) = pending.validate().await {
///                 handle(stream).await;
///             }
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

    /// Registers an acceptor directly, giving full control over stream handling.
    ///
    /// The returned [`Handle`](crate::acceptor::Handle) keeps the acceptor registered for as
    /// long as it is held. Dropping the handle unregisters the acceptor.
    ///
    /// Use this form when you need custom dispatch logic or want to manage the acceptor
    /// lifecycle explicitly. For the common case of feeding streams into an async task,
    /// prefer [`register_acceptor_channel`](Self::register_acceptor_channel).
    ///
    /// # Errors
    ///
    /// Returns `AddrInUse` if `acceptor_id` is already registered.
    pub fn register_acceptor(
        &self,
        acceptor_id: VarInt,
        acceptor: Arc<dyn acceptor::Acceptor<PendingValidation>>,
    ) -> io::Result<acceptor::Handle> {
        self.endpoint
            .acceptor_registry
            .register(acceptor_id, acceptor)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("acceptor ID ({acceptor_id}) already registered"),
                )
            })
    }

    /// Registers a channel-based acceptor and returns a receiver for accepted streams.
    ///
    /// Incoming [`PendingValidation`] streams are placed in a bounded queue. Callers read
    /// from the returned receiver and call [`validate`](PendingValidation::validate) before
    /// using each stream.
    ///
    /// Cloning the receiver scales out acceptance across multiple tasks: incoming streams
    /// are distributed across all live receivers using pick-two load balancing.
    ///
    /// The acceptor is automatically unregistered when all clones of the receiver are
    /// dropped. No explicit cleanup is required.
    ///
    /// `config` controls the per-receiver queue capacity and eviction policy for streams
    /// that arrive faster than the application can accept them.
    ///
    /// # Errors
    ///
    /// Returns `AddrInUse` if `acceptor_id` is already registered.
    pub fn register_acceptor_channel(
        &self,
        acceptor_id: VarInt,
        config: accept_channel::Config,
    ) -> io::Result<accept_channel::Receiver<PendingValidation>> {
        let (tx, rx) = accept_channel::new(config);

        let channel_acceptor = Arc::new(ChannelAcceptor::new(tx));

        let handle = self
            .endpoint
            .acceptor_registry
            .register(acceptor_id, channel_acceptor.clone())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::AddrInUse, "acceptor ID already registered")
            })?;

        channel_acceptor.set_handle(handle);

        Ok(rx)
    }
}

pub struct ChannelAcceptor {
    tx: parking_lot::Mutex<accept_channel::Sender<PendingValidation>>,
    handle: std::sync::Mutex<Option<acceptor::Handle>>,
}

impl ChannelAcceptor {
    pub fn new(tx: accept_channel::Sender<PendingValidation>) -> Self {
        Self {
            tx: parking_lot::Mutex::new(tx),
            handle: std::sync::Mutex::new(None),
        }
    }

    pub fn set_handle(&self, handle: acceptor::Handle) {
        *self.handle.lock().unwrap() = Some(handle);
    }
}

impl acceptor::Acceptor<PendingValidation> for ChannelAcceptor {
    fn handle_request(&self, stream: PendingValidation) -> AutoWake {
        self.send(stream)
    }

    fn handle_pending(&self, stream: PendingValidation) -> acceptor::Dispatch {
        let waker = self.send(stream);
        acceptor::Dispatch {
            action: acceptor::PendingAction::AcceptedWithRetry,
            waker,
        }
    }
}

impl ChannelAcceptor {
    fn send(&self, stream: PendingValidation) -> AutoWake {
        let res = {
            let mut tx = self.tx.lock();
            tx.send(stream)
        };
        match res {
            Ok((Some(mut evicted), waker)) => {
                evicted.reset(Error::ServerBusy);
                AutoWake::new(waker)
            }
            Ok((None, waker)) => AutoWake::new(waker),
            Err(_) => {
                drop(self.handle.lock().unwrap().take());
                AutoWake::new(None)
            }
        }
    }
}
