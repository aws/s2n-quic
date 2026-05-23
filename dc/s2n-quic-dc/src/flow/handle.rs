// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Flow handle with validation support

use crate::credentials;
use rustc_hash::FxHashMap;
use s2n_quic_core::varint::VarInt;
use std::{
    collections::{hash_map, VecDeque},
    sync::Arc,
};

/// Flow handle that validates credentials and stream_id
///
/// This is used as the Key type in the queue system to ensure that
/// packets routed to a flow actually belong to that flow.
#[derive(Debug, Clone)]
pub struct Handle {
    /// Global stream identifier (client-wide)
    stream_id: VarInt,
    /// Inner state (server-side with drop channel, or client-side with path entry)
    inner: HandleInner,
}

#[derive(Clone)]
enum HandleInner {
    /// Server-side handle — sends stream_id to drop channel on drop
    Server {
        credential_id: credentials::Id,
        drop_channel: Arc<DropChannel>,
    },
    /// Client-side handle with path secret entry
    Client {
        path_entry: Arc<crate::path::secret::map::Entry>,
    },
}

impl core::fmt::Debug for HandleInner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Server { .. } => f.debug_struct("Server").finish_non_exhaustive(),
            Self::Client { .. } => f.debug_struct("Client").finish_non_exhaustive(),
        }
    }
}

impl HandleInner {
    fn credential_id(&self) -> &credentials::Id {
        match self {
            Self::Server { credential_id, .. } => credential_id,
            Self::Client { path_entry } => path_entry.id(),
        }
    }
}

impl Handle {
    /// Create a client-side handle with path secret entry
    pub fn client(stream_id: VarInt, path_entry: Arc<crate::path::secret::map::Entry>) -> Self {
        Self {
            stream_id,
            inner: HandleInner::Client { path_entry },
        }
    }

    pub fn credential_id(&self) -> &credentials::Id {
        self.inner.credential_id()
    }

    pub fn stream_id(&self) -> VarInt {
        self.stream_id
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        if let HandleInner::Server { drop_channel, .. } = &self.inner {
            drop_channel.push(self.stream_id);
        }
    }
}

/// Thread-safe drop notification channel. Handle::drop pushes stream_ids here;
/// the owning dispatch worker drains them.
pub struct DropChannel {
    pending: parking_lot::Mutex<VecDeque<VarInt>>,
}

impl DropChannel {
    pub fn new() -> Self {
        Self {
            pending: parking_lot::Mutex::new(VecDeque::new()),
        }
    }

    fn push(&self, stream_id: VarInt) {
        self.pending.lock().push_back(stream_id);
    }

    fn drain_into(&self, buf: &mut VecDeque<VarInt>) {
        let mut pending = self.pending.lock();
        if pending.is_empty() {
            return;
        }
        // Swap: buf (empty from last drain) goes into pending for allocation reuse,
        // pending (with items) comes out into buf for processing.
        core::mem::swap(&mut *pending, buf);
    }
}

/// Request parameters for flow validation
#[derive(Debug, Clone, Copy)]
pub struct Request {
    pub credential_id: credentials::Id,
    /// When `None`, only credential validation is performed.
    pub stream_id: Option<VarInt>,
}

impl crate::flow::queue::Key for Handle {
    type Request = Request;

    #[inline]
    fn validate(&self, params: &Self::Request) -> Result<(), crate::flow::queue::ValidationError> {
        use crate::flow::queue::ValidationError;
        if self.credential_id() != &params.credential_id {
            return Err(ValidationError::CredentialMismatch);
        }
        if params
            .stream_id
            .is_some_and(|stream_id| self.stream_id != stream_id)
        {
            return Err(ValidationError::StreamIdMismatch);
        }
        Ok(())
    }
}

/// Tracker for managing flow lifecycle on a single dispatch worker thread.
///
/// The map is thread-local (Rc + RefCell). Cross-thread Handle drops are
/// received via a shared DropChannel and applied during `drain_drops`.
#[derive(Clone)]
pub struct Tracker {
    map: std::rc::Rc<std::cell::RefCell<FxHashMap<VarInt, VarInt>>>,
    drop_channel: Arc<DropChannel>,
    drain_buf: std::rc::Rc<std::cell::RefCell<VecDeque<VarInt>>>,
    credentials: credentials::Id,
}

impl core::fmt::Debug for Tracker {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Tracker")
            .field("credentials", &self.credentials)
            .finish_non_exhaustive()
    }
}

impl Tracker {
    #[inline]
    pub fn new(credentials: credentials::Id) -> Self {
        Self {
            map: Default::default(),
            drop_channel: Arc::new(DropChannel::new()),
            drain_buf: Default::default(),
            credentials,
        }
    }

    #[inline]
    pub fn credential_id(&self) -> &credentials::Id {
        &self.credentials
    }

    /// Drain pending Handle drops and remove them from the local map.
    #[inline]
    fn drain_drops(&self) {
        let mut buf = self.drain_buf.borrow_mut();
        self.drop_channel.drain_into(&mut buf);
        if buf.is_empty() {
            return;
        }
        let mut map = self.map.borrow_mut();
        for stream_id in buf.drain(..) {
            map.remove(&stream_id);
        }
    }

    /// Look up the queue_id associated with a stream_id, if one exists.
    ///
    /// Returns `None` if no flow has been registered for this stream_id, or if
    /// the entry was already dropped.
    #[inline]
    pub fn lookup(&self, stream_id: VarInt) -> Option<VarInt> {
        self.drain_drops();
        self.map.borrow().get(&stream_id).copied()
    }

    #[inline]
    pub fn try_register<Q>(
        &self,
        stream_id: VarInt,
        create_queue: impl FnOnce(Handle) -> (VarInt, Q),
    ) -> Result<Q, VarInt> {
        self.drain_drops();

        match self.map.borrow_mut().entry(stream_id) {
            hash_map::Entry::Occupied(entry) => {
                Err(*entry.get())
            }
            hash_map::Entry::Vacant(entry) => {
                let handle = Handle {
                    stream_id,
                    inner: HandleInner::Server {
                        credential_id: self.credentials,
                        drop_channel: self.drop_channel.clone(),
                    },
                };
                let (queue_id, queue) = create_queue(handle);
                entry.insert(queue_id);
                Ok(queue)
            }
        }
    }
}
