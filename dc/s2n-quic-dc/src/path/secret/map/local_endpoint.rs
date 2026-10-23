// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::store::Store;
use crate::psk::io::HandshakeReason;
use std::{
    collections::HashMap,
    net::SocketAddr,
    num::NonZeroU32,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, RwLock,
    },
};
use tokio::task::JoinHandle;

pub(crate) type RequestHandshake =
    dyn Fn(SocketAddr, HandshakeReason) -> Option<JoinHandle<()>> + Send + Sync;

/// Identifies a local endpoint attached to a map. Never reused within a map.
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

    /// Sets the callback that re-handshakes entries this endpoint created.
    ///
    /// Set after registering: the endpoint's map handle needs the id before the endpoint is built,
    /// while the callback needs the built endpoint.
    pub(crate) fn set_rehandshake_callback(&self, callback: Arc<RequestHandshake>) {
        self.store.set_rehandshake_callback(self.id, callback);
    }
}

impl Drop for LocalEndpoint {
    fn drop(&mut self) {
        self.store.unregister_local_endpoint(self.id);
    }
}

/// The local endpoints currently registered with a map.
#[derive(Default)]
pub(super) struct LocalEndpoints {
    next_id: AtomicU32,
    callbacks: RwLock<HashMap<LocalEndpointId, Arc<RequestHandshake>>>,
}

impl LocalEndpoints {
    pub(super) fn register(&self) -> LocalEndpointId {
        #[expect(
            clippy::expect_used,
            reason = "u32::MAX endpoints attached over one map's lifetime is not a realistic workload"
        )]
        let id = self
            .next_id
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .ok()
            .and_then(|id| NonZeroU32::new(id + 1))
            .expect("local endpoint ids exhausted");
        LocalEndpointId(id)
    }

    pub(super) fn set_rehandshake_callback(
        &self,
        id: LocalEndpointId,
        callback: Arc<RequestHandshake>,
    ) {
        self.callbacks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, callback);
    }

    pub(super) fn unregister(&self, id: LocalEndpointId) {
        self.callbacks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id);
    }

    pub(super) fn get(&self, id: LocalEndpointId) -> Option<Arc<RequestHandshake>> {
        self.callbacks
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .cloned()
    }
}
