// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::marker::PhantomData;

/// Data structure for tracking the minimum or maximum value seen over a configurable
/// time period specified by the `window_length`
///
/// Based on https://git.kernel.org/pub/scm/linux/kernel/git/torvalds/linux.git/commit/?id=f672258391b42a5c7cc2732c9c063e56a85c8dbe
#[derive(Clone, Debug)]
pub(crate) struct WindowedFilter<T, TimeType, DurationType, FilterType> {
    current_value: Option<T>,
    last_updated: Option<TimeType>,
    window_length: DurationType,
    filter: PhantomData<FilterType>,
}

pub(crate) trait Filter<T> {
    /// Returns true if the `new` value should replace the `current` value
    fn supersedes(new: T, current: Option<T>) -> bool;
}

#[derive(Clone, Debug)]
pub(crate) struct MaxFilter;
#[derive(Clone, Debug)]
pub(crate) struct MinFilter;

impl<T: core::cmp::PartialOrd> Filter<T> for MaxFilter {
    fn supersedes(new: T, current: Option<T>) -> bool {
        current.map_or(true, |current| new >= current)
    }
}

impl<T: core::cmp::PartialOrd> Filter<T> for MinFilter {
    fn supersedes(new: T, current: Option<T>) -> bool {
        current.map_or(true, |current| new <= current)
    }
}

/// Filter that maintains the maximum value seen over the window
#[allow(dead_code)] // TODO: Remove when used
pub(crate) type WindowedMaxFilter<T, TimeType, DurationType> =
    WindowedFilter<T, TimeType, DurationType, MaxFilter>;
/// Filter that maintains the minimum value seen over the window
#[allow(dead_code)] // TODO: Remove when used
pub(crate) type WindowedMinFilter<T, TimeType, DurationType> =
    WindowedFilter<T, TimeType, DurationType, MinFilter>;

#[allow(dead_code)] // TODO: Remove when used
impl<
        T: Copy + PartialOrd,
        TimeType: Copy + PartialOrd + core::ops::Sub<Output = DurationType>,
        DurationType: PartialOrd,
        FilterType: Filter<T>,
    > WindowedFilter<T, TimeType, DurationType, FilterType>
{
    /// Constructs a new `WindowedFilter` with the specified `window_length`
    pub fn new(window_length: DurationType) -> Self {
        Self {
            current_value: None,
            last_updated: None,
            window_length,
            filter: Default::default(),
        }
    }

    /// Updates the `WindowedFilter` with the given sample
    ///
    /// If the `new_sample` supersedes the current value according to the `Filter` or the current
    /// value has expired according to the window length and the `now` value, the new sample will
    /// become the current value.
    ///
    /// `now` must be monotonically increasing, unless the `TimeType` supports wrapping (such as
    /// `core::num::Wrapping`)
    pub fn update(&mut self, new_sample: T, now: TimeType) {
        let current_value_expired = self.last_updated.map_or(true, |last_updated| {
            now - last_updated >= self.window_length
        });

        if current_value_expired || FilterType::supersedes(new_sample, self.current_value) {
            self.current_value = Some(new_sample);
            self.last_updated = Some(now);
        }
    }

    /// Returns the current value if one has been recorded yet
    pub fn value(&self) -> Option<T> {
        self.current_value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{Clock, NoopClock};
    use core::time::Duration;

    #[test]
    fn min_filter() {
        let mut filter = WindowedMinFilter::new(Duration::from_secs(10));

        // Filter has not received an update, so no value should be present
        assert_eq!(None, filter.value());
        assert_eq!(None, filter.last_updated);

        // After the first update, the first value is the min
        let now = NoopClock.get_time();
        filter.update(7, now);
        assert_eq!(Some(7), filter.value());
        assert_eq!(Some(now), filter.last_updated);

        // A lower value is received
        let now = now + Duration::from_secs(5);
        filter.update(3, now);
        assert_eq!(Some(3), filter.value());
        assert_eq!(Some(now), filter.last_updated);

        // A value higher than the min is received, no update to the value
        let now = now + Duration::from_secs(9);
        filter.update(4, now);
        assert_eq!(Some(3), filter.value());
        assert!(filter.last_updated.unwrap() < now);

        // A value higher than the min is received, but the current min has expired
        let now = now + Duration::from_secs(1);
        filter.update(4, now);
        assert_eq!(Some(4), filter.value());
        assert_eq!(Some(now), filter.last_updated);
    }

    #[test]
    fn max_filter() {
        let mut filter = WindowedMaxFilter::new(10);

        // Filter has not received an update, so no value should be present
        assert_eq!(None, filter.value());
        assert_eq!(None, filter.last_updated);

        // After the first update, the first value is the max
        let mut now = 0;
        filter.update(7, now);
        assert_eq!(Some(7), filter.value());
        assert_eq!(Some(now), filter.last_updated);

        // A higher value is received
        now += 1;
        filter.update(8, now);
        assert_eq!(Some(8), filter.value());
        assert_eq!(Some(now), filter.last_updated);

        // A value lower than the max is received, no update to the value
        now += 9;
        filter.update(4, now);
        assert_eq!(Some(8), filter.value());
        assert!(filter.last_updated.unwrap() < now);

        // A value lower than the max is received, but the current max has expired
        now += 1;
        filter.update(4, now);
        assert_eq!(Some(4), filter.value());
        assert_eq!(Some(now), filter.last_updated);
    }

    #[test]
    fn wrapping() {
        let mut filter = WindowedMaxFilter::new(core::num::Wrapping(2_u8));

        // Filter has not received an update, so no value should be present
        assert_eq!(None, filter.value());
        assert_eq!(None, filter.last_updated);

        // After the first update, the first value is the max
        let mut now = core::num::Wrapping(0);
        filter.update(7, now);
        assert_eq!(Some(7), filter.value());
        assert_eq!(Some(now), filter.last_updated);

        // now is set to the maximum for the time type in use
        now = core::num::Wrapping(u8::MAX);
        filter.update(2, now);
        assert_eq!(Some(2), filter.value());
        assert_eq!(Some(now), filter.last_updated);

        // Wrapping around, the current value should not be considered expired
        now = core::num::Wrapping(0);
        filter.update(1, now);
        assert_eq!(Some(2), filter.value());
        assert_eq!(Some(core::num::Wrapping(u8::MAX)), filter.last_updated);

        // Now the current value has expired
        now += core::num::Wrapping(1);
        filter.update(1, now);
        assert_eq!(Some(1), filter.value());
        assert_eq!(Some(now), filter.last_updated);
    }
}
