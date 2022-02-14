// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines time related datatypes and functions

#![allow(dead_code)]

use cfg_if::cfg_if;

cfg_if! {
    if #[cfg(any(feature = "testing", test))] {
        pub use if_testing::*;
    } else if #[cfg(feature = "std")] {
        pub use if_std::*;
    } else {
        pub use if_no_std::*;
    }
}

#[cfg(any(feature = "std", test))]
mod if_std {
    //! This module implements the clock functionality for the "std" feature
    //! and environment. In this environment we are directly using `Instant`
    //! types from the Rust standard library as Timestamps.
    use lazy_static::lazy_static;
    use s2n_quic_core::time::{Clock, StdClock, Timestamp};

    lazy_static! {
        static ref GLOBAL_CLOCK: StdClock = StdClock::default();
    }

    /// Returns the current [`Timestamp`] according to the system clock
    pub fn now() -> Timestamp {
        GLOBAL_CLOCK.get_time()
    }

    /// Returns a reference to the clock.
    pub fn clock() -> &'static dyn Clock {
        &*GLOBAL_CLOCK
    }
}

// The no-std version allows to set a global clock manually.
// It is not possible to overwrite the clock at runtime.
#[cfg(any(not(feature = "std"), test))]
mod if_no_std {
    //! This module implements the clock functionality for the "no-std"
    //! environments. In those environments we allow to configure a global clock
    //! via a trait object.
    //!
    //! The global clock has to be initialized via a call to
    //! `init_global_clock`.
    use core::sync::atomic::{AtomicUsize, Ordering};
    use s2n_quic_core::time::{Clock, NoopClock, Timestamp};

    /// The configured global clock
    static mut GLOBAL_CLOCK: &'static dyn Clock = &NoopClock {};

    const CLOCK_UNINITIALIZED: usize = 0;
    const CLOCK_INITIALIZING: usize = 1;
    const CLOCK_INITIALIZED: usize = 2;

    /// Tracks whether the global clock had already been initialized
    static CLOCK_STATE: AtomicUsize = AtomicUsize::new(CLOCK_UNINITIALIZED);

    /// Initialize the global Clock for use in `no-std` mode.
    /// The configured clock will be queried on all `crate::time::now()` calls.
    /// The global clock can only be set once.
    pub fn init_global_clock(clock: &'static dyn Clock) -> Result<(), ()> {
        unsafe {
            match CLOCK_STATE.compare_exchange(
                CLOCK_UNINITIALIZED,
                CLOCK_INITIALIZING,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => {
                    GLOBAL_CLOCK = clock;
                    CLOCK_STATE.store(CLOCK_INITIALIZED, Ordering::SeqCst);
                    Ok(())
                }
                Err(err) if err == CLOCK_INITIALIZING => {
                    // Wait until a different thread has initialized the clock
                    while CLOCK_STATE.load(Ordering::SeqCst) != CLOCK_INITIALIZED {}
                    Err(())
                }
                _ => Err(()),
            }
        }
    }

    /// Returns the current [`Timestamp`] according to the system clock
    pub fn now() -> Timestamp {
        clock().get_time()
    }

    /// Returns a reference to the clock.
    ///
    /// If a clock has not been set, a no-op implementation is returned.
    pub fn clock() -> &'static dyn Clock {
        unsafe {
            if CLOCK_STATE.load(Ordering::SeqCst) != CLOCK_INITIALIZED {
                static NOP: NoopClock = NoopClock {};
                &NOP
            } else {
                GLOBAL_CLOCK
            }
        }
    }
}

/// Clock implementation if the "testing" feature is enabled
#[cfg(any(feature = "testing", test))]
mod if_testing {
    use cfg_if::cfg_if;
    use core::cell::RefCell;
    use s2n_quic_core::time::{Clock, Duration, Timestamp};
    use std::sync::Arc;

