// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::time::timestamp::Timestamp;
use core::task::Poll;

/// A timer that does not trigger an update in a timer
/// list. These are usually owned by individual components
/// and needs to be explicitly polled.
///
/// Note: The timer doesn't implement Copy to ensure it isn't accidentally moved
///       and have the expiration discarded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timer {
    expiration: Option<Timestamp>,
}

impl Timer {
    /// Sets the timer to expire at the given timestamp
    pub fn set(&mut self, time: Timestamp) {
        self.expiration = Some(time);
    }

    /// Cancels the timer.
    /// After cancellation, a timer will no longer report as expired.
    pub fn cancel(&mut self) {
        self.expiration = None;
    }

    /// Returns true if the timer has expired
    pub fn is_expired(&self, current_time: Timestamp) -> bool {
        match self.expiration {
            Some(timeout) => timeout.has_elapsed(current_time),
            _ => false,
        }
    }

    /// Returns true if the timer is armed
    pub fn is_armed(&self) -> bool {
        self.expiration.is_some()
    }

    /// Notifies the timer of the current time.
    /// If the timer's expiration occurs before the current time, it will be cancelled.
    /// The method returns whether the timer was expired and had been
    /// cancelled.
    pub fn poll_expiration(&mut self, current_time: Timestamp) -> Poll<()> {
        if self.is_expired(current_time) {
            self.cancel();
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    /// Iterates over the contained timers
    pub fn iter(&self) -> impl Iterator<Item = Timestamp> {
        Iter(self.expiration)
    }
}

pub struct Iter(Option<Timestamp>);

impl Iterator for Iter {
    type Item = Timestamp;

    fn next(&mut self) -> Option<Self::Item> {
        self.0.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::clock::{Clock, NoopClock};
    use core::time::Duration;

    #[test]
    fn is_armed_test() {
        let now = NoopClock.get_time();
        let mut timer = Timer::default();

        assert!(!timer.is_armed());

        timer.set(now);
        assert!(timer.is_armed());

        timer.cancel();
        assert!(!timer.is_armed());
    }

    #[test]
    fn is_expired_test() {
        let mut now = NoopClock.get_time();
        let mut timer = Timer::default();

        assert!(!timer.is_expired(now));

        timer.set(now + Duration::from_millis(100));

        now += Duration::from_millis(99);
        assert!(!timer.is_expired(now));

        now += Duration::from_millis(1);
        assert!(timer.is_expired(now));

        timer.cancel();
        assert!(!timer.is_expired(now));
    }

    #[test]
    fn poll_expiration_test() {
        let mut now = NoopClock.get_time();
        let mut timer = Timer::default();

        timer.set(now + Duration::from_millis(100));

        assert!(!timer.poll_expiration(now).is_ready());
        assert!(timer.is_armed());

        now += Duration::from_millis(100);

        assert!(timer.poll_expiration(now).is_ready());
        assert!(!timer.is_armed());
    }
}
