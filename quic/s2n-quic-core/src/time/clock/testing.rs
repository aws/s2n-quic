// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::time::{Duration, Timestamp};
use core::cell::Cell;

fn initial() -> Timestamp {
    unsafe {
        // Safety: the timestamp is non-zero
        Timestamp::from_duration(Duration::from_micros(1))
    }
}

thread_local! {
    static CLOCK: Cell<Timestamp> = Cell::new(initial());
}

pub fn now() -> Timestamp {
    CLOCK.with(|c| c.get())
}

pub fn reset() {
    CLOCK.with(|c| c.set(initial()));
}

pub fn advance(duration: Duration) {
    CLOCK.with(|c| {
        let next = c.get() + duration;
        c.set(next);
    });
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Clock(());

impl super::Clock for Clock {
    fn get_time(&self) -> Timestamp {
        now()
    }
}

impl Clock {
    pub fn inc_by(&mut self, duration: Duration) {
        advance(duration);
    }
}
