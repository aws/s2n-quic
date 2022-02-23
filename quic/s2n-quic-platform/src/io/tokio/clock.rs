// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use s2n_quic_core::time::{self, Clock as ClockTrait, Timestamp};
use tokio::time::{sleep_until, Instant, Sleep};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Clone, Debug)]
pub struct Clock(Instant);

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock {
    pub fn new() -> Self {
        Self(Instant::now())
    }

    pub fn timer(&self) -> Timer {
        Timer::new(self.clone())
    }
}

impl ClockTrait for Clock {
    fn get_time(&self) -> time::Timestamp {
        let duration = self.0.elapsed();
        unsafe {
            // Safety: time duration is only derived from a single `Instant`
            time::Timestamp::from_duration(duration)
        }
    }
}

#[derive(Debug)]
pub struct Timer {
    clock: Clock,
    target: Option<Instant>,
    sleep: Pin<Box<Sleep>>,
}

impl Timer {
    fn new(clock: Clock) -> Self {
        let target = clock.0 + DEFAULT_TIMEOUT;
        let sleep = Box::pin(sleep_until(target));
        Self {
            clock,
            target: Some(target),
            sleep,
        }
    }

    pub fn reset(&mut self, timestamp: Timestamp) {
        let delay = unsafe {
            // Safety: the same clock epoch is being used
            timestamp.as_duration()
        };

        // floor the delay to milliseconds to reduce timer churn
        let delay = Duration::from_millis(delay.as_millis() as u64);

        // add the delay to the clock's epoch
        let next_time = self.clock.0 + delay;

        if Some(next_time) == self.target {
            return;
        }

        // if the clock has changed let the sleep future know
        self.sleep.as_mut().reset(next_time);
        self.target = Some(next_time);
    }

    pub fn cancel(&mut self) {
        self.target = None;
    }
}

impl Future for Timer {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.target.is_none() {
            return Poll::Pending;
        }

        self.sleep.as_mut().poll(cx)
    }
}
