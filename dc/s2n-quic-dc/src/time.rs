// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::tracing::trace;
use core::{fmt, pin::Pin, task::Poll, time::Duration};
use s2n_quic_core::{
    ensure, time,
    time::{timer, timer::Provider},
};

#[macro_use]
mod macros;

#[cfg(any(test, feature = "testing"))]
pub mod bach;
pub mod precision;
#[cfg(test)]
pub mod testing;
#[cfg(feature = "tokio")]
pub mod tokio;
pub use time::clock::Cached;

pub use time::Timestamp;
pub mod wheel;

/// Default production clock.
///
/// Checks [`bach::is_active()`](::bach::is_active) **once** at construction and
/// stores either the Bach simulated-time epoch or the busy-poll epoch internally.
/// Subsequent [`now`](Self::now) calls skip the per-call static deref.
#[derive(Clone, Debug)]
pub struct DefaultClock(DefaultClockKind);

#[derive(Clone, Debug)]
enum DefaultClockKind {
    #[cfg(any(test, feature = "testing"))]
    Bach(bach::Clock),
    BusyPoll(crate::busy_poll::clock::Clock),
}

impl Default for DefaultClock {
    fn default() -> Self {
        #[cfg(any(test, feature = "testing"))]
        if ::bach::is_active() {
            return Self(DefaultClockKind::Bach(bach::Clock::default()));
        }
        Self(DefaultClockKind::BusyPoll(crate::busy_poll::clock::Clock::default()))
    }
}

impl DefaultClock {
    /// Returns the current time as a [`precision::Timestamp`].
    #[inline]
    pub fn now(&self) -> precision::Timestamp {
        match &self.0 {
            #[cfg(any(test, feature = "testing"))]
            DefaultClockKind::Bach(c) => precision::Clock::now(c),
            DefaultClockKind::BusyPoll(c) => precision::Clock::now(c),
        }
    }
}

impl time::Clock for DefaultClock {
    #[inline]
    fn get_time(&self) -> time::Timestamp {
        self.now().into()
    }
}

pub type SleepHandle = Pin<Box<dyn Sleep>>;

pub trait Clock: 'static + Send + Sync + fmt::Debug + time::Clock {
    fn sleep(&self, amount: Duration) -> (SleepHandle, Timestamp);

    fn timer(&self) -> Timer;
}

pub trait Sleep: Clock + core::future::Future<Output = ()> {
    fn update(self: Pin<&mut Self>, target: Timestamp);
}

pub struct Timer {
    /// The `Instant` at which the timer should expire
    target: timer::Timer,
    /// The handle to the timer entry in the tokio runtime
    sleep: Pin<Box<dyn Sleep>>,
}

impl fmt::Debug for Timer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Timer")
            .field("target", &self.target)
            .finish()
    }
}

impl Timer {
    #[inline]
    pub fn new(clock: &dyn Clock) -> Self {
        /// We can't create a timer without first arming it to something, so just set it to 1s in
        /// the future.
        const INITIAL_TIMEOUT: Duration = Duration::from_secs(1);

        let mut timer = Self::new_with_timeout(clock, INITIAL_TIMEOUT);
        timer.cancel();
        timer
    }

    #[inline]
    pub fn new_with_timeout(clock: &dyn Clock, timeout: Duration) -> Self {
        let (sleep, target) = clock.sleep(timeout);
        let mut timer = timer::Timer::default();
        timer.set(target);
        Self {
            target: timer,
            sleep,
        }
    }

    #[inline]
    pub fn cancel(&mut self) {
        trace!(cancel = ?self.target);
        self.target.cancel();
    }

    pub async fn sleep(&mut self, target: Timestamp) {
        use time::clock::Timer;
        self.update(target);
        core::future::poll_fn(|cx| self.poll_ready(cx)).await
    }
}

impl time::Clock for Timer {
    fn get_time(&self) -> Timestamp {
        self.sleep.get_time()
    }
}

impl time::clock::Timer for Timer {
    #[inline]
    fn poll_ready(&mut self, cx: &mut core::task::Context) -> Poll<()> {
        ensure!(self.target.is_armed(), Poll::Ready(()));

        let res = self.sleep.as_mut().poll(cx);

        if res.is_ready() {
            // clear the target after it fires, otherwise we'll endlessly wake up the task
            self.target.cancel();
        }

        res
    }

    #[inline]
    fn update(&mut self, target: Timestamp) {
        // no need to update if it hasn't changed
        ensure!(self.target.next_expiration() != Some(target));

        self.sleep.as_mut().update(target);
        self.target.set(target);
    }
}

impl timer::Provider for Timer {
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.target.timers(query)
    }
}
