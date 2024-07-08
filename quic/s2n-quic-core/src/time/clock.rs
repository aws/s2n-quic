// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::time::timestamp::Timestamp;
use core::{
    task::{Context, Poll},
    time::Duration,
};

#[cfg(any(test, feature = "std"))]
mod std;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[cfg(any(test, feature = "std"))]
pub use self::std::*;

/// A `Clock` is a source of [`Timestamp`]s.
pub trait Clock {
    /// Returns the current [`Timestamp`]
    fn get_time(&self) -> Timestamp;
}

pub trait ClockWithTimer: Clock {
    type Timer: Timer;

    fn timer(&self) -> Self::Timer;
}

pub trait Timer {
    #[inline]
    fn ready(&mut self) -> TimerReady<Self> {
        TimerReady(self)
    }

    fn poll_ready(&mut self, cx: &mut Context) -> Poll<()>;
    fn update(&mut self, timestamp: Timestamp);
}

impl_ready_future!(Timer, TimerReady, ());

/// A clock which always returns a Timestamp of value 1us
#[derive(Clone, Copy, Debug)]
pub struct NoopClock;

impl Clock for NoopClock {
    fn get_time(&self) -> Timestamp {
        unsafe { Timestamp::from_duration(Duration::from_micros(1)) }
    }
}

impl Clock for Timestamp {
    #[inline]
    fn get_time(&self) -> Timestamp {
        *self
    }
}

/// A clock that caches the time query for the inner clock
pub struct Cached<'a, C: Clock + ?Sized> {
    clock: &'a C,
    cached_value: core::cell::Cell<Option<Timestamp>>,
}

impl<'a, C: Clock + ?Sized> Cached<'a, C> {
    #[inline]
    pub fn new(clock: &'a C) -> Self {
        Self {
            clock,
            cached_value: Default::default(),
        }
    }
}

impl<'a, C: Clock + ?Sized> Clock for Cached<'a, C> {
    #[inline]
    fn get_time(&self) -> Timestamp {
        if let Some(time) = self.cached_value.get() {
            return time;
        }

        let now = self.clock.get_time();
        self.cached_value.set(Some(now));
        now
    }
}
