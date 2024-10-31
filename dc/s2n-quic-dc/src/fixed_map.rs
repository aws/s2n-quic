// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! A fixed-allocation concurrent HashMap.
//!
//! This implements a concurrent map backed by a fixed-size allocation created at construction
//! time, with a fixed memory footprint. The expectation is that all storage is inline (to the
//! extent possible) reducing the likelihood.

use core::{
    fmt::Debug,
    hash::Hash,
    sync::atomic::{AtomicU8, Ordering},
};
use parking_lot::{RwLock, RwLockReadGuard, RwLockUpgradableReadGuard};
use std::{collections::hash_map::RandomState, hash::BuildHasher};

pub use parking_lot::MappedRwLockReadGuard as ReadGuard;

pub struct Map<K, V, S = RandomState> {
    slots: Box<[Slot<K, V>]>,
    hash_builder: S,
}

impl<K, V, S> Map<K, V, S>
where
    K: Hash + Eq + Debug,
    S: BuildHasher,
{
    pub fn with_capacity(entries: usize, hasher: S) -> Self {
        let slots = std::cmp::max(1, (entries + SLOT_CAPACITY) / SLOT_CAPACITY).next_power_of_two();
        let map = Map {
            slots: (0..slots)
                .map(|_| Slot::new())
                .collect::<Vec<_>>()
                .into_boxed_slice(),
            hash_builder: hasher,
        };
        assert!(map.slots.len().is_power_of_two());
        assert!(u32::try_from(map.slots.len()).is_ok());
        map
    }

    pub fn clear(&self) {
        for slot in self.slots.iter() {
            slot.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.slots.iter().map(|s| s.len()).sum()
    }

    // can't lend references to values outside of a lock, so Iterator interface doesn't work
    #[allow(unused)]
    pub fn iter(&self, mut f: impl FnMut(&K, &V)) {
        for slot in self.slots.iter() {
            // this feels more readable than flatten
            #[allow(clippy::manual_flatten)]
            for entry in slot.values.read().iter() {
                if let Some(v) = entry {
                    f(&v.0, &v.1);
                }
            }
        }
    }

    pub fn retain(&self, mut f: impl FnMut(&K, &V) -> bool) {
        for slot in self.slots.iter() {
            // this feels more readable than flatten
            #[allow(clippy::manual_flatten)]
            for entry in slot.values.write().iter_mut() {
                if let Some(v) = entry {
                    if !f(&v.0, &v.1) {
                        *entry = None;
                    }
                }
            }
        }
    }

    fn slot_by_hash(&self, key: &K) -> &Slot<K, V> {
        let hash = self.hash_builder.hash_one(key);
        // needed for bit-and modulus, checked in new as a non-debug assert!.
        debug_assert!(self.slots.len().is_power_of_two());
        let slot_idx = hash as usize & (self.slots.len() - 1);
        &self.slots[slot_idx]
    }

    /// Returns Some(v) if overwriting a previous value for the same key.
    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.slot_by_hash(&key).put(key, value)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.get_by_key(key).is_some()
    }

    pub fn get_by_key(&self, key: &K) -> Option<ReadGuard<'_, V>> {
        self.slot_by_hash(key).get_by_key(key)
    }
}

// Balance of speed of access (put or get) and likelihood of false positive eviction.
const SLOT_CAPACITY: usize = 32;

struct Slot<K, V> {
    next_write: AtomicU8,
    values: RwLock<[Option<(K, V)>; SLOT_CAPACITY]>,
}

impl<K, V> Slot<K, V>
where
    K: Hash + Eq + Debug,
{
    fn new() -> Self {
        Slot {
            next_write: AtomicU8::new(0),
            values: RwLock::new(std::array::from_fn(|_| None)),
        }
    }

    fn clear(&self) {
        *self.values.write() = std::array::from_fn(|_| None);
    }

    /// Returns Some(v) if overwriting a previous value for the same key.
    fn put(&self, new_key: K, new_value: V) -> Option<V> {
        let values = self.values.upgradable_read();
        for (value_idx, value) in values.iter().enumerate() {
            // overwrite if same key or if no key/value pair yet
            if value.as_ref().map_or(true, |(k, _)| *k == new_key) {
                let mut values = RwLockUpgradableReadGuard::upgrade(values);
                let old = values[value_idx].take().map(|v| v.1);
                values[value_idx] = Some((new_key, new_value));
                return old;
            }
        }

        let mut values = RwLockUpgradableReadGuard::upgrade(values);

        // If `new_key` isn't already in this slot, replace one of the existing entries with the
        // new key. For now we rotate through based on `next_write`.
        let replacement = self.next_write.fetch_add(1, Ordering::Relaxed) as usize % SLOT_CAPACITY;
        tracing::trace!(
            "evicting {:?} - bucket overflow",
            values[replacement].as_mut().unwrap().0
        );
        values[replacement] = Some((new_key, new_value));
        None
    }

    fn get_by_key(&self, needle: &K) -> Option<ReadGuard<'_, V>> {
        // Scan each value and check if our requested needle is present.
        let values = self.values.read();
        for (value_idx, value) in values.iter().enumerate() {
            if value.as_ref().map_or(false, |(k, _)| *k == *needle) {
                return Some(RwLockReadGuard::map(values, |values| {
                    &values[value_idx].as_ref().unwrap().1
                }));
            }
        }

        None
    }

    fn len(&self) -> usize {
        let values = self.values.read();
        let mut len = 0;
        for value in values.iter().enumerate() {
            len += value.1.is_some() as usize;
        }
        len
    }
}

#[cfg(test)]
mod test;
