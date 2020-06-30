use crate::time::timestamp::Timestamp;
use core::time::Duration;

/// A `Clock` is a source of [`Timestamp`]s.
pub trait Clock {
    /// Returns the current [`Timestamp`]
    fn get_time(&self) -> Timestamp;
}

/// A clock which always returns a Timestamp of value 1us
#[derive(Clone, Copy, Debug)]
pub struct NoopClock;

impl Clock for NoopClock {
    fn get_time(&self) -> Timestamp {
        unsafe { Timestamp::from_duration(Duration::from_micros(1)) }
    }
}

#[cfg(any(test, feature = "std"))]
mod std_clock {
    use super::*;
    use std::time::Instant;

    impl<C: 'static + Clock> Clock for &'static std::thread::LocalKey<C> {
        fn get_time(&self) -> Timestamp {
            self.with(|clock| clock.get_time())
        }
    }

    #[derive(Clone, Copy, Debug)]
    pub struct StdClock {
        epoch: Instant,
    }

    impl Default for StdClock {
        fn default() -> Self {
            Self {
                epoch: Instant::now(),
            }
        }
    }

    impl StdClock {
        /// Creates a new `StdClock` with the given epoch
        pub const fn new(epoch: Instant) -> Self {
            Self { epoch }
        }
    }

    impl Clock for StdClock {
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
}

pub use std_clock::*;
