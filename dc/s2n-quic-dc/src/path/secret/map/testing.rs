// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{event, path::secret};

pub fn new(capacity: usize) -> secret::Map {
    crate::testing::init_tracing();

    let subscriber = event::tracing::Subscriber::default();

    let signer = secret::stateless_reset::Signer::random();

    if s2n_quic_platform::io::testing::is_in_env() {
        secret::Map::new(
            signer,
            capacity,
            false,
            crate::clock::bach::Clock::default(),
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
