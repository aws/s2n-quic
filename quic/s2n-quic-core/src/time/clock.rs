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

pub trait Timer: Sized {
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
