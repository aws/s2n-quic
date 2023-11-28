// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::time::{
    timer::{self, Provider as _},
    Timer, Timestamp,
};
use core::time::Duration;

#[derive(Debug)]
pub struct TokenBucket {
    /// The current number of tokens
    current: u64,
    /// The number of tokens to refill per interval
    refill_amount: u64,
    /// The rate to refill
    refill_interval: Duration,
    /// The current pending refill
    refill_timer: Timer,
    /// The maximum number of tokens for the bucket
    max: u64,
}

impl Default for TokenBucket {
    #[inline]
    fn default() -> Self {
        Self::builder().build()
    }
}

impl TokenBucket {
    #[inline]
    pub fn builder() -> Builder {
        Builder::default()
    }

    #[inline]
    pub fn take(&mut self, amount: u64, now: Timestamp) -> u64 {
        if amount == 0 {
            self.on_timeout(now);
            return 0;
        }

        // try to refill the bucket if we couldn't take the whole thing
        if self.current < amount {
            self.on_timeout(now);
        }

        let credits = amount.min(self.current);
        self.current -= credits;

        self.on_timeout(now);

        credits
    }

    #[inline]
    pub fn set_refill_interval(&mut self, new_interval: Duration) {
        // if the value didn't change, then no need to update
        if self.refill_interval == new_interval {
            return;
        }

        // replace the previous with the new one
        let prev_interval = core::mem::replace(&mut self.refill_interval, new_interval);

        // recalibrate the refill timer with the new interval
        if let Some(target) = self.refill_timer.next_expiration() {
            if let Some(now) = target.checked_sub(prev_interval) {
                self.refill_timer.set(now + new_interval);
            }
        }
    }

    #[inline]
    pub fn on_timeout(&mut self, now: Timestamp) {
        while self.current < self.max {
            if let Some(target) = self.refill_timer.next_expiration() {
                // the target hasn't expired yet
                if !target.has_elapsed(now) {
                    break;
                }

                // increase the allowed amount of credits
                self.current = self
                    .max
                    .min(self.current.saturating_add(self.refill_amount));

                // no need to keep looping if we're at the max
                if self.current == self.max {
                    self.refill_timer.cancel();
                    break;
                }

                // reset the timer to the refill interval and loop back around to see if we can
                // issue more, just in case we were late to query the timer
                self.refill_timer.set(target + self.refill_interval);
            } else {
                // we haven't set a timer yet so set it now
                self.refill_timer.set(now + self.refill_interval);
                break;
            }
        }

        self.invariants();
    }

    #[inline]
    pub fn cancel(&mut self) {
        self.refill_timer.cancel();
    }

    #[inline]
    fn invariants(&self) {
        if cfg!(debug_assertions) {
            assert!(self.current <= self.max);
            assert_eq!(
                self.refill_timer.is_armed(),
                self.current < self.max,
                "timer should be armed ({}) if current ({}) is less than max ({})",
                self.refill_timer.is_armed(),
                self.current,
                self.max,
            );
        }
    }
}

impl timer::Provider for TokenBucket {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.refill_timer.timers(query)?;
        Ok(())
    }
}

pub struct Builder {
    max: u64,
    refill_interval: Duration,
    refill_amount: u64,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            max: 100,
            refill_amount: 5,
            refill_interval: Duration::from_secs(1),
        }
    }
}

impl Builder {
    #[inline]
    pub fn with_max(mut self, max: u64) -> Self {
        self.max = max;
        self
    }

    #[inline]
    pub fn with_refill_amount(mut self, amount: u64) -> Self {
        self.refill_amount = amount;
        self
    }

    #[inline]
    pub fn with_refill_interval(mut self, interval: Duration) -> Self {
        self.refill_interval = interval;
        self
    }

    #[inline]
    pub fn build(self) -> TokenBucket {
        let Self {
            max,
            refill_interval,
            refill_amount,
        } = self;

        TokenBucket {
            current: max,
            max,
            refill_amount,
            refill_interval,
            refill_timer: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{testing::Clock, Clock as _};

    #[test]
    fn example_test() {
        let mut bucket = TokenBucket::default();

        let mut clock = Clock::default();

        assert_eq!(bucket.take(1, clock.get_time()), 1);
        assert!(bucket.refill_timer.is_armed());

        assert_eq!(bucket.take(100, clock.get_time()), 99);

        assert_eq!(bucket.take(1, clock.get_time()), 0);

        clock.inc_by(Duration::from_secs(1));

        assert_eq!(bucket.take(100, clock.get_time()), 5);
        assert!(bucket.refill_timer.is_armed());

        clock.inc_by(Duration::from_secs(3));

        assert_eq!(bucket.take(100, clock.get_time()), 15);
        assert!(bucket.refill_timer.is_armed());
    }
}
