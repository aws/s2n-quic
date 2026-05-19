// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{event, path::secret};

pub fn new(capacity: usize) -> secret::Map {
    crate::testing::init_tracing();

    let subscriber = event::tracing::Subscriber::default();

    let signer = if bach::is_active() {
        let mut secret = [0u8; 32];
        let group_id = bach::group::current().id();
        secret[..8].copy_from_slice(&group_id.to_be_bytes());
        tracing::trace!(
            group_id,
            "using deterministic stateless reset signer for bach sim map"
        );
        secret::stateless_reset::Signer::new(&secret)
    } else {
        secret::stateless_reset::Signer::random()
    };

    if s2n_quic_platform::io::testing::is_in_env() {
        secret::Map::new(
            signer,
            capacity,
            false,
            crate::time::bach::Clock::default(),
            subscriber,
        )
    } else {
        secret::Map::new(
            signer,
            capacity,
            false,
            s2n_quic_core::time::StdClock::default(),
            subscriber,
        )
    }
}
