// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::time::Duration;
use rand::prelude::*;
use tokio::time;

pub async fn random_delay() {
    let delay = thread_rng().gen_range(0..100);
    if delay > 0 {
        let delay = Duration::from_micros(delay);
        trace!("sleeping for {delay:?}");
        time::sleep(delay).await;
    }
}

/// The number of items to send through the test queues
pub const TEST_ITEMS: usize = 10_000;

/// The configured size of each test queue.
///
/// This value is purposefully low to more frequently trigger corner cases of
/// queues wrapping and/or getting full.
pub const QUEUE_SIZE: usize = 16;
