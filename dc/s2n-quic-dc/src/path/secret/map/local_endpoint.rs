// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::store::Store;
use crate::psk::io::HandshakeReason;
use slab::Slab;
use std::{
    net::SocketAddr,
    num::NonZeroU32,
    sync::{Arc, RwLock},
};
use tokio::task::JoinHandle;

pub(crate) type RequestHandshake =
    dyn Fn(SocketAddr, HandshakeReason) -> Option<JoinHandle<()>> + Send + Sync;

/// Identifies a local endpoint attached to a map. Reused once the endpoint is dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LocalEndpointId(NonZeroU32);

/// A local endpoint's registration with a map. Dropping it unregisters the endpoint.
pub(crate) struct LocalEndpoint {
    id: LocalEndpointId,
    store: Arc<dyn Store>,
}

impl LocalEndpoint {
    pub(super) fn new(id: LocalEndpointId, store: Arc<dyn Store>) -> Self {
        Self { id, store }
    }

    pub(crate) fn id(&self) -> LocalEndpointId {
        self.id
    }
}

impl Drop for LocalEndpoint {
    fn drop(&mut self) {
        self.store.unregister_local_endpoint(self.id);
    }
}

/// The local endpoints currently registered with a map, with the callback that re-handshakes the
/// entries each one created.
pub(super) struct LocalEndpoints {
    callbacks: RwLock<Slab<Arc<RequestHandshake>>>,
}

impl Default for LocalEndpoints {
    fn default() -> Self {
        let mut callbacks = Slab::new();
        // We reserve the 0th ID/key so that we can use it to represent `None` in an
        // `Option<LocalEndpointId>`, which keeps the option the same size as the ID.
        callbacks.insert(Arc::new(|_, _| None) as Arc<RequestHandshake>);
        Self {
            callbacks: RwLock::new(callbacks),
        }
    }
}

impl LocalEndpoints {
    pub(super) fn register(&self, callback: Arc<RequestHandshake>) -> LocalEndpointId {
        let key = self
            .callbacks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(callback);
        #[expect(
            clippy::expect_used,
            reason = "The amount of active endpoints on a single map should be limited by FD before we hit u32::MAX"
        )]
        let id = u32::try_from(key)
            .ok()
            .and_then(NonZeroU32::new)
            .expect("too many local endpoints");
        LocalEndpointId(id)
    }

    pub(super) fn unregister(&self, id: LocalEndpointId) {
        self.callbacks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .try_remove(id.key());
    }

    /// Returns the callback that re-handshakes the entries `id` created.
    ///
    /// If `id` is no longer registered, its socket is gone, and with it the address the peer knows
    /// those entries by. Any registered endpoint reaches the same peer and is then as good as the
    /// original, so this falls back to one of them.
    ///
    /// The fallback endpoint handshakes with its own configuration, which may differ from the
    /// original's. Today there's no known use case for differences here.
    pub(super) fn get(&self, id: LocalEndpointId) -> Option<Arc<RequestHandshake>> {
        let callbacks = self.callbacks.read().unwrap_or_else(|e| e.into_inner());
        callbacks
            .get(id.key())
            .or_else(|| {
                callbacks
                    .iter()
                    .find(|(key, _)| *key != 0)
                    .map(|(_, callback)| callback)
            })
            .cloned()
    }
}

impl LocalEndpointId {
    fn key(self) -> usize {
        self.0.get() as usize
    }
}
