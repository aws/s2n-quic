// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{dc, seal, Bidirectional, Credentials, Entry, Map, TransportFeatures};
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

    #[inline]
    pub fn map(&self) -> &Map {
        &self.map
    }
}
