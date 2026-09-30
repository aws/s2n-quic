// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{dc, seal, Bidirectional, Credentials, Entry, Id, Map, TransportFeatures};
use crate::psk::io::HandshakeReason;
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

    /// Returns `None` if this peer's path secret has exhausted its key IDs, having requested a
    /// re-handshake.
    #[inline]
    pub fn seal_once(&self) -> Option<(seal::Once, Credentials, dc::ApplicationParams)> {
        let Some((sealer, credentials)) = self.entry.uni_sealer() else {
            // Key ID has been exhausted. Therefore, initiate another handshake.
            self.map
                .store
                .request_handshake(*self.entry.peer(), HandshakeReason::Remote);
            return None;
        };

        Some((sealer, credentials, self.entry.parameters()))
    }

    /// Returns `None` if this peer's path secret has exhausted its key IDs, having requested a
    /// re-handshake.
    #[inline]
    pub fn pair(
        &self,
        features: &TransportFeatures,
    ) -> Option<(Bidirectional, dc::ApplicationParams)> {
        let Some(keys) = self.entry.bidi_local(features) else {
            // Key ID has been exhausted. Therefore, initiate another handshake.
            self.map
                .store
                .request_handshake(*self.entry.peer(), HandshakeReason::Remote);
            return None;
        };

        Some((keys, self.entry.parameters()))
    }

    #[inline]
    pub fn id(&self) -> &Id {
        self.entry.id()
    }

    #[inline]
    pub fn map(&self) -> &Map {
        &self.map
    }
}