    cfg_if! {
        if #[cfg(feature = "std")] {
            use super::if_std::now as inner_now;
        } else {
            use super::if_no_std::now as inner_now;
        }
    }

    thread_local! {
        static LOCAL_CLOCK: ClockHolder = ClockHolder {
            inner: RefCell::new(None),
        }
    }

    struct ClockHolder {
        inner: RefCell<Option<Arc<dyn Clock>>>,
    }

    impl Clock for ClockHolder {
        fn get_time(&self) -> Timestamp {
            match &*self.inner.borrow() {
                Some(clock) => clock.get_time(),
                None => inner_now(),
            }
        }
    }

    /// Returns the current [`Timestamp`] according to the system clock
    pub fn now() -> Timestamp {
        (&LOCAL_CLOCK).get_time()
    }

    /// Returns a reference to the clock.
    pub fn clock() -> &'static dyn Clock {
        &&LOCAL_CLOCK
    }

    pub mod testing {
        use super::*;
        use std::sync::Mutex;

        /// Configures a [`Clock`] which will be utilized for the following
        /// calls to `crate::time::now()` on the current thread.
        ///
        /// Example:
        ///
        /// ```ignore
        /// # use core::time::Duration;
        /// use std::sync::Arc;
        /// use s2n_quic_platform::time::{self, testing};
        /// let clock = Arc::new(testing::MockClock::new());
        /// testing::set_local_clock(clock.clone());
        ///
        /// let before = time::now();
        /// clock.adjust_by(Duration::from_millis(333));
        /// let after = time::now();
        /// assert_eq!(after - before, Duration::from_millis(333));
        /// ```
        pub fn set_local_clock(clock: Arc<dyn Clock>) {
            LOCAL_CLOCK.with(|current_local_clock| {
                *current_local_clock.inner.borrow_mut() = Some(clock);
            });
        }

        /// Resets the local clock.
        ///
        /// Following invocations to [`crate::time::now()`]
        /// will return the system time again.
        pub fn reset_local_clock() {
            LOCAL_CLOCK.with(|current_local_clock| {
                *current_local_clock.inner.borrow_mut() = None;
            });
        }

        /// A [`Clock`] for testing purposes.
        ///
        /// The timestamp stored by the clock can be adjusted through the
        /// `adjust_by` and `set_time` functions.
        /// Following calls to `get_time` return the adjusted timestamp.
        pub struct MockClock {
            timestamp: Mutex<Timestamp>,
        }

        impl Default for MockClock {
            fn default() -> Self {
                Self::new()
            }
        }

        impl MockClock {
            /// Creates a new clock instance for testing purposes.
            ///
            /// The Clock will default to a default [`Timestamp`], which
            /// represents the lowest possible [`Timestamp`] which can ever be
            /// returned by this Clock. It is not allowed to adjust the Clock
            /// to a [`Timestamp`] before its initial time.
            pub fn new() -> MockClock {
                MockClock {
                    timestamp: Mutex::new(inner_now()),
                }
            }

            /// Sets the current time to the given [`Timestamp`].
            /// Follow-up calls to [`Self::get_time`] will return this [`Timestamp`]
            /// until the time had been adjusted again.
            pub fn set_time(&self, timestamp: Timestamp) {
                let mut guard = self.timestamp.lock().unwrap();
                *guard = timestamp;
            }

            /// Adjusts the time which is returned by the clock by the given
            /// Duration. This will not perform any overflow or underflow checks.
            /// If the clock went backwards more than to the initial zero
            /// timestamp, the call will lead to a panic.
            pub fn adjust_by(&self, duration: Duration) {
                let mut guard = self.timestamp.lock().unwrap();
                *guard += duration;
            }
        }

        impl Clock for MockClock {
            fn get_time(&self) -> Timestamp {
                *self.timestamp.lock().unwrap()
            }
        }
    }

    #[test]
    fn use_mocked_clock() {
        use std::sync::Arc;

        let original_time = now();

        // Switch to a mock clock
        let clock = Arc::new(testing::MockClock::new());
        testing::set_local_clock(clock.clone());

        let ts1 = now();
        clock.adjust_by(Duration::from_millis(333));
        let ts2 = now();
        assert_eq!(ts2 - ts1, Duration::from_millis(333));
        clock.adjust_by(Duration::from_millis(111));
        let ts3 = now();
        assert_eq!(ts3 - ts1, Duration::from_millis(444));

        clock.set_time(ts1);
        assert_eq!(ts1, now());

        // Switch back to the original clock
        testing::reset_local_clock();
        let restored_time = now();
        assert!(restored_time - original_time >= Duration::from_millis(0));
        assert!(restored_time - original_time <= Duration::from_millis(100));
    }
}
