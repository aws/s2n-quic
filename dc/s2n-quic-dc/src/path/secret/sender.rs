// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::schedule;
use crate::{crypto::awslc::open, packet::secret_control};
use s2n_quic_core::varint::VarInt;
use std::sync::atomic::{AtomicU64, Ordering};

type StatelessReset = [u8; secret_control::TAG_LEN];

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
    /// The most a single stale key packet may advance `current_id` past its current value.
    const MAX_STALE_KEY_ADVANCE: u64 = 1 << 16;

    /// The highest `current_id` a stale key may advance the counter to. Stale keys are replayable,
    /// so the per-packet cap alone could be ratcheted arbitrarily high; this bounds the cumulative
    /// effect and keeps `current_id` far enough below `VarInt::MAX` (2^62 - 1) that `next_key_id`
    /// never panics.
    const STALE_KEY_ID_CEILING: u64 = 1 << 61;

    pub fn new(stateless_reset: StatelessReset) -> Self {
        Self {
            current_id: AtomicU64::new(0),
            stateless_reset,
        }
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
    /// Note that this packet can be replayed without detection, so we must deal with authenticated
    /// but arbitrarily old IDs here.
    ///
    /// Returns `true` if the update was applied, or `false` if `min_key_id` was rejected as implausible.
    /// On rejection the caller should fall back to a re-handshake:
    /// if the rejection dropped an advance the peer genuinely needed, a fresh handshake resets both sides' key state.
    #[must_use]
    pub(super) fn update_for_stale_key(&self, min_key_id: VarInt) -> bool {
        // `next_key_id` panics if `current_id` reaches `VarInt::MAX`, and `min_key_id` is
        // attacker-controllable. A legitimate `min_key_id` only references key IDs we have already sent,
        // so it is always `<= current_id`. Reject anything implausibly far ahead rather than advancing toward exhaustion.
        // Bound it both relative to the current value and by the absolute `STALE_KEY_ID_CEILING`, which
        // holds even if replayed packets try to ratchet the counter up over many steps.
        let current = self.current_id.load(Ordering::Relaxed);
        let max_plausible = current
            .saturating_add(Self::MAX_STALE_KEY_ADVANCE)
            .min(Self::STALE_KEY_ID_CEILING);
        if *min_key_id > max_plausible {
            return false;
        }
        self.current_id.fetch_max(*min_key_id, Ordering::Relaxed);
        true
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

    assert!(state.update_for_stale_key(VarInt::new(3).unwrap()));

    // Update should start at the minimum trusted key ID on the other side.
    assert_eq!(*state.next_key_id(), 3);
}

#[test]
fn stale_key_rejects_implausible_min_key_id() {
    // A stale key carrying a `min_key_id` at the top of the VarInt space must not poison the sender counter.
    // A legitimate value is always `<= current_id`, so such a value is rejected and the counter is left untouched.
    for min_key_id in [VarInt::MAX, VarInt::MAX - 1, VarInt::new(1 << 61).unwrap()] {
        let state = State::new([0; secret_control::TAG_LEN]);

        assert!(!state.update_for_stale_key(min_key_id));

        // Rejected: the counter never moved, so allocation continues from 0.
        assert_eq!(*state.next_key_id(), 0);
    }
}

#[test]
fn stale_key_replay_cannot_ratchet_past_ceiling() {
    // Stale keys are replayable, so a relative-only bound could in principle be applied repeatedly
    // to ratchet the counter toward exhaustion. The absolute `STALE_KEY_ID_CEILING` prevents that:
    // even within the relative margin, a value above the ceiling is rejected.
    let state = State::new([0; secret_control::TAG_LEN]);
    let start = State::STALE_KEY_ID_CEILING - 1;
    state.current_id.store(start, Ordering::Relaxed);

    // Within `MAX_STALE_KEY_ADVANCE` of the current value, but reject it when it goes above the ceiling.
    assert!(!state.update_for_stale_key(VarInt::new(State::STALE_KEY_ID_CEILING + 1).unwrap()));

    assert_eq!(state.current_id.load(Ordering::Relaxed), start);

    // Replaying a max-value stale key. Those updates should be rejected.
    for _ in 0..1_000 {
        assert!(!state.update_for_stale_key(VarInt::MAX));
    }

    // Every update was rejected, so the counter never moved. The state should remain the same.
    assert_eq!(state.current_id.load(Ordering::Relaxed), start);
    assert_eq!(*state.next_key_id(), start);
}
