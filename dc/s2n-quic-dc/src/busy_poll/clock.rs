// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::time::precision::{self, Clock as _, Timestamp};
use core::task::Poll;
use std::{future::poll_fn, sync::OnceLock, time::Instant};

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// A polling-based clock and timer backed by `std::time::Instant`.
///
/// Unlike tokio/bach timers, busy-poll timers never register wakers — all futures
/// are polled unconditionally every iteration, so the timer just checks whether
/// wall-clock time has passed the target on each poll.
#[derive(Clone, Copy, Debug)]
pub struct Clock(Instant);

impl Default for Clock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock {
    pub fn new() -> Self {
        Self(epoch())
    }
}

impl precision::Clock for Clock {
    type Timer = Timer;

    fn now(&self) -> Timestamp {
        let nanos = self.0.elapsed().as_nanos() as u64;
        Timestamp { nanos }
    }

    fn timer(&self) -> Self::Timer {
        Timer {
            clock: *self,
            target: None,
            armed: false,
        }
    }
}

impl s2n_quic_core::time::Clock for Clock {
    #[inline]
    fn get_time(&self) -> s2n_quic_core::time::Timestamp {
        precision::Clock::now(self).into()
    }
}

#[derive(Clone, Debug)]
pub struct Timer {
    clock: Clock,
    target: Option<Timestamp>,
    armed: bool,
}

impl precision::Clock for Timer {
    type Timer = Self;

    fn now(&self) -> Timestamp {
        self.clock.now()
    }

    fn timer(&self) -> Self::Timer {
        self.clock.timer()
    }
}

impl precision::Timer for Timer {
    fn now(&self) -> Timestamp {
        precision::Clock::now(self)
    }

    async fn sleep_until(&mut self, target: Timestamp) {
        self.update(target);
        poll_fn(|cx| self.poll_ready(cx)).await
    }

    fn poll_ready(&mut self, _cx: &mut core::task::Context) -> Poll<()> {
        if !self.armed {
            return Poll::Ready(());
        }

        if let Some(target) = self.target {
            if self.clock.now() >= target {
                self.cancel();
                Poll::Ready(())
            } else {
                // We don't use the waker in busy poll since all futures are polled all the time
                Poll::Pending
            }
        } else {
            Poll::Ready(())
        }
    }

    fn update(&mut self, target: Timestamp) {
        self.target = Some(target);
        self.armed = true;
    }

    fn cancel(&mut self) {
        self.armed = false;
        self.target = None;
    }

    fn is_armed(&self) -> bool {
        self.armed
    }
}

impl s2n_quic_core::time::Clock for Timer {
    #[inline]
    fn get_time(&self) -> s2n_quic_core::time::Timestamp {
        precision::Clock::now(self).into()
    }
}
