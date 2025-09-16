// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines time related datatypes and functions

use crate::recovery::K_GRANULARITY;
use core::{fmt, num::NonZeroU64, time::Duration};

/// An absolute point in time.
///
/// The absolute value of `Timestamp`s should be treated as opaque. It is not
/// necessarily related to any calendar time.
/// `Timestamp`s should only be compared if they are are sourced from the same
/// clock.
///
/// `Timestamp`s are similar to the `Instant` data-type in the Rust standard
/// library, but can be created even without an available standard library.
///
/// The size of `Timestamp` is guaranteed to be consistent across platforms.
#[derive(PartialEq, Eq, PartialOrd, Ord, Copy, Clone, Hash)]
pub struct Timestamp(NonZeroU64);

impl fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Timestamp({self})")
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let duration = self.as_duration_impl();
        let micros = duration.subsec_micros();
        let secs = duration.as_secs() % 60;
        let mins = duration.as_secs() / 60 % 60;
        let hours = duration.as_secs() / 60 / 60;
        if micros != 0 {
            write!(f, "{hours}:{mins:02}:{secs:02}.{micros:06}")
        } else {
            write!(f, "{hours}:{mins:02}:{secs:02}")
        }
    }
}

#[test]
fn fmt_test() {
    macro_rules! debug {
        ($secs:expr, $micros:expr) => {
            format!(
                "{:#?}",
                Timestamp::from_duration_impl(
                    Duration::from_secs($secs) + Duration::from_micros($micros)
                )
            )
        };
    }

    assert_eq!(debug!(1, 0), "Timestamp(0:00:01)");
    assert_eq!(debug!(1, 1), "Timestamp(0:00:01.000001)");
    assert_eq!(debug!(123456789, 123456), "Timestamp(34293:33:09.123456)");
}

/// A prechecked 1us value
const ONE_MICROSECOND: NonZeroU64 = NonZeroU64::new(1).unwrap();

impl Timestamp {
    /// Tries to calculate a `Timestamp` based on the current `Timestamp` and
    /// adding the provided `Duration`. If this `Timestamp` is representable
    /// within the range of `Timestamp` it is returned as `Some(timestamp)`.
    /// Otherwise `None` is returned.
    #[inline]
    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        self.as_duration_impl()
            .checked_add(duration)
            .map(Self::from_duration_impl)
    }

    /// Tries to calculate a `Timestamp` based on the current `Timestamp` and
    /// subtracting the provided `Duration`. If this `Timestamp` is representable
    /// within the range of `Timestamp` it is returned as `Some(timestamp)`.
    /// Otherwise `None` is returned.
    #[inline]
    pub fn checked_sub(self, duration: Duration) -> Option<Self> {
        self.as_duration_impl()
            .checked_sub(duration)
            .map(Self::from_duration_impl)
    }

    /// Returns the `Duration` which elapsed since an earlier `Timestamp`.
    /// If `earlier` is more recent, the method returns a `Duration` of 0.
    #[inline]
    pub fn saturating_duration_since(self, earlier: Self) -> Duration {
        self.checked_sub(earlier.as_duration_impl())
            .map(Self::as_duration_impl)
            .unwrap_or_default()
    }

    /// Creates a `Timestamp` from a `Duration` since the time source's epoch.
    /// This will treat the duration as an absolute point in time.
    ///
    /// # Safety
    /// This should only be used by time sources
    #[inline]
    pub unsafe fn from_duration(duration: Duration) -> Self {
        Self::from_duration_impl(duration)
    }

    /// Creates a `Timestamp` from a `Duration` since the time source's epoch.
    #[inline]
    fn from_duration_impl(duration: Duration) -> Self {
        // 2^64 microseconds is ~580,000 years so casting from a u128 should be ok
        debug_assert!(duration.as_micros() <= u64::MAX.into());
        let micros = duration.as_micros() as u64;
        // if the value is 0 then round up to 1us after the epoch
        let micros = NonZeroU64::new(micros).unwrap_or(ONE_MICROSECOND);
        Self(micros)
    }

    /// Converts the `Timestamp` into the `Duration` since the time source's epoch.
    ///
    /// # Safety
    /// This should only be used by time sources
    #[inline]
    pub unsafe fn as_duration(self) -> Duration {
        Self::as_duration_impl(self)
    }

    /// Returns the timestamp as a [`Duration`] since the clock epoch.
    #[inline]
    const fn as_duration_impl(self) -> Duration {
        Duration::from_micros(self.0.get())
    }

    /// Compares the timestamp to the current time and returns true if it is in the past
    ///
    /// Note that this compares milliseconds, as any finer resolution would result in
    /// excessive timer churn.
    #[inline]
    pub const fn has_elapsed(self, now: Self) -> bool {
        let mut now = now.0.get();

        // even if the timestamp is less than the timer granularity in the future, consider it elapsed
        now += K_GRANULARITY.as_micros() as u64;

        self.0.get() < now
    }
}

