// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Fast non-cryptographic PRNG for load balancing decisions (pick-two, etc).

use std::hash::Hasher;

#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new() -> Self {
        #[cfg(any(test, feature = "testing"))]
        if bach::is_active() {
            if crate::testing::snapshots_enabled() {
                let seed = bach::group::current()
                    .id()
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15);
                return Self(seed | 1);
            }

            use bach::rand::any;
            return Self(any::<u64>() | 1);
        }

        let seed =
            std::hash::BuildHasher::build_hasher(&std::collections::hash_map::RandomState::new())
                .finish();
        Self(seed | 1)
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        #[cfg(any(test, feature = "testing"))]
        if bach::is_active() && !crate::testing::snapshots_enabled() {
            use bach::rand::any;
            return any();
        }

        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    #[inline]
    pub fn next_usize(&mut self, bound: usize) -> usize {
        debug_assert!(bound > 0);

        #[cfg(any(test, feature = "testing"))]
        if bach::is_active() && !crate::testing::snapshots_enabled() {
            use bach::rand::Any;
            return (..bound).any();
        }

        self.next_u64() as usize % bound
    }
}

impl Default for Rng {
    fn default() -> Self {
        Self::new()
    }
}

impl s2n_quic_core::random::Generator for Rng {
    fn public_random_fill(&mut self, dest: &mut [u8]) {
        let mut offset = 0;
        while offset < dest.len() {
            let bytes = self.next_u64().to_ne_bytes();
            let remaining = dest.len() - offset;
            let to_copy = remaining.min(8);
            dest[offset..offset + to_copy].copy_from_slice(&bytes[..to_copy]);
            offset += to_copy;
        }
    }

    fn private_random_fill(&mut self, _dest: &mut [u8]) {
        panic!("xorshift::Rng must not be used for private/cryptographic randomness");
    }
}
