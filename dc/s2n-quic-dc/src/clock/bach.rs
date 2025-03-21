// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bach::time::{self, sleep_until, Instant};

impl_clock!();

#[cfg(test)]
mod tests {
    use crate::{
        clock::{bach::Clock, Timer},
        testing::{ext::*, sim},
    };
    use bach::time::Instant;
    use core::time::Duration;
    use s2n_quic_core::time::{clock::Timer as _, Clock as _};

    #[test]
    fn clock_test() {
        sim(|| {
            async {
                let clock = Clock::default();
                let mut timer = Timer::new(&clock);
                timer.ready().await;
                let before = Instant::now();
                let wait = Duration::from_secs(1);
                timer.update(clock.get_time() + wait);
                timer.ready().await;
                assert_eq!(before + wait, Instant::now());
            }
            .primary()
            .spawn();
        });
    }
}
