// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;
use tokio::time::{self, sleep_until};

fn root() -> Instant {
    use std::sync::OnceLock;
    static ROOT: OnceLock<Instant> = OnceLock::new();

    *ROOT.get_or_init(Instant::now)
}

#[derive(Clone, Debug)]
pub struct Handle(Instant);

impl super::macros::InstantHandle for Handle {
    type Sleep = time::Sleep;

    fn new() -> Self {
        Self(root())
    }

    fn elapsed_since_start(&self) -> Duration {
        self.0.elapsed()
    }

    fn sleep(&self, amount: Duration) -> (time::Sleep, Duration) {
        let now = Instant::now();
        let sleep = sleep_until((now + amount).into());
        let target = now.saturating_duration_since(self.0);
        (sleep, target)
    }

    fn update_sleep(&self, sleep: Pin<&mut Self::Sleep>, since_start: Duration) {
        let target = self.0 + since_start;
        sleep.reset(target.into());
    }
}

impl_clock!(Handle);

#[cfg(test)]
mod tests {
    use crate::time::{tokio::Clock, Timer};
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
