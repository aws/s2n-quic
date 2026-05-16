// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::credentials::{Credentials, KeyId};
use bitvec::BitArr;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

const WINDOW: usize = 896;

type Seen = BitArr!(for WINDOW);

#[derive(Debug)]
pub struct State {
    // This is the maximum ID we've seen so far. This is sent to peers for when we cannot determine
    // if the packet sent is replayed as it falls outside our replay window. Peers use this
    // information to resynchronize on the latest state.
    max_seen_key_id: AtomicU64,

    seen: Mutex<Seen>,
}

impl super::map::SizeOf for Mutex<Seen> {
    fn size(&self) -> usize {
        // If we don't need drop, it's very likely that this type is fully contained in size_of
        // Self. This simplifies implementing this trait for e.g. std types.
        //
        // Mutex on macOS (at least) has a more expensive, pthread-based impl that allocates. But
        // on Linux there's no extra allocation.
        if cfg!(target_os = "linux") {
            assert!(
                !std::mem::needs_drop::<Self>(),
                "{:?} requires custom SizeOf impl",
                std::any::type_name::<Self>()
            );
        }
        std::mem::size_of::<Self>()
    }
}

impl super::map::SizeOf for State {
    fn size(&self) -> usize {
        let State {
            max_seen_key_id,
            seen,
        } = self;
        max_seen_key_id.size() + seen.size()
    }
}

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

impl State {
    pub fn new() -> State {
        State {
            max_seen_key_id: AtomicU64::new(u64::MAX),
            seen: Default::default(),
        }
    }

    pub fn pre_authentication(&self, _identity: &Credentials) -> Result<(), Error> {
        // TODO: Provide more useful pre-auth checks. For now just don't bother checking this, we
        // can always rely on the post-auth check in practice, this is just a slight optimization.
        Ok(())
    }

    pub fn minimum_unseen_key_id(&self) -> KeyId {
        KeyId::try_from(
            self.max_seen_key_id
                .load(Ordering::Relaxed)
                // Initial u64::MAX wraps to zero, which is the correct answer for the initial
                // state. After that just +1 consistently.
                .wrapping_add(1),
        )
        .unwrap()
    }

    /// Called after decryption has been performed
    pub fn post_authentication(&self, identity: &Credentials) -> Result<(), Error> {
        let mut seen = self.seen.lock().unwrap();

        let key_id = *identity.key_id;
        let mut previous_max = self.max_seen_key_id.load(Ordering::Relaxed);
        let new_max = if previous_max == u64::MAX {
            previous_max = 0;
            key_id
        } else {
            previous_max.max(key_id)
        };
        self.max_seen_key_id.store(new_max, Ordering::Relaxed);

        let delta = new_max - previous_max;
        if delta > seen.len() as u64 {
            // not yet seen since we shifted forward by more than the bitset's size.
            seen.fill(false);
        } else {
            // Even on a 32-bit platform we'd hit the check above (since seen is way smaller than
            // 2^32).
            seen.shift_right(delta as usize);
        }

        let Ok(idx) = usize::try_from(new_max - key_id) else {
            // We'd never store more than usize bits, so treat this as too old as well.
            return Err(Error::Unknown);
        };

        let ret = if let Some(mut entry) = seen.get_mut(idx) {
            if *entry {
                return Err(Error::AlreadyExists);
            }

            entry.set(true);

            Ok(())
        } else {
            // Too old -- no longer in memory.
            return Err(Error::Unknown);
        };

        ret
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
