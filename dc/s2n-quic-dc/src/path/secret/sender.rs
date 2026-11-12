// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::schedule;
use crate::{crypto::awslc::open, packet::secret_control};
use s2n_quic_core::varint::VarInt;
use std::sync::atomic::{AtomicU64, Ordering};

type StatelessReset = [u8; secret_control::TAG_LEN];

/// Why restoring a [`State`] from a persisted sender counter failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RestoreError {
    /// Advancing the persisted counter would exceed the `VarInt` maximum, so no safe key id remains
    /// for this entry. Practically unreachable -- it needs a persisted counter within one advance of
    /// 2^62 -- but restoring it verbatim would risk key-id reuse, so the entry is rejected instead.
    #[error("restored sender counter would exceed the maximum key id")]
    CounterOverflow,
}

#[derive(Debug)]
pub struct State {
    current_id: AtomicU64,
    pub(super) stateless_reset: StatelessReset,
}

impl super::map::SizeOf for StatelessReset {}

impl super::map::SizeOf for State {
    fn size(&self) -> usize {
        let State {
            current_id,
            stateless_reset,
        } = self;
        current_id.size() + stateless_reset.size()
    }
}

impl State {
    pub fn new(stateless_reset: StatelessReset) -> Self {
        Self {
            current_id: AtomicU64::new(0),
            stateless_reset,
        }
    }

    /// Rebuilds a sender from a persisted counter, advancing it by `advance` so no key id is reissued.
    ///
    /// `persisted_current_id` is the counter value observed when the entry was written (see
    /// [`State::current_id`]); the live counter may have moved past it before the write, and a crash
    /// loses any issuance since. `advance` is applied verbatim and must cover that gap: a sender
    /// restored with too small an advance reissues key ids, and reissuing a key id reuses the
    /// `(key, iv)` pair it derives. A caller restoring a sender that will issue applies the
    /// `MIN_RESTORE_ADVANCE` minimum the persistence codec defines; only inspection of what a file
    /// holds passes less.
    ///
    /// The result must remain a usable key id: the sender always keeps room to add one (see
    /// [`State::next_key_id`]), so the ceiling is `VarInt::MAX - 1`, and a value that would cross it
    /// is rejected with [`RestoreError::CounterOverflow`] rather than silently wrapped.
    pub fn restore(
        persisted_current_id: u64,
        stateless_reset: StatelessReset,
        advance: u64,
    ) -> Result<Self, RestoreError> {
        let advanced = persisted_current_id
            .checked_add(advance)
            .filter(|id| *id < *VarInt::MAX)
            .ok_or(RestoreError::CounterOverflow)?;

        Ok(Self {
            current_id: AtomicU64::new(advanced),
            stateless_reset,
        })
    }

    /// The next key id this sender would issue, as a plain `u64`, for persistence.
    ///
    /// This is the live counter, not advanced by anything: the advance is applied on the way back
    /// in by [`State::restore`], so a persisted file records the truth and the advance can change
    /// without invalidating existing files.
    pub fn current_id(&self) -> u64 {
        self.current_id.load(Ordering::Relaxed)
    }

    /// Returns the next key ID to send at, or `None` if available key id is exhausted.
    pub fn next_key_id(&self) -> Option<VarInt> {
        let id = self
            .current_id
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                VarInt::try_from(current + 1)
                    .ok()
                    // Make sure we can always +1. This is a useful property for StaleKey packets
                    // which send a minimum *not yet seen* ID. In practice it shouldn't matter
                    // since we are assuming we can't hit 2^62, but this helps localize handling
                    // that edge to this code.
                    .filter(|id| *id != VarInt::MAX)
                    .map(|id| *id)
            })
            // `try_update` reports `Err(current)` when the closure declined to advance such as when
            // there is no usable ID left in the VarInt range.
            .ok()?;

        VarInt::try_from(id).ok()
    }

    #[inline]
    pub fn control_secret(&self, secret: &schedule::Secret) -> open::control::Secret {
        // We don't try to cache this, hmac init is cheap (~200-600ns depending on algorithm) and
        // the space requirement is huge (700+ bytes)
        secret.control_opener()
    }

    /// Update the sender for a received stale key packet.
    ///
    /// This increments the current ID we are sending at to at least the ID provided in the packet.
    ///
    /// Note that this packet can be replayed without detection, we must deal with authenticated
    /// but arbitrarily old IDs here. In the future we may want to guard against advancing too
    /// quickly (e.g., due to bit flips), but for now we ignore that problem.
    pub(super) fn update_for_stale_key(&self, min_key_id: VarInt) {
        // Update the key to the new minimum to start at.
        self.current_id.fetch_max(*min_key_id, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub fn reset_counter(&self) {
        self.current_id.store(0, Ordering::Relaxed);
    }
}

#[test]
fn sender_does_not_wrap() {
    let state = State::new([0; secret_control::TAG_LEN]);
    assert_eq!(*state.next_key_id().unwrap(), 0);

    state.current_id.store((1 << 62) - 3, Ordering::Relaxed);

    // The last usable ID: handing it out leaves the counter at the reserved `VarInt::MAX - 1`,
    // which can still represent "one past the last ID" for StaleKey packets.
    assert_eq!(*state.next_key_id().unwrap(), (1 << 62) - 3);

    // Advancing again would have to store `VarInt::MAX`, so we report exhaustion instead of wrapping
    // or panicking, and keep reporting it while leaving the counter where it is.
    assert_eq!(state.next_key_id(), None);
    assert_eq!(state.next_key_id(), None);
    assert_eq!(state.current_id.load(Ordering::Relaxed), (1 << 62) - 2);
}

#[test]
fn update_restarts_sequence() {
    let state = State::new([0; secret_control::TAG_LEN]);
    assert_eq!(*state.next_key_id().unwrap(), 0);

    state.update_for_stale_key(VarInt::new(3).unwrap());

    // Update should start at the minimum trusted key ID on the other side.
    assert_eq!(*state.next_key_id().unwrap(), 3);
}

#[test]
fn update_for_stale_key_to_max_is_not_fatal() {
    // A StaleKey packet carries an authenticated but otherwise arbitrary minimum key ID, so the peer
    // can push the counter to the very top of the VarInt range. Sending afterwards must report
    // exhaustion rather than panicking.
    let max = *VarInt::MAX;
    for min_key_id in [max, max - 1] {
        let state = State::new([0; secret_control::TAG_LEN]);
        state.update_for_stale_key(VarInt::new(min_key_id).unwrap());

        assert_eq!(state.next_key_id(), None);
    }

    // Two below the top is still usable, and is the last ID we ever hand out.
    let state = State::new([0; secret_control::TAG_LEN]);
    state.update_for_stale_key(VarInt::new(max - 2).unwrap());
    assert_eq!(*state.next_key_id().unwrap(), max - 2);
    assert_eq!(state.next_key_id(), None);
}
