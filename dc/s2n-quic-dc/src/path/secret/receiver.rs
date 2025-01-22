// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::sync::{Arc, Mutex};

use crate::credentials::{Credentials, KeyId};

#[derive(Debug)]
pub struct Shared {
    // FIXME: Improve scalability by avoiding the global mutex.
    // Most likely strategy is something like fixed-size which (in principle) allows per-entry
    // Mutex's. Likely means dropping the slab dependency.
    entries: Mutex<slab::Slab<InnerState>>,
}

impl Shared {
    pub fn new() -> Arc<Shared> {
        Arc::new(Shared {
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

    fn remove(&self, entry: usize) {
        let mut guard = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(entry);
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
        self.shared.remove(self.entry);
    }
}

#[derive(Clone, Debug)]
pub struct InnerState {
    max_seen: u64,
    bitset: u32,
    // FIXME: simple, later move into shared memory.
    list: Vec<u64>,
}

impl InnerState {
    fn new() -> Self {
        Self {
            max_seen: u64::MAX,
            bitset: 0,
            list: Vec::new(),
        }
    }
}

impl State {
    pub fn without_shared() -> State {
        let mut entries = Mutex::new(slab::Slab::with_capacity(1));
        entries.get_mut().unwrap().insert(InnerState::new());
        State {
            shared: Arc::new(Shared { entries }),
            entry: 0,
        }
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

            // Iterate over the unseen IDs that were > previous max seen, and
            // will not *become* tracked now (i.e., don't fall into the new bitset).
            //
            // The bitset tracks (max_seen-32)..=(max_seen-1)
            let end = entry.max_seen.saturating_sub(u32::BITS as u64);
            // Push start up so we don't push more than 65k elements, which is our list limit.
            // This avoids a too-long loop if we jump forward too much.
            let start = end.saturating_sub(u16::MAX as u64);
            for id in start..end {
                entry.list.push(id);
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
                    entry.list.push(id);
                }
            }

            // Iterate over the unseen IDs that were > previous max seen, and
            // will not *become* tracked now (i.e., don't fall into the new bitset).
            //
            // The bitset tracks (max_seen-32)..=(max_seen-1)
            let end = entry.max_seen.saturating_sub(u32::BITS as u64);
            // Push start up so we don't push more than 65k elements, which is our list limit.
            // This avoids a too-long loop if we jump forward too much.
            let start = (previous_max + 1).max(end.saturating_sub(u16::MAX as u64));
            for id in start..end {
                entry.list.push(id);
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
            } else if let Ok(idx) = entry.list.binary_search(&*credentials.key_id) {
                // FIXME: augment with bitset for fast removal
                entry.list.remove(idx);
                Ok(())
            } else {
                Err(Error::Unknown)
            }
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
