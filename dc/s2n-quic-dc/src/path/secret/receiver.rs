// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use self::allocator::Allocator;
use crate::credentials::{Credentials, KeyId};
use std::alloc::Layout;
use std::sync::{Arc, Mutex};

mod allocator;

#[derive(Debug)]
pub struct Shared {
    alloc: Allocator,
    entries: Mutex<slab::Slab<InnerState>>,
}

unsafe impl Send for Shared {}
unsafe impl Sync for Shared {}

impl Shared {
    pub fn without_region() -> Arc<Shared> {
        Arc::new(Shared {
            alloc: Allocator::with_capacity(0),
            entries: Mutex::new(slab::Slab::new()),
        })
    }

    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
            // ~20MB
            alloc: Allocator::with_capacity(20 * 1024 * 1024),
            entries: Mutex::new(slab::Slab::new()),
        })
    }

    pub fn new_receiver(self: Arc<Self>) -> State {
        let mut guard = self.entries.lock().unwrap();
        let key = guard.insert(InnerState::new());
        State {
            shared: self.clone(),
            entry: key,
        }
    }

    fn remove(&self, entry: usize) -> InnerState {
        let mut guard = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(entry)
    }
}

#[derive(Debug)]
pub struct State {
    // FIXME: Avoid storing Shared pointer inside every path secret entry.
    // Instead thread the pointer through all the methods.
    shared: Arc<Shared>,
    // FIXME: shrink to u32 index?
    entry: usize,
}

impl Drop for State {
    fn drop(&mut self) {
        let entry = self.shared.remove(self.entry);
        if let SharedIndex::Bitset(handle) | SharedIndex::Array(handle) = entry.shared.unpack() {
            // SAFETY: Entry is being dropped, so this is called at most once.
            unsafe { self.shared.alloc.deallocate(handle) };
        }
    }
}

// KeyIDs move through two filters:
//
// * `max_seen` + bitset absorbs traffic with minimal reordering. Conceptually they are a single
//   33-bit bitset ending at (inclusively) `max_seen`. 1-bits indicate seen entries. This is
//   currently expected to be enough to absorb the vast majority (>99.99%) of traffic seen in
//   practice. This space is always available to every Path Secret.
// * If we don't see a key ID (i.e., we shift out a zero bit from the bitset) we insert into a list
//   or bitset within the Shared state. This list tracks *only* unseen entries, so we expect it to
//   generally be short. Currently the list can track entries within a region 2**16 wide. Note that
//   this region is independent of `max_seen` and so only needs to potentially be changed if we
//   evict a zero bit (which happens pretty rarely), and even then only if we still haven't caught
//   a packet that's 2**16 old. See more details on `SortedListHeader` and `BitsetHeader`.
#[derive(Clone, Debug)]
pub struct InnerState {
    max_seen: u64,

    // Directly stored bitset, adjacent to max_seen.
    bitset: u32,

    // Any key ID > to this is either AlreadyExists or Ok.
    // Note that == is Unknown, since += 1 is *not* a safe operation.
    //
    // This is updated when we evict from the list/bitset (i.e., drop a still-Ok value).
    // FIXME: actually not updated today, because we need to thread this into deallocation for
    // proper updates.
    minimum_evicted: u64,

    // Index into the shared allocator's parents/entry array, if any.
    shared: SharedIndexMemory,

    // FIXME: Move into shared allocation.
    list: Vec<u64>,
}

// "u24" indices keep the in-memory size down.
#[derive(Copy, Clone)]
enum SharedIndexMemory {
    None,
    Array([u8; 3]),
    Bitset([u8; 3]),
}

impl std::fmt::Debug for SharedIndexMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.unpack().fmt(f)
    }
}

#[derive(Debug, Copy, Clone)]
enum SharedIndex {
    None,
    Array(usize),
    Bitset(usize),
}

impl SharedIndexMemory {
    fn unpack(self) -> SharedIndex {
        match self {
            SharedIndexMemory::None => SharedIndex::None,
            SharedIndexMemory::Array([a, b, c]) => {
                SharedIndex::Array(u32::from_le_bytes([0, a, b, c]) as usize)
            }
            SharedIndexMemory::Bitset([a, b, c]) => {
                SharedIndex::Bitset(u32::from_le_bytes([0, a, b, c]) as usize)
            }
        }
    }
}

impl SharedIndex {
    fn pack(self) -> SharedIndexMemory {
        match self {
            SharedIndex::None => SharedIndexMemory::None,
            SharedIndex::Array(i) => {
                assert!(i < (1 << 24));
                let [a, b, c, d] = (i as u32).to_le_bytes();
                assert!(a == 0);
                SharedIndexMemory::Array([b, c, d])
            }
            SharedIndex::Bitset(i) => {
                assert!(i < (1 << 24));
                let [a, b, c, d] = (i as u32).to_le_bytes();
                assert!(a == 0);
                SharedIndexMemory::Bitset([b, c, d])
            }
        }
    }
}

impl InnerState {
    fn new() -> Self {
        Self {
            max_seen: u64::MAX,
            minimum_evicted: u64::MAX,
            bitset: 0,
            shared: SharedIndexMemory::None,

            list: vec![],
        }
    }

