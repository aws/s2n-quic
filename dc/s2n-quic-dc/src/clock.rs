// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{fmt, pin::Pin, task::Poll, time::Duration};
use s2n_quic_core::{ensure, time};
use tracing::trace;

#[macro_use]
mod macros;

#[cfg(any(test, feature = "testing"))]
pub mod bach;
#[cfg(feature = "tokio")]
pub mod tokio;
pub use time::clock::Cached;

pub use time::Timestamp;

use crate::either::Either;
pub type SleepHandle = Pin<Box<dyn Sleep>>;

pub trait Clock: 'static + Send + Sync + fmt::Debug + time::Clock {
    fn sleep(&self, amount: Duration) -> (SleepHandle, Timestamp);

    fn timer(&self) -> Timer
    where
        Self: Sized,
    {
        Timer::new(self)
    }
}

impl<A, B> Clock for Either<A, B>
where
    A: Clock,
    B: Clock,
{
    fn sleep(&self, amount: Duration) -> (SleepHandle, Timestamp) {
        match self {
            Either::A(a) => a.sleep(amount),
            Either::B(b) => b.sleep(amount),
        }
    }
}

impl<A, B> time::Clock for Either<A, B>
where
    A: time::Clock,
    B: time::Clock,
{
    fn get_time(&self) -> Timestamp {
        match self {
            Either::A(a) => a.get_time(),
            Either::B(b) => b.get_time(),
        }
    }
}

pub trait Sleep: Clock + core::future::Future<Output = ()> {
    fn update(self: Pin<&mut Self>, target: Timestamp);
}

pub struct Timer {
    /// The `Instant` at which the timer should expire
    target: Option<Timestamp>,
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

        Self::new_with_timeout(clock, INITIAL_TIMEOUT)
    }

    #[inline]
    pub fn new_with_timeout(clock: &dyn Clock, timeout: Duration) -> Self {
        let (sleep, target) = clock.sleep(timeout);
        Self {
            target: Some(target),
            sleep,
        }
    }

    #[inline]
    pub fn cancel(&mut self) {
        trace!(cancel = ?self.target);
        self.target = None;
    }

    pub async fn sleep(&mut self, target: Timestamp) {
        use time::clock::Timer;
        self.update(target);
        core::future::poll_fn(|cx| self.poll_ready(cx)).await
    }
}

impl time::clock::Timer for Timer {
    #[inline]
    fn poll_ready(&mut self, cx: &mut core::task::Context) -> Poll<()> {
        ensure!(self.target.is_some(), Poll::Ready(()));

        let res = self.sleep.as_mut().poll(cx);

        if res.is_ready() {
            // clear the target after it fires, otherwise we'll endlessly wake up the task
            self.target = None;
        }

        res
    }

    #[inline]
    fn update(&mut self, target: Timestamp) {
        // no need to update if it hasn't changed
        ensure!(self.target != Some(target));

        self.sleep.as_mut().update(target);
        self.target = Some(target);
    }
}
