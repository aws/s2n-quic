// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::schedule;
use crate::{crypto::awslc::open, packet::secret_control};
use s2n_quic_core::varint::{VarInt, MAX_VARINT_VALUE};
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
    /// Key IDs held in reserve below `VarInt::MAX` so `next_key_id` always has room to allocate.
    const STALE_KEY_ID_RESERVE: u64 = 1 << 40;

    /// The highest `current_id` a stale key may advance the counter to.
    ///
    /// A stale key carries a peer-supplied, authenticated `min_key_id` that we apply with
    /// `fetch_max`. We let it advance the counter freely and clamp only at the
    /// point where going further would starve `next_key_id`.
    pub(super) const STALE_KEY_ID_CEILING: u64 = MAX_VARINT_VALUE - Self::STALE_KEY_ID_RESERVE;

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
    /// Returns `true` if the peer's `min_key_id` was applied as-is, or `false` if it exceeded
    /// [`Self::STALE_KEY_ID_CEILING`] and was clamped. On a clamp the caller should fall back to a
    /// re-handshake: we could not move the counter to where the peer asked, so a fresh handshake
    /// resets both sides' key state.
    #[must_use]
    pub(super) fn update_for_stale_key(&self, min_key_id: VarInt) -> bool {
        // Let the peer advance the counter freely, but clamp it below `VarInt::MAX` so `next_key_id`
        // always has headroom and never panics.
        let applied = (*min_key_id).min(Self::STALE_KEY_ID_CEILING);
        self.current_id.fetch_max(applied, Ordering::Relaxed);
        *min_key_id <= Self::STALE_KEY_ID_CEILING
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
fn stale_key_allows_fast_forward() {
    // A large but in-range `min_key_id` is applied as-is -- we do not reject fast advances.
    let state = State::new([0; secret_control::TAG_LEN]);

    let target = 1u64 << 40;
    assert!(state.update_for_stale_key(VarInt::new(target).unwrap()));

    assert_eq!(*state.next_key_id(), target);
}

#[test]
fn stale_key_clamps_to_ceiling() {
    // A `min_key_id` above the ceiling must not poison the sender counter.
    // It is clamped to `STALE_KEY_ID_CEILING` so `next_key_id` keeps working
    // with ample headroom rather than panicking.
    for min_key_id in [VarInt::MAX, VarInt::MAX - 1] {
        let state = State::new([0; secret_control::TAG_LEN]);

        assert!(!state.update_for_stale_key(min_key_id));

        // Clamped to the ceiling, not advanced to the requested value.
        assert_eq!(*state.next_key_id(), State::STALE_KEY_ID_CEILING);
    }
}

#[test]
fn stale_key_ceiling_leaves_room_to_allocate() {
    // Guards the choice of `STALE_KEY_ID_RESERVE`: clamping must leave real headroom, not park the
    // counter next to the panic. Verify a clamped sender can still allocate freely.
    let state = State::new([0; secret_control::TAG_LEN]);
    assert!(!state.update_for_stale_key(VarInt::MAX));
    assert_eq!(
        state.current_id.load(Ordering::Relaxed),
        State::STALE_KEY_ID_CEILING
    );

    for i in 0..10_000 {
        assert_eq!(*state.next_key_id(), State::STALE_KEY_ID_CEILING + i);
    }

    // The reserve is far larger than the allocations above, so plenty remains.
    assert!(MAX_VARINT_VALUE > state.current_id.load(Ordering::Relaxed));
}

#[test]
fn stale_key_replay_is_idempotent() {
    // `min_key_id` is absolute and applied with `fetch_max`,
    // so replaying the same packet cannot ratchet the counter past where the first packet left it.
    let state = State::new([0; secret_control::TAG_LEN]);

    for _ in 0..1_000 {
        assert!(!state.update_for_stale_key(VarInt::MAX));
    }

    // The counter sits at the ceiling after the first packet and never moves past it.
    assert_eq!(
        state.current_id.load(Ordering::Relaxed),
        State::STALE_KEY_ID_CEILING
    );
    assert_eq!(*state.next_key_id(), State::STALE_KEY_ID_CEILING);
}