    // Iterate over the unseen IDs that were > previous max seen, and
    // will not *become* tracked now (i.e., don't fall into the new bitset).
    //
    // The bitset tracks (max_seen-32)..=(max_seen-1)
    fn skipped_bitset(&self, previous_max: Option<u64>) -> std::ops::Range<u64> {
        let end = self.max_seen.saturating_sub(u32::BITS as u64);
        // Push start up so we don't push more than 65k elements, which is our list limit.
        // This avoids a too-long loop if we jump forward too much.
        let start = match previous_max {
            Some(previous_max) => (previous_max + 1).max(end.saturating_sub(u16::MAX as u64)),
            None => end.saturating_sub(u16::MAX as u64),
        };
        start..end
    }
}

impl State {
    pub fn without_shared() -> State {
        let shared = Shared::without_region();
        shared.new_receiver()
    }

    #[cfg(test)]
    pub fn snapshot(&self) -> InnerState {
        self.shared.entries.lock().unwrap()[self.entry].clone()
    }

    pub fn with_shared(shared: Arc<Shared>) -> State {
        shared.new_receiver()
    }

    pub fn minimum_unseen_key_id(&self) -> KeyId {
        // wrapping_add ensures that our sentinel u64::MAX is zero, which is accurate (i.e., if we
        // have not seen any keys, then we have not seen the zeroth key either).
        KeyId::new(
            self.shared.entries.lock().unwrap()[self.entry]
                .max_seen
                .wrapping_add(1),
        )
        .unwrap()
    }

    pub fn pre_authentication(&self, _credentials: &Credentials) -> Result<(), Error> {
        // always pass for now
        Ok(())
    }

    pub fn post_authentication(&self, credentials: &Credentials) -> Result<(), Error> {
        let entry = &mut self.shared.entries.lock().unwrap()[self.entry];

        if entry.max_seen == u64::MAX {
            // no need to touch the bitset, we've not seen any of the previous entries.
            entry.max_seen = *credentials.key_id;

            for id in entry.skipped_bitset(None) {
                self.push_list(entry, id);
            }

            Ok(())
        } else if credentials.key_id > entry.max_seen {
            let previous_max = entry.max_seen;
            entry.max_seen = *credentials.key_id;
            let delta = entry.max_seen - previous_max;

            // This is the range that is going to get shifted out.
            //
            // Any bit not set means we haven't yet seen it, so we should add it to our list.
            //
            // If we shifted by 1, then the range we want is 31..=31 (1 bit, 1 << 31, top bit)
            // If we shifted by 2, then the range we want is 30..=31 (2 bits)
            // If we shifted by 30, then the range we want is 2..=31 (30 bits)
            // If we shifted by 60, then the range we want is 0..=31 (all 32 bits)
            for bit in (32u64.saturating_sub(delta)..=31).rev() {
                // +1 since bit 0 is previous_max - 1
                let Some(id) = previous_max.checked_sub(bit + 1) else {
                    continue;
                };
                if entry.bitset & (1 << bit) == 0 {
                    self.push_list(entry, id);
                }
            }

            for id in entry.skipped_bitset(Some(previous_max)) {
                self.push_list(entry, id);
            }

            if delta <= u32::BITS as u64 {
                // as u32 is safe since we checked we're less than 32.
                let delta = delta as u32;

                // Shift the no longer fitting bits out
                // 0s mean we have *not* seen the entry, so shifting those in for the middle part
                entry.bitset = entry.bitset.checked_shl(delta).unwrap_or(0);
                // Set the bit corresponding to previously max-seen.
                entry.bitset |= 1 << (delta - 1);
            } else {
                entry.bitset = 0;
            }

            // forward shift is always successful
            Ok(())
        } else if credentials.key_id == entry.max_seen {
            Err(Error::AlreadyExists)
        } else {
            let delta = entry.max_seen - *credentials.key_id;
            if delta <= u32::BITS as u64 {
                // -1 for the transition from max seen to the bitset
                if (entry.bitset & (1 << (delta - 1) as u32)) != 0 {
                    Err(Error::AlreadyExists)
                } else {
                    entry.bitset |= 1 << (delta - 1) as u32;
                    Ok(())
                }
            } else if let Ok(()) = self.try_remove_list(entry, *credentials.key_id) {
                Ok(())
            } else if *credentials.key_id > entry.minimum_evicted {
                Err(Error::AlreadyExists)
            } else {
                Err(Error::Unknown)
            }
        }
    }

    fn push_list(&self, entry: &mut InnerState, id: u64) {
        entry.list.push(id);
    }

    fn try_remove_list(&self, entry: &mut InnerState, id: u64) -> Result<(), ()> {
        if let Ok(idx) = entry.list.binary_search(&id) {
            // FIXME: augment with bitset for fast removal
            entry.list.remove(idx);
            Ok(())
        } else {
            Err(())
        }
    }
}

impl super::map::SizeOf for State {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// This indicates that we know about this element and it *definitely* already exists.
    #[error("packet definitely already seen before")]
    AlreadyExists,
    /// We don't know whether we've seen this element before. It may or may not have already been
    /// received.
    #[error("packet may have been seen before")]
    Unknown,
}

#[cfg(test)]
mod tests;
