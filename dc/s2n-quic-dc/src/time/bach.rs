// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bach::time::{self, sleep_until, Instant};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

impl_clock!(Handle);

/// A wrapper around `bach::time::Instant` that caches the last elapsed value.
///
/// This is necessary because bach's time scheduler scope may be torn down before
/// all objects are dropped. When `elapsed_since_start()` is called after the scope
/// is gone, we fall back to the cached value instead of panicking.
#[derive(Clone, Debug)]
pub struct Handle {
    root: Instant,
    cached_elapsed_nanos: Arc<AtomicU64>,
}

impl super::macros::InstantHandle for Handle {
    type Sleep = time::Sleep;

    fn new() -> Self {
        let root = unsafe {
            // SAFETY: bach stores durations
            // TODO: add a `zero` method in bach
            core::mem::transmute(Duration::ZERO)
        };
        Self {
            root,
            cached_elapsed_nanos: Default::default(),
        }
    }

    /// Returns the elapsed time since the root instant, caching the result.
    /// If the bach scope is no longer available, returns the last cached value.
    fn elapsed_since_start(&self) -> Duration {
        if let Some(elapsed) = self.root.try_elapsed() {
            let nanos = elapsed.as_nanos() as u64;
            self.cached_elapsed_nanos
                .fetch_max(nanos, Ordering::Relaxed);
            elapsed
        } else {
            let nanos = self.cached_elapsed_nanos.load(Ordering::Relaxed);
            Duration::from_nanos(nanos)
        }
    }

    /// Sleeps for the given amount and returns the sleep future and the current
    /// elapsed duration from start.
    fn sleep(&self, amount: Duration) -> (time::Sleep, Duration) {
        let now = Instant::now();
        let sleep = sleep_until((now + amount).into());
        let target = now.saturating_duration_since(self.root);
        (sleep, target)
    }

    fn update_sleep(&self, mut sleep: Pin<&mut Self::Sleep>, since_start: Duration) {
        let target = self.root + since_start;
        sleep.reset(target);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        testing::{ext::*, sim},
        time::{bach::Clock, Timer},
    };
    use core::time::Duration;
    use s2n_quic_core::time::{clock::Timer as _, Clock as _};

    #[test]
    fn clock_test() {
        sim(|| {
            async {
                let clock = Clock::default();
                let mut timer = Timer::new(&clock);
                timer.ready().await;
                let before = clock.get_time();
                let wait = Duration::from_secs(1);
                let target = before + wait;
                timer.update(target);
                timer.ready().await;
                assert_eq!(before + wait, clock.get_time());
            }
            .primary()
            .spawn();
        });
    }
}
