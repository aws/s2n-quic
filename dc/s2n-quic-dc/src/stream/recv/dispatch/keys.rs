// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use ahash::AHasher;
use dashmap::DashMap;
use s2n_quic_core::varint::VarInt;
use std::{
    hash::{BuildHasherDefault, Hash},
    sync::Arc,
};

type Hasher = BuildHasherDefault<AHasher>;

pub struct Keys<Key: 'static> {
    keys: Arc<DashMap<Key, VarInt, Hasher>>,
}

impl<Key: 'static> Clone for Keys<Key> {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            keys: self.keys.clone(),
        }
    }
}

impl<Key: 'static> Keys<Key>
where
    Key: Eq + Hash,
{
    #[inline]
    pub fn new(key_capacity: usize) -> Self {
        let keys = DashMap::with_capacity_and_hasher(key_capacity, Default::default());
        let keys = Arc::new(keys);
        Self { keys }
    }

    #[inline]
    pub fn get(&self, key: &Key) -> Option<VarInt> {
        self.keys.get(key).map(|entry| *entry.value())
    }

    #[inline]
    pub fn insert(&self, key: Key, queue_id: VarInt) {
        self.keys.insert(key, queue_id);
    }

    #[inline]
    pub fn remove(&self, key: &Key) {
        self.keys.remove(key);
    }
}
