// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::time::Timestamp;
use core::{marker::PhantomData, time::Duration};

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
        current.is_none_or(|current| new >= current)
    }
}

impl<T: core::cmp::PartialOrd> Filter<T> for MinFilter {
    fn supersedes(new: T, current: Option<T>) -> bool {
        current.is_none_or(|current| new <= current)
    }
}

/// Filter that maintains the maximum value seen over the window
pub(crate) type WindowedMaxFilter<T, TimeType, DurationType> =
    WindowedFilter<T, TimeType, DurationType, MaxFilter>;
/// Filter that maintains the minimum value seen over the window
pub(crate) type WindowedMinFilter<T, TimeType, DurationType> =
    WindowedFilter<T, TimeType, DurationType, MinFilter>;

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
        if self.window_expired(now) || FilterType::supersedes(new_sample, self.current_value) {
            self.current_value = Some(new_sample);
            self.last_updated = Some(now);
        }
    }

    /// Returns the current value if one has been recorded yet
    pub fn value(&self) -> Option<T> {
        self.current_value
    }

    #[inline]
    fn window_expired(&self, now: TimeType) -> bool {
        self.last_updated
            .is_some_and(|last_updated| now - last_updated >= self.window_length)
    }
}
//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.3.2
//# A BBR implementation MAY use a generic windowed min filter to track BBR.min_rtt.
//# However, a significant savings in space and improvement in freshness can be achieved
//# by integrating the BBR.min_rtt estimation into the ProbeRTT state machine

/// Specialized WindowedMinFilter for tracking min round trip time
///
/// BBRv2 tracks both a min probe RTT that is refreshed at least every `PROBE_RTT_INTERVAL`,
/// and an overall min_rtt that is refreshed at least every `MIN_RTT_FILTER_LEN`.
#[derive(Clone, Debug)]
pub(crate) struct MinRttWindowedFilter {
    min_probe_rtt: WindowedMinFilter<Duration, Timestamp, Duration>,
    min_rtt: WindowedMinFilter<Duration, Timestamp, Duration>,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.14.2
    //# A boolean recording whether the BBR.probe_rtt_min_delay has expired and is due for a
    //# refresh with an application idle period or a transition into ProbeRTT state.
    probe_rtt_expired: bool,
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.14.2
//# A constant specifying the minimum time interval between ProbeRTT states: 5 secs.
pub const PROBE_RTT_INTERVAL: Duration = Duration::from_secs(5);

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.14.1
//# A constant specifying the length of the BBR.min_rtt min filter window,
//# MinRTTFilterLen is 10 secs.
const MIN_RTT_FILTER_LEN: Duration = Duration::from_secs(10);

impl MinRttWindowedFilter {
    /// Constructs a new MinRttWindowedFilter
    pub fn new() -> Self {
        Self {
            min_probe_rtt: WindowedMinFilter::new(PROBE_RTT_INTERVAL),
            min_rtt: WindowedMinFilter::new(MIN_RTT_FILTER_LEN),
            probe_rtt_expired: false,
        }
    }

    /// Updates the min_probe_rtt and min_rtt estimates with the given `rtt`
    pub fn update(&mut self, rtt: Duration, now: Timestamp) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.4
        //# BBRUpdateMinRTT()
        //#   BBR.probe_rtt_expired =
        //#     Now() > BBR.probe_rtt_min_stamp + ProbeRTTInterval
        //#   if (rs.rtt >= 0 and
        //#       (rs.rtt < BBR.probe_rtt_min_delay or
        //#        BBR.probe_rtt_expired))
        //#      BBR.probe_rtt_min_delay = rs.rtt
        //#      BBR.probe_rtt_min_stamp = Now()
        //#
        //#   min_rtt_expired =
        //#     Now() > BBR.min_rtt_stamp + MinRTTFilterLen
        //#   if (BBR.probe_rtt_min_delay < BBR.min_rtt or
        //#       min_rtt_expired)
        //#     BBR.min_rtt       = BBR.probe_rtt_min_delay
        //#     BBR.min_rtt_stamp = BBR.probe_rtt_min_stamp

