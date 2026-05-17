// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! stream Server: inbound connection acceptance
//!
//! The Server is constructed from an Arc<Endpoint> and a PSK server provider. It provides
//! acceptor registration with two modes:
//!
//! 1. **Channel acceptor** (`register_acceptor_channel`): Returns a Receiver that yields
//!    pending streams that must be validated. The acceptor handle is managed internally - dropping all receivers
//!    unregisters the acceptor.
//!
//! 2. **Direct acceptor** (`register_acceptor`): Takes an `impl Acceptor<PendingValidation>` directly
//!    for full control over flow handling.

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

/// Server for accepting inbound stream connections
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

    /// Register a channel-based acceptor that yields pending streams
    ///
    /// Returns a Receiver that yields accepted streams. Cloning the receiver
    /// scales out acceptance across multiple tasks via pick-two load balancing.
    /// The acceptor is automatically unregistered when all receivers are dropped.
    ///
    /// `config` controls the per-receiver queue capacity and eviction policy.
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
