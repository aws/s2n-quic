// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::schedule;
use crate::{crypto::awslc::open, packet::secret_control};
use s2n_quic_core::varint::VarInt;
use std::sync::atomic::{AtomicU64, Ordering};

type StatelessReset = [u8; secret_control::TAG_LEN];

/// The minimum distance the sender's key id is advanced when an entry is restored from disk.
///
/// Key derivation is deterministic: `derive_application_key` derives the AEAD key *and* the IV
/// from `(export_secret, key_id)` with no random nonce, so reusing a key id reuses the
/// `(key, iv)` pair. For AES-GCM that leaks the xor of the two plaintexts and exposes the GHASH
/// authentication key, permitting forgery. A restored sender must therefore never reissue a key
/// id the pre-restart instance may already have used.
///
/// The persisted value is the id the sender was *about to* use, but a write can lag the live
/// counter, and a host crash loses any issuance since the last write. Advancing by 2^32 clears any
/// plausible gap: at a pessimistic 10k streams/sec/peer it covers roughly five days of issuance,
/// while consuming only 2^-30 of the 2^62 `VarInt` budget.
///
/// This is a *floor*, not the whole story: the caller supplies an `advance` derived from how stale
/// the persisted file is, and [`State::restore`] applies whichever is larger. The floor lives here,
/// next to the counter it protects, so a caller that passes too small an `advance` -- or zero --
/// cannot defeat the key-schedule invariant (see the plan's C5).
const MIN_RESTORE_ADVANCE: u64 = 1 << 32;

/// Why restoring a [`State`] from a persisted sender counter failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RestoreError {
    /// Advancing the persisted counter would exceed the `VarInt` maximum, so no safe key id remains
    /// for this entry. Practically unreachable at the 2^32 floor -- it needs a persisted counter
    /// within 2^32 of 2^62 -- but restoring it verbatim would risk key-id reuse, so the entry is
    /// rejected instead.
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

    /// Rebuilds a sender from a persisted counter, advancing it so no key id can be reused.
    ///
    /// `persisted_current_id` is the counter value observed when the entry was written (see
    /// [`State::current_id`]). It is advanced by `max(advance, MIN_RESTORE_ADVANCE)` to skip past
    /// any key id the pre-restart instance might have issued but not persisted. `advance` is the
    /// caller's staleness estimate (larger for an older file); the [`MIN_RESTORE_ADVANCE`] floor
    /// applies whatever the caller passes, so a too-small `advance` cannot cause key-id reuse. The
    /// result must remain a usable key id: the sender always keeps room to add one (see
    /// [`State::next_key_id`]), so the ceiling is `VarInt::MAX - 1`, and a value that would cross it
    /// is rejected with [`RestoreError::CounterOverflow`] rather than silently wrapped.
    pub fn restore(
        persisted_current_id: u64,
        stateless_reset: StatelessReset,
        advance: u64,
    ) -> Result<Self, RestoreError> {
        let advanced = persisted_current_id
            .checked_add(advance.max(MIN_RESTORE_ADVANCE))
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

    pub fn next_key_id(&self) -> VarInt {
        let id = self
            .current_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                VarInt::try_from(current + 1)
                    .ok()
                    // Make sure we can always +1. This is a useful property for StaleKey packets
                    // which send a minimum *not yet seen* ID. In practice it shouldn't matter
                    // since we are assuming we can't hit 2^62, but this helps localize handling
                    // that edge to this code.
                    .filter(|id| *id != VarInt::MAX)
                    .map(|id| *id)
            });

        let id = id.expect("2^62 integer incremented per-path will not wrap");

        // The atomic will not be incremented (i.e., would have panic'd above) if we do not fit
        // into a VarInt.
        #[expect(
            clippy::unwrap_used,
            reason = "id was produced by a successful VarInt::try_from in fetch_update, so it is provably in range"
        )]
        VarInt::try_from(id).unwrap()
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
#[should_panic = "2^62 integer incremented"]
fn sender_does_not_wrap() {
    let state = State::new([0; secret_control::TAG_LEN]);
    assert_eq!(*state.next_key_id(), 0);

    state.current_id.store((1 << 62) - 3, Ordering::Relaxed);

    assert_eq!(*state.next_key_id(), (1 << 62) - 3);
    assert_eq!(*state.next_key_id(), (1 << 62) - 2);
    assert_eq!(*state.next_key_id(), (1 << 62) - 1);
    // should panic
    state.next_key_id();
}

#[test]
fn update_restarts_sequence() {
    let state = State::new([0; secret_control::TAG_LEN]);
    assert_eq!(*state.next_key_id(), 0);

    state.update_for_stale_key(VarInt::new(3).unwrap());

    // Update should start at the minimum trusted key ID on the other side.
    assert_eq!(*state.next_key_id(), 3);
}
