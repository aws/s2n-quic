// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use tokio::time::{self, sleep_until, Instant};

impl_clock!();

#[cfg(test)]
mod tests {
    use crate::clock::{tokio::Clock, Timer};
    use core::time::Duration;
    use s2n_quic_core::time::{clock::Timer as _, Clock as _};

    #[tokio::test]
    async fn clock_test() {
        let clock = Clock::default();
        let mut timer = Timer::new(&clock);
        timer.ready().await;
        timer.update(clock.get_time() + Duration::from_secs(1));
        timer.ready().await;
    }
}
