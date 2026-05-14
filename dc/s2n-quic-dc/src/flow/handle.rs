// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Flow handle with validation support

use crate::credentials;
use s2n_quic_core::varint::VarInt;

/// Flow handle that validates credentials and stream_id
///
/// This is used as the Key type in the queue system to ensure that
/// packets routed to a flow actually belong to that flow.
#[derive(Debug, Clone)]
pub struct Handle {
    /// Global stream identifier (client-wide)
    stream_id: VarInt,
    /// Inner state (server-side with tracker, or client-side with path entry)
    inner: HandleInner,
}

#[derive(Clone)]
enum HandleInner {
    /// Server-side handle with tracker for deduplication
    Server { tracker: Tracker },
    /// Client-side handle with path secret entry
    Client {
        path_entry: std::sync::Arc<crate::path::secret::map::Entry>,
    },
}

impl core::fmt::Debug for HandleInner {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Server { tracker } => f.debug_struct("Server").field("tracker", tracker).finish(),
            Self::Client { .. } => f.debug_struct("Client").finish_non_exhaustive(),
        }
    }
}

impl HandleInner {
    fn credential_id(&self) -> &credentials::Id {
        match self {
            Self::Server { tracker } => &tracker.0.credentials,
            Self::Client { path_entry } => path_entry.id(),
        }
    }
}

impl Handle {
    /// Create a client-side handle with path secret entry
    ///
    /// Client-side handles don't need trackers since the client doesn't do deduplication.
    pub fn client(
        stream_id: VarInt,
        path_entry: std::sync::Arc<crate::path::secret::map::Entry>,
    ) -> Self {
        Self {
            stream_id,
            inner: HandleInner::Client { path_entry },
        }
    }

    /// Create a server-side handle with tracker (internal use only)
    pub(super) fn server(stream_id: VarInt, tracker: Tracker) -> Self {
        Self {
            stream_id,
            inner: HandleInner::Server { tracker },
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
        // Only clean up tracker on server side
        if let HandleInner::Server { tracker } = &self.inner {
            tracker.0.map.remove(&self.stream_id);
        }
    }
}

/// Request parameters for flow validation
#[derive(Debug, Clone, Copy)]
pub struct Request {
    pub credential_id: credentials::Id,
    pub stream_id: VarInt,
}

impl crate::flow::queue::Key for Handle {
    type Request = Request;

    #[inline]
    fn validate(&self, params: &Self::Request) -> Result<(), crate::flow::queue::ValidationError> {
        use crate::flow::queue::ValidationError;
        if self.credential_id() != &params.credential_id {
            return Err(ValidationError::CredentialMismatch);
        }
        if self.stream_id != params.stream_id {
            return Err(ValidationError::StreamIdMismatch);
        }
        Ok(())
    }

}

/// Tracker for managing flow lifecycle
///
/// Shared between the handle and the sender state to remove flows on drop.
#[derive(Clone)]
pub struct Tracker(std::sync::Arc<TrackerInner>);

impl core::fmt::Debug for Tracker {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Tracker")
            .field("credentials", &self.0.credentials)
            .finish_non_exhaustive()
    }
}

impl Tracker {
    pub fn new(credentials: credentials::Id) -> Self {
        Self(std::sync::Arc::new(TrackerInner {
            map: dashmap::DashMap::new(),
            credentials,
        }))
    }

    pub fn try_register<Q>(
        &self,
        stream_id: VarInt,
        create_queue: impl FnOnce(Handle) -> (VarInt, Q),
    ) -> Result<Q, VarInt> {
        match self.0.map.entry(stream_id) {
            dashmap::mapref::entry::Entry::Occupied(local_queue_id) => Err(*local_queue_id.get()),
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                let handle = Handle::server(stream_id, self.clone());
                let (queue_id, queue) = create_queue(handle);
                entry.insert(queue_id);
                Ok(queue)
            }
        }
    }
}

struct TrackerInner {
    map: dashmap::DashMap<VarInt, VarInt>,
    credentials: credentials::Id,
}
