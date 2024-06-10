// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::{REPLAY_DETECTED, STALE_KEY, UNKNOWN_PATH_SECRET};
use crate::crypto::IntoNonce;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, derive(bolero_generator::TypeGenerator))]
pub enum Nonce {
    UnknownPathSecret,
    StaleKey {
        // This is the minimum key ID the server will accept (at the time of sending).
        //
        // This is used for cases where the server intentionally drops state in a manner that cuts
        // out a chunk of not-yet-used key ID space.
        min_key_id: u64,
    },
    ReplayDetected {
        // This is the key ID we rejected. Should only be sent for *definitively* rejected keys:
        // use StaleKey if the key ID's status is MaybeReplayed.
        //
        // The client should enqueue a handshake but it should keep in mind that this might be
        // caused by an attacker replaying packets, so maybe impose rate limiting or ignore "really
        // old" replay detected packets.
        rejected_key_id: u64,
    },
}

impl IntoNonce for Nonce {
    #[inline]
    fn into_nonce(self) -> [u8; 12] {
        let mut nonce = [0; 12];
        match self {
            Self::UnknownPathSecret => {
                nonce[0] = UNKNOWN_PATH_SECRET;
            }
            Self::StaleKey { min_key_id } => {
                nonce[0] = STALE_KEY;
                nonce[1..9].copy_from_slice(&min_key_id.to_be_bytes());
            }
            Self::ReplayDetected { rejected_key_id } => {
                nonce[0] = REPLAY_DETECTED;
                nonce[1..9].copy_from_slice(&rejected_key_id.to_be_bytes());
            }
        }
        nonce
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;

    /// ensures output nonces are only equal if the messages are equal
    #[test]
    #[cfg_attr(kani, kani::proof, kani::solver(cadical))]
    fn nonce_uniqueness() {
        check!().with_type::<(Nonce, Nonce)>().for_each(|(a, b)| {
            if a == b {
                assert_eq!(a.into_nonce(), b.into_nonce());
            } else {
                assert_ne!(a.into_nonce(), b.into_nonce());
            }
        });
    }
}