impl core::ops::Add<Duration> for Timestamp {
    type Output = Timestamp;

    #[inline]
    fn add(self, rhs: Duration) -> Self::Output {
        Timestamp::from_duration_impl(self.as_duration_impl() + rhs)
    }
}

impl core::ops::AddAssign<Duration> for Timestamp {
    #[inline]
    fn add_assign(&mut self, other: Duration) {
        *self = *self + other;
    }
}

impl core::ops::Sub for Timestamp {
    type Output = Duration;

    #[inline]
    fn sub(self, rhs: Timestamp) -> Self::Output {
        self.as_duration_impl() - rhs.as_duration_impl()
    }
}

impl core::ops::Sub<Duration> for Timestamp {
    type Output = Timestamp;

    #[inline]
    fn sub(self, rhs: Duration) -> Self::Output {
        Timestamp::from_duration_impl(self.as_duration_impl() - rhs)
    }
}

impl core::ops::SubAssign<Duration> for Timestamp {
    #[inline]
    fn sub_assign(&mut self, other: Duration) {
        *self = *self - other;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_from_and_to_duration() {
        let ts1 = Timestamp::from_duration_impl(Duration::from_millis(100));
        let ts2 = Timestamp::from_duration_impl(Duration::from_millis(220));

        // Subtract timestamps to gain a duration
        assert_eq!(Duration::from_millis(120), ts2 - ts1);

        // Add duration to timestamp
        let ts3 = ts2 + Duration::from_millis(11);
        assert_eq!(Duration::from_millis(231), unsafe {
            Timestamp::as_duration(ts3)
        });

        // Subtract a duration from a timestamp
        let ts4 = ts3 - Duration::from_millis(41);
        assert_eq!(Duration::from_millis(190), unsafe {
            Timestamp::as_duration(ts4)
        });
    }

    fn timestamp_math(initial: Timestamp) {
        // Add Duration
        let mut ts1 = initial + Duration::from_millis(500);
        assert_eq!(Duration::from_millis(500), ts1 - initial);
        // AddAssign Duration
        ts1 += Duration::from_millis(100);
        assert_eq!(Duration::from_millis(600), ts1 - initial);
        // SubAssign Duration
        ts1 -= Duration::from_millis(50);
        assert_eq!(Duration::from_millis(550), ts1 - initial);
        // Sub Duration
        let ts2 = ts1 - Duration::from_millis(110);
        assert_eq!(Duration::from_millis(440), ts2 - initial);
        // Checked Sub
        assert!(ts2.checked_sub(Duration::from_secs(u64::MAX)).is_none());
        assert_eq!(Some(initial), ts2.checked_sub(Duration::from_millis(440)));
        // Checked Add
        let max_duration =
            Duration::from_secs(u64::MAX) + (Duration::from_secs(1) - Duration::from_nanos(1));
        assert_eq!(None, ts2.checked_add(max_duration));
        assert!(ts2.checked_add(Duration::from_secs(24 * 60 * 60)).is_some());

        // Saturating Timestamp sub
        let higher = initial + Duration::from_millis(200);
        assert_eq!(
            Duration::from_millis(200),
            higher.saturating_duration_since(initial)
        );
        assert_eq!(
            Duration::from_millis(0),
            initial.saturating_duration_since(higher)
        );
    }

    #[test]
    fn timestamp_math_test() {
        // Start at a high initial timestamp to let the overflow check work
        let initial = Timestamp::from_duration_impl(Duration::from_micros(1));
        timestamp_math(initial);

        // Start at a high initial timestamp to let the overflow check work
        let initial = Timestamp::from_duration_impl(Duration::from_micros(1u64 << 63));
        timestamp_math(initial);
    }
}