        self.probe_rtt_expired = self.min_probe_rtt.window_expired(now);
        self.min_probe_rtt.update(rtt, now);

        let probe_rtt = self
            .min_probe_rtt
            .value()
            .expect("probe_rtt is updated just prior");

        if self.min_rtt.window_expired(now)
            || MinFilter::supersedes(probe_rtt, self.min_rtt.value())
        {
            // When the min_rtt expires or is superseded, it is replaced with the
            // min_probe_rtt value and the last updated time from min_probe_rtt
            // rather than the latest RTT and current time to keep min_probe_rtt and
            // min_rtt coordinated.
            self.min_rtt.current_value = self.min_probe_rtt.value();
            self.min_rtt.last_updated = self.min_probe_rtt.last_updated;
        }
    }

    /// The current windowed estimate of minimum round trip time
    pub fn min_rtt(&self) -> Option<Duration> {
        self.min_rtt.value()
    }

    /// True if the probe RTT has expired and is due for a refresh by entering the ProbeRTT state
    pub fn probe_rtt_expired(&self) -> bool {
        self.probe_rtt_expired
    }

    /// Overrides the last updated time for Min Probe RTT to ensure ProbeRTT is not entered until
    /// the next `PROBE_RTT_INTERVAL`.
    pub fn schedule_next_probe_rtt(&mut self, now: Timestamp) {
        self.min_probe_rtt.last_updated = Some(now);
    }

    #[cfg(test)]
    pub fn next_probe_rtt(&self) -> Option<Timestamp> {
        self.min_probe_rtt
            .last_updated
            .map(|last_updated| last_updated + PROBE_RTT_INTERVAL)
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

    #[test]
    fn min_rtt_filter() {
        let mut filter = MinRttWindowedFilter::new();

        // Filter has not received an update, so no value should be present
        assert_eq!(None, filter.min_rtt());
        // Probe RTT is not expired upon initialization
        assert!(!filter.probe_rtt_expired());

        // After the first update, the first value is the min
        let now = NoopClock.get_time();
        filter.update(Duration::from_millis(5), now);
        assert_eq!(Some(Duration::from_millis(5)), filter.min_rtt());
        assert!(!filter.probe_rtt_expired());

        // PROBE_RTT_INTERVAL has passed
        let now = now + PROBE_RTT_INTERVAL;
        filter.update(Duration::from_millis(9), now);
        // min_rtt is not updated since it has not expired and is lower than the new rtt
        assert_eq!(Some(Duration::from_millis(5)), filter.min_rtt());
        assert!(filter.probe_rtt_expired());

        // Midway through the Probe RTT period the RTT drops
        let now = now + Duration::from_secs(2);
        let probe_rtt_update_time = now;
        filter.update(Duration::from_millis(7), now);
        // min_rtt is not updated since it has not expired and is lower than the new rtt
        assert_eq!(Some(Duration::from_millis(5)), filter.min_rtt());
        assert!(!filter.probe_rtt_expired());
        assert_eq!(Some(Duration::from_millis(7)), filter.min_probe_rtt.value());

        // Now the Min RTT has expired, since it has been MIN_RTT_FILTER_LEN (10 seconds) since the
        // min_rtt value was first set (PROBE_RTT_INTERVAL + 2 seconds + 3 seconds)
        let now = now + Duration::from_secs(3);
        filter.update(Duration::from_millis(10), now);
        // min_rtt is updated since it has expired. The value is set to the current probe_rtt
        assert_eq!(Some(Duration::from_millis(7)), filter.min_rtt());
        // min_rtt last_updated is set to the probe_rtt last updated time
        assert_eq!(Some(probe_rtt_update_time), filter.min_rtt.last_updated);
        assert!(!filter.probe_rtt_expired());

        filter.schedule_next_probe_rtt(now);

        let now = now + PROBE_RTT_INTERVAL;
        assert!(!filter.probe_rtt_expired());
        filter.update(Duration::from_secs(10), now);
        assert!(filter.probe_rtt_expired());
    }
}
