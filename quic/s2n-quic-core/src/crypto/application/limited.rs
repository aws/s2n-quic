// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::crypto::OneRttKey;

//= https://www.rfc-editor.org/rfc/rfc9001#section-6.6
//# Endpoints MUST count the number of encrypted packets for each set of
//# keys.
pub struct Key<K> {
    key: K,

    // Keeping encrypted_packets out of the key allow keys to be immutable, which allows optimizations
    // later on.
    encrypted_packets: u64,
    decrypted_packets: u64,
    confidentiality_limit: u64,
}

#[derive(Copy, Clone, Debug)]
#[non_exhaustive]
pub struct Limits {
    /// The number of packets before the limit at which a key update will be scheduled
    pub key_update_window: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            key_update_window: KEY_UPDATE_WINDOW,
        }
    }
}

// TODO: This should be configured through the limit provider.
// https://github.com/aws/s2n-quic/issues/322
const KEY_UPDATE_WINDOW: u64 = 10_000;

impl<K: OneRttKey> Key<K> {
    pub fn new(key: K) -> Self {
        Key {
            // TODO: This should be configured through the limit provider, the default being the
            // key's AEAD limit.
            // https://github.com/aws/s2n-quic/issues/322
            confidentiality_limit: key.aead_confidentiality_limit(),
            key,
            encrypted_packets: 0,
            decrypted_packets: 0,
        }
    }

    /// Keys used past the confidentiality_limit are expired
    #[inline]
    pub fn expired(&self) -> bool {
        // We check >= because we don't want to encrypt an additional packet if the key has
        // already been used up to the limit.
        self.encrypted_packets >= self.confidentiality_limit
    }

    /// If the key is within the update window, an update should be initiated.
    #[inline]
    pub fn needs_update(&self, limits: &Limits) -> bool {
        self.encrypted_packets
            > (self
                .confidentiality_limit
                .saturating_sub(limits.key_update_window))
    }

    pub fn derive_next_key(&self) -> K {
        self.key.derive_next_key()
    }

    #[inline]
    pub fn encrypted_packets(&self) -> u64 {
        self.encrypted_packets
    }

    #[inline]
    pub fn on_packet_encryption(&mut self, _limits: &Limits) {
        self.encrypted_packets += 1;
    }

    #[inline]
    pub fn on_packet_decryption(&mut self, _limits: &Limits) {
        self.decrypted_packets += 1;
    }

    #[inline]
    pub fn key_mut(&mut self) -> &mut K {
        &mut self.key
    }
}
