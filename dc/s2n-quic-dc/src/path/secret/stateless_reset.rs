// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{credentials::Id, packet::secret_control::TAG_LEN};
use aws_lc_rs::hmac;

#[derive(Debug)]
pub struct Signer {
    key: hmac::Key,
}

impl Signer {
    /// Creates a signer with the given secret
    pub fn new(secret: &[u8]) -> Self {
        let key = hmac::Key::new(hmac::HMAC_SHA384, secret);
        Self { key }
    }

    /// Returns a random `Signer`
    ///
    /// Note that this signer cannot be used across restarts and will result in an endpoint
    /// producing invalid `UnknownPathSecret` packets.
    ///
    /// In Bach simulation tests the key is derived deterministically from the
    /// current group's ID so that signers (and the stateless-reset tokens they
    /// produce) are stable across runs and amenable to snapshot testing.
    pub fn random() -> Self {
        let mut secret = [0u8; 32];
        #[cfg(any(test, feature = "testing"))]
        if bach::is_active() {
            let group_id = bach::group::current().id();
            secret[..8].copy_from_slice(&group_id.to_be_bytes());
            return Self::new(&secret);
        }
        aws_lc_rs::rand::fill(&mut secret).unwrap();
        Self::new(&secret)
    }

    pub fn sign(&self, id: &Id) -> [u8; TAG_LEN] {
        let mut stateless_reset = [0; TAG_LEN];

        let tag = hmac::sign(&self.key, &**id);
        stateless_reset.copy_from_slice(&tag.as_ref()[..TAG_LEN]);

        stateless_reset
    }
}
