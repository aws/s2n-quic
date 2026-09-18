// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::credentials::{Credentials, KeyId};
use bitvec::BitArr;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

const WINDOW: usize = 896;

/// The minimum distance the receiver's maximum-seen key id is advanced when an entry is restored
/// from disk.
///
/// This is distinct from `WINDOW`: `WINDOW` is the width of the replay bitmap and the *minimum* jump
/// needed for bitmap-vs-maximum consistency, but it is nowhere near enough to cover the unpersisted
/// gap. Like the sender's floor this is applied under `max(advance, floor)`, so a too-small caller
/// `advance` cannot cause replay acceptance.
///
/// The value is 1M keys/sec * 5 days: a deliberately implausible sustained rate times a generous
/// inter-restart window, so the floor alone covers any plausible gap between the last write and a
/// restart even when the caller's advance is zero.
const MIN_RESTORE_ADVANCE: u64 = 1_000_000 * 5 * 24 * 60 * 60;

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

/// Why restoring a [`State`] from a persisted maximum-seen key id failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RestoreError {
    /// The persisted value was the `u64::MAX` sentinel that denotes "nothing seen yet". A
    /// well-formed file records that state as an absent maximum, not as the sentinel, so seeing it
    /// here means the record is corrupt.
    #[error("restored receiver key id was the reserved sentinel value")]
    Sentinel,

    /// Advancing the persisted maximum by the replay-window width would overflow `u64`. Practically
    /// unreachable, but rejected rather than wrapped so a restored receiver can never end up with a
    /// maximum below an id it had already accepted.
    #[error("restored receiver key id would overflow")]
    Overflow,
}

impl State {
    pub fn new() -> State {
        State {
            max_seen_key_id: AtomicU64::new(u64::MAX),
            seen: Default::default(),
        }
    }

    /// The highest key id this receiver has accepted, or `None` if it has accepted none.
    ///
    /// The internal representation uses `u64::MAX` as the "nothing seen yet" sentinel (a freshly
    /// constructed receiver via [`State::new`]). That sentinel is mapped to `None` here rather than
    /// exposed, so a caller persisting the value never mistakes it for a real, astronomically large
    /// key id.
    pub fn max_seen_key_id(&self) -> Option<u64> {
        match self.max_seen_key_id.load(Ordering::Relaxed) {
            u64::MAX => None,
            seen => Some(seen),
        }
    }

    /// Rebuilds a receiver from a persisted maximum-seen key id, advanced so no prior id is
    /// accepted again.
    ///
    /// The replay bitmap is indexed *relative* to the maximum (`idx = new_max - key_id`), so a
    /// receiver restored with a maximum at or below one it had already reached would treat an
    /// already-accepted key id as unseen and accept the replay. The maximum is therefore advanced
    /// past the persisted value by `max(advance, MIN_RESTORE_ADVANCE)`, and the bitmap starts empty.
    /// Any id at or below the persisted maximum then lands well outside the bitmap and is rejected as
    /// too old, which is the safe answer for a packet the pre-restart instance may already have
    /// accepted.
    ///
    /// `advance` is the caller's staleness estimate, derived from how old the persisted file is (a
    /// stale file may have accepted many more ids than it managed to persist). The floor
    /// [`MIN_RESTORE_ADVANCE`]  must cover that unpersisted gap -- not merely the `WINDOW`
    /// bitmap width, which is too small -- and matches the sender's floor for the same reason. It is
    /// applied to whatever the caller passes, so a too-small `advance` cannot cause replay acceptance.
    ///
    /// The cost is that the first few genuine packets from a peer, whose ids sit just above the old
    /// maximum but below the advanced one, are also rejected until the peer resynchronises. This is
    /// addressed by the "Peer_HI" mechanism.
    ///
    /// `persisted_max_seen` is the value returned by [`State::max_seen_key_id`]. The `WINDOW`
    /// constant is private to this module and applied here, next to the invariant it protects, so a
    /// caller cannot get it wrong.
    pub fn restore(persisted_max_seen: u64, advance: u64) -> Result<State, RestoreError> {
        // `u64::MAX` is the sentinel `new()` uses for "nothing seen". A persisted file should carry
        // `None` for that state and never reach here, so treat an explicit sentinel as corrupt
        // rather than advancing it (which would wrap).
        if persisted_max_seen == u64::MAX {
            return Err(RestoreError::Sentinel);
        }

        let advanced = persisted_max_seen
            .checked_add(advance.max(MIN_RESTORE_ADVANCE))
            // Leave the sentinel value itself unreachable, so a restored receiver is never confused
            // for a fresh one.
            .filter(|max| *max != u64::MAX)
            .ok_or(RestoreError::Overflow)?;

        Ok(State {
            max_seen_key_id: AtomicU64::new(advanced),
            seen: Default::default(),
        })
    }

    pub fn pre_authentication(&self, identity: &Credentials) -> Result<(), Error> {
        // Bail if we get the max key ID. This is not practically reachable on well-behaved senders
        // (see sender.rs for comments), and lets us always return a valid KeyId from
        // `minimum_unseen_key_id` even with non well-behaved peers.
        if identity.key_id == KeyId::MAX {
            return Err(Error::Unknown);
        }

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
        .unwrap_or(
            // Saturate if we've exhausted the key ID space. Should be unreachable in practice due
            // to the pre_authentication check above, but avoid a panic by handling it here too.
            KeyId::MAX,
        )
    }

    /// Called after decryption has been performed
    #[expect(
        clippy::unwrap_used,
        clippy::unwrap_in_result,
        reason = "lock is only poisoned if another thread already panicked while holding it"
    )]
    pub fn post_authentication(&self, identity: &Credentials) -> Result<(), Error> {
        // Duplicate since it's cheap right now, can be refined in the future.
        // In practice callers should have already run this early in the receiving process.
        self.pre_authentication(identity)?;

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
            seen.shift_end(delta as usize);
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
