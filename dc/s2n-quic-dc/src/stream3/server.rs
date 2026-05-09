// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream3 Server: inbound connection acceptance
//!
//! The Server is constructed from an Arc<Endpoint> and a PSK server provider. It provides
//! acceptor registration with two modes:
//!
//! 1. **Channel acceptor** (`register_acceptor_channel`): Returns a Receiver that yields
//!    Streams. The acceptor handle is managed internally - dropping all receivers
//!    unregisters the acceptor.
//!
//! 2. **Direct acceptor** (`register_acceptor`): Takes an `impl Acceptor<Stream>` directly
//!    for full control over flow handling.
//!
//! Unlike stream2, the endpoint constructs the Stream directly before dispatching to the
//! acceptor. The Stream's Reader is already in the correct state (PendingValidation or Open)
//! so the acceptor doesn't need to build anything — it just uses the stream.

use crate::{
    acceptor,
    stream3::{
        endpoint::{reset_error::ResetError, Endpoint},
        Stream,
    },
    sync::mpmc,
};
use s2n_quic_core::varint::VarInt;
use std::{io, sync::Arc};

/// Server for accepting inbound stream3 connections
///
/// Cheap to clone - holds an Arc to the shared Endpoint and PSK provider.
#[derive(Clone)]
pub struct Server {
    endpoint: Arc<Endpoint>,
    #[allow(dead_code)]
    psk: crate::psk::server::Provider,
}

impl Server {
    /// Create a new Server from a shared Endpoint and PSK provider
    ///
    /// # Panics
    ///
    /// Panics if the PSK provider's map is not the same instance as the endpoint's map.
    pub fn new(endpoint: Arc<Endpoint>, psk: crate::psk::server::Provider) -> Self {
        assert_eq!(
            endpoint.path_secret_map,
            *psk.map(),
            "PSK provider map must be the same instance as the endpoint map"
        );
        Self { endpoint, psk }
    }

    /// Register an acceptor directly
    ///
    /// This gives full control over how accepted streams are handled. The acceptor
    /// handle is returned for the caller to manage.
    pub fn register_acceptor(
        &self,
        acceptor_id: VarInt,
        acceptor: Arc<dyn acceptor::Acceptor<Stream>>,
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

    /// Register a channel-based acceptor that yields Streams
    ///
    /// Returns a Receiver that yields accepted streams. The acceptor is automatically
    /// unregistered when all receivers are dropped.
    ///
    /// `capacity` controls the accept queue depth. Under high connection rates, older
    /// pending streams are dropped to make room for new ones.
    pub fn register_acceptor_channel(
        &self,
        acceptor_id: VarInt,
        capacity: usize,
    ) -> io::Result<mpmc::Receiver<Stream>> {
        let (tx, rx) = mpmc::new(capacity);

        let channel_acceptor = Arc::new(ChannelAcceptor {
            tx,
            handle: std::sync::Mutex::new(None),
        });

        let handle = self
            .endpoint
            .acceptor_registry
            .register(acceptor_id, channel_acceptor.clone())
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::AddrInUse, "acceptor ID already registered")
            })?;

        *channel_acceptor.handle.lock().unwrap() = Some(handle);

        Ok(rx)
    }
}

struct ChannelAcceptor {
    tx: mpmc::Sender<Stream>,
    handle: std::sync::Mutex<Option<acceptor::Handle>>,
}

impl acceptor::Acceptor<Stream> for ChannelAcceptor {
    fn handle_request(&self, stream: Stream) {
        self.send(stream);
    }

    fn handle_pending(&self, stream: Stream) -> acceptor::PendingAction {
        self.send(stream);
        acceptor::PendingAction::AcceptedWithRetry
    }
}

impl ChannelAcceptor {
    fn send(&self, stream: Stream) {
        match self.tx.send_back(stream) {
            Ok(Some(mut evicted)) => {
                evicted.reset(ResetError::ServerBusy);
            }
            Ok(None) => {}
            Err(_) => {
                drop(self.handle.lock().unwrap().take());
            }
        }
    }
}
