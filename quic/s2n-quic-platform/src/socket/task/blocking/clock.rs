// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::{
    sync::atomic::{AtomicU64, Ordering},
    task::{Context, Poll},
};
use s2n_quic_core::time::{self, Timestamp};
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct Clock {
    pub(super) clock: time::StdClock,
    pub(super) timer: Timer,
}

impl time::Clock for Clock {
    #[inline]
    fn get_time(&self) -> Timestamp {
        self.clock.get_time()
    }
}

impl time::ClockWithTimer for Clock {
    type Timer = Timer;

    #[inline]
    fn timer(&self) -> Self::Timer {
        self.timer.clone()
    }
}

#[derive(Clone)]
pub struct Timer(Arc<AtomicU64>);

impl Timer {
    #[inline]
    pub(super) fn load(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }

    #[inline]
    pub(super) fn on_wake(&self) {
        self.0.store(u64::MAX, Ordering::Relaxed)
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self(Arc::new(AtomicU64::new(0)))
    }
}

impl time::clock::Timer for Timer {
    #[inline]
    fn poll_ready(&mut self, _cx: &mut Context) -> Poll<()> {
        if self.load() == u64::MAX {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }

    #[inline]
    fn update(&mut self, deadline: Timestamp) {
        let deadline = unsafe { deadline.as_duration().as_micros() as u64 };
        self.0.store(deadline, Ordering::Relaxed)
    }
}
