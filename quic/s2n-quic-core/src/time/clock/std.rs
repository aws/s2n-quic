// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use ::std::time::Instant;

impl<C: 'static + Clock> Clock for &'static ::std::thread::LocalKey<C> {
    fn get_time(&self) -> Timestamp {
        self.with(|clock| clock.get_time())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StdClock {
    epoch: Instant,
}

impl Default for StdClock {
    // this is the only usage of `Instant::now` since it implements the clock abstraction
    #[allow(clippy::disallowed_methods)]
    fn default() -> Self {
        let epoch = Instant::now();
        Self { epoch }
    }
}

impl StdClock {
    /// Creates a new `StdClock` with the given epoch
    pub const fn new(epoch: Instant) -> Self {
        Self { epoch }
    }
}

impl Clock for StdClock {
    // this is the only usage of `Instant::elapsed` since it implements the clock abstraction
    #[allow(clippy::disallowed_methods)]
    fn get_time(&self) -> Timestamp {
        unsafe { Timestamp::from_duration(self.epoch.elapsed()) }
    }
}

#[test]
#[cfg_attr(miri, ignore)] // time isn't queryable in miri
fn monotonicity_test() {
    let clock = StdClock::default();
    let ts1 = clock.get_time();
    ::std::thread::sleep(Duration::from_millis(50));
    let ts2 = clock.get_time();
    assert!(ts2 - ts1 >= Duration::from_millis(50));
}
