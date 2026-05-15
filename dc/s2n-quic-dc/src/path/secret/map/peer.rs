// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{dc, seal, Bidirectional, Credentials, Entry, Id, Map, TransportFeatures};
use crate::path::secret::map::PeerDataAddrs;
use s2n_quic_core::time::Timestamp;
use std::sync::Arc;

pub struct Peer {
    entry: Arc<Entry>,
    map: Map,
}

impl Peer {
    pub(super) fn new(entry: &Arc<Entry>, map: &Map) -> Self {
        Self {
            entry: entry.clone(),
            map: map.clone(),
        }
    }

    #[inline]
    pub fn seal_once(&self) -> (seal::Once, Credentials, dc::ApplicationParams) {
        let (sealer, credentials) = self.entry.uni_sealer();
        (sealer, credentials, self.entry.parameters())
    }

    #[inline]
    pub fn pair(&self, features: &TransportFeatures) -> (Bidirectional, dc::ApplicationParams) {
        let keys = self.entry.bidi_local(features);

        (keys, self.entry.parameters())
    }

    /// Atomically claims the next connection slot for rate limiting.
    ///
    /// Returns the `Timestamp` at which the caller should start sending packets.
    /// This can be used to initialize the transmission wheel's start time rather
    /// than starting at `now`.
    #[inline]
    pub fn next_connection_time(&self, now: Timestamp) -> Timestamp {
        self.entry.next_connection_time(now)
    }

    #[inline]
    pub fn id(&self) -> &Id {
        self.entry.id()
    }

    #[inline]
    pub fn map(&self) -> &Map {
        &self.map
    }

    /// Returns true if the peer's data addresses have been learned via the post-handshake exchange.
    #[inline]
    pub fn peer_data_addrs(&self) -> &PeerDataAddrs {
        self.entry.peer_data_addrs()
    }

    /// Consume the Peer and return the underlying Arc<Entry>.
    ///
    /// This is useful for low-level datagram transmission where you need
    /// direct access to the entry for creating PartialDatagram packets.
    #[inline]
    pub fn into_raw(self) -> Arc<Entry> {
        self.entry
    }
}
