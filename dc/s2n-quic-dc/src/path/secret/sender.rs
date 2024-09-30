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
