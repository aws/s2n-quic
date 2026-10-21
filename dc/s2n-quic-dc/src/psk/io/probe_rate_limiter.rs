// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::{sync::Mutex, time::Duration};
use tokio::time::Instant;

/// A token bucket for probe starts, represented by the time at which it will be full.
///
/// The bucket starts with `rate` tokens and refills one token every `interval`. For example,
/// rate 4 allows four probes immediately, then one every 250 ms under continuous demand.
/// Idle time restores the initial burst, but never accumulates more than `rate` tokens.
///
/// Instead of storing a token count and its last update time, this uses the generic cell rate
/// algorithm: `full_at - now` is the missing credit, measured in time. Consuming a token adds
/// `interval` to that debt; advancing `now` reduces it. A probe is allowed only if the resulting
/// debt fits within `burst = rate * interval`. Clamping `full_at` to `now` caps idle credit.
///
/// Waiting does not reserve a token, so cancelling a wait when handshake capacity becomes
/// available leaves the bucket unchanged. Concurrent waiters retry against the same bucket
/// after waking; only successful acquisitions update it.
pub struct ProbeRateLimiter {
    interval: Duration,
    burst: Duration,
    /// `None` means the initially full bucket has never been used.
    full_at: Mutex<Option<Instant>>,
}

impl ProbeRateLimiter {
    pub fn new(rate: u32) -> Self {
        let rate = rate.max(1);
        // Round up to avoid exceeding the configured refill rate. This also keeps the interval
        // positive for rates above the clock's nanosecond resolution.
        let interval = Duration::from_nanos(1_000_000_000u64.div_ceil(u64::from(rate)));
        Self {
            interval,
            burst: interval * rate,
            full_at: Mutex::new(None),
        }
    }

    fn try_acquire(&self, now: Instant) -> Result<(), Instant> {
        let mut full_at = self.full_at.lock().unwrap_or_else(|e| e.into_inner());
        let next = full_at.map_or(now, |full_at| full_at.max(now)) + self.interval;
        if next.saturating_duration_since(now) > self.burst {
            // At next - burst, exactly one token will be available. Leave full_at unchanged:
            // a denied caller must not push back another waiter's opportunity to acquire it.
            return Err(next - self.burst);
        }
        *full_at = Some(next);
        Ok(())
    }

    pub async fn acquire(&self) {
        while let Err(ready_at) = self.try_acquire(Instant::now()) {
            tokio::time::sleep_until(ready_at).await;
        }
    }

    #[cfg(test)]
    pub fn is_unused(&self) -> bool {
        self.full_at
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_burst_then_sustained_refill() {
        let limiter = ProbeRateLimiter::new(4);
        let start = Instant::now();
        for _ in 0..4 {
            assert_eq!(limiter.try_acquire(start), Ok(()));
        }
        assert_eq!(
            limiter.try_acquire(start),
            Err(start + Duration::from_millis(250))
        );

        // Under continuous demand, only one token refills each 250 ms. In particular, a second
        // acquisition at 250 ms waits until 500 ms; a fresh four-token burst never appears.
        for quarter in 1..=12 {
            let now = start + Duration::from_millis(250 * quarter);
            assert_eq!(limiter.try_acquire(now - Duration::from_nanos(1)), Err(now));
            assert_eq!(limiter.try_acquire(now), Ok(()));
            assert_eq!(
                limiter.try_acquire(now),
                Err(now + Duration::from_millis(250))
            );
        }
    }

    #[test]
    fn idle_refill_is_capped_at_burst() {
        let limiter = ProbeRateLimiter::new(4);
        let start = Instant::now();
        for elapsed in [0, 1, 10, 100] {
            let now = start + Duration::from_secs(elapsed);
            for _ in 0..4 {
                assert_eq!(limiter.try_acquire(now), Ok(()));
            }
            assert_eq!(
                limiter.try_acquire(now),
                Err(now + Duration::from_millis(250))
            );
        }
    }

    #[test]
    fn partial_refill_preserves_fractional_credit() {
        let limiter = ProbeRateLimiter::new(4);
        let start = Instant::now();
        for _ in 0..2 {
            assert_eq!(limiter.try_acquire(start), Ok(()));
        }
        let halfway = start + Duration::from_millis(125);
        for _ in 0..2 {
            assert_eq!(limiter.try_acquire(halfway), Ok(()));
        }
        // The half-token refilled since the initial burst is retained.
        let ready_at = start + Duration::from_millis(250);
        assert_eq!(limiter.try_acquire(halfway), Err(ready_at));
        assert_eq!(limiter.try_acquire(ready_at), Ok(()));
        assert_eq!(
            limiter.try_acquire(ready_at),
            Err(start + Duration::from_millis(500))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn acquire_waits_for_one_token_at_a_time() {
        let limiter = ProbeRateLimiter::new(4);
        let start = Instant::now();
        for _ in 0..4 {
            limiter.acquire().await;
            assert_eq!(Instant::now(), start);
        }
        for quarter in 1..=8 {
            limiter.acquire().await;
            assert_eq!(Instant::now(), start + Duration::from_millis(250 * quarter));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancelled_wait_does_not_reserve_a_token() {
        let limiter = ProbeRateLimiter::new(1);
        let start = Instant::now();
        assert!(limiter.is_unused());
        limiter.acquire().await;
        assert!(!limiter.is_unused());

        assert!(
            tokio::time::timeout(Duration::from_millis(100), limiter.acquire())
                .await
                .is_err()
        );
        limiter.acquire().await;
        assert_eq!(Instant::now(), start + Duration::from_secs(1));
        assert_eq!(
            limiter.try_acquire(Instant::now()),
            Err(start + Duration::from_secs(2))
        );
    }

    #[tokio::test(start_paused = true)]
    async fn competing_waiters_share_refill() {
        let limiter = ProbeRateLimiter::new(1);
        let start = Instant::now();
        limiter.acquire().await;

        let acquire = || async {
            limiter.acquire().await;
            Instant::now()
        };
        let (first, second) = tokio::join!(acquire(), acquire());
        let mut acquired_at = [first, second];
        acquired_at.sort();
        assert_eq!(
            acquired_at,
            [
                start + Duration::from_secs(1),
                start + Duration::from_secs(2),
            ]
        );
    }

    #[test]
    fn refill_interval_rounds_up() {
        let limiter = ProbeRateLimiter::new(3);
        let start = Instant::now();
        for _ in 0..3 {
            assert_eq!(limiter.try_acquire(start), Ok(()));
        }
        let ready_at = start + Duration::from_nanos(333_333_334);
        assert_eq!(
            limiter.try_acquire(ready_at - Duration::from_nanos(1)),
            Err(ready_at)
        );
        assert_eq!(limiter.try_acquire(ready_at), Ok(()));
    }

    #[test]
    fn maximum_rate_still_requires_refill() {
        let limiter = ProbeRateLimiter::new(u32::MAX);
        let start = Instant::now();
        // Consume the initial burst without iterating over billions of tokens.
        *limiter.full_at.lock().unwrap() = Some(start + limiter.burst);
        let ready_at = start + Duration::from_nanos(1);
        assert_eq!(limiter.try_acquire(start), Err(ready_at));
        assert_eq!(limiter.try_acquire(ready_at), Ok(()));
        assert_eq!(
            limiter.try_acquire(ready_at),
            Err(ready_at + Duration::from_nanos(1))
        );
    }
}
