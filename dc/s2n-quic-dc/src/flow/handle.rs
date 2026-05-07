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
    /// Tracker for cleanup on drop
    tracker: Tracker,
}

impl Handle {
    pub fn credential_id(&self) -> credentials::Id {
        self.tracker.0.credentials
    }

    pub fn stream_id(&self) -> VarInt {
        self.stream_id
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.tracker.0.map.remove(&self.stream_id);
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
    fn validate(&self, params: &Self::Request) -> bool {
        self.tracker.0.credentials == params.credential_id && self.stream_id == params.stream_id
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
                let handle = Handle {
                    stream_id,
                    tracker: self.clone(),
                };
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
