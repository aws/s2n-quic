// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use bach::time::{self, scheduler};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use futures::ready;
use s2n_quic_core::time::Timestamp;

pub fn now() -> Timestamp {
    unsafe { Timestamp::from_duration(time::now()) }
}

pub fn delay(duration: Duration) -> Timer {
    Timer::new(now() + duration, duration)
}

pub fn delay_until(deadline: Timestamp) -> Timer {
    let delay = deadline.saturating_duration_since(now());
    Timer::new(deadline, delay)
}

#[derive(Debug, Default)]
pub(crate) struct Clock(());

impl s2n_quic_core::time::Clock for Clock {
    fn get_time(&self) -> Timestamp {
        now()
    }
}

pub struct Timer {
    timer: scheduler::Timer,
    deadline: Option<Timestamp>,
}

impl Default for Timer {
    fn default() -> Self {
        let timer = time::delay(Duration::ZERO);

        Self {
            timer,
            deadline: None,
        }
    }
}

impl Timer {
    fn new(deadline: Timestamp, delay: Duration) -> Self {
        Self {
            timer: time::delay(delay),
            deadline: Some(deadline),
        }
    }

    pub fn update(&mut self, deadline: Timestamp) {
        if self.deadline != Some(deadline) {
            self.cancel();
            *self = delay_until(deadline);
        }
    }

    pub fn cancel(&mut self) {
        self.deadline = None;
        self.timer.cancel()
    }
}

impl Future for Timer {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.deadline.is_none() {
            return Poll::Pending;
        }

        ready!(Pin::new(&mut self.timer).poll(cx));

        self.deadline = None;
        Poll::Ready(())
    }
}
