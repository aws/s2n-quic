// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Saturating},
    recovery::RttEstimator,
    time::{Duration, Timestamp},
};
use core::ops::Div;

struct Fraction(u32, u32);

impl Div<Fraction> for Duration {
    type Output = Duration;

    fn div(self, rhs: Fraction) -> Self::Output {
        self * rhs.1 / rhs.0
    }
}

//= https://www.rfc-editor.org/rfc/rfc9002.txt#7.7
//# Using a value for "N" that is small, but at least 1 (for example, 1.25) ensures
//# that variations in RTT do not result in underutilization of the congestion window.
const N: Fraction = Fraction(5, 4); // 5/4 = 1.25

// In Slow Start, the congestion window grows rapidly, so there is a higher likelihood the congestion
// window may be underutilized due to pacing. To prevent that, we use a higher value for `N` while
// in slow start, as done in Linux:
// https://github.com/torvalds/linux/blob/fc02cb2b37fe2cbf1d3334b9f0f0eab9431766c4/net/ipv4/tcp_input.c#L905-L906
const SLOW_START_N: Fraction = Fraction(2, 1); // 2/1 = 2.00

// TODO: this should be aligned with GSO max segments
//= https://www.rfc-editor.org/rfc/rfc9002.txt#7.7
//# Senders SHOULD limit bursts to the initial congestion window
const MAX_BURST_PACKETS: u16 = 10;

/// A packet pacer that returns departure times that evenly distribute bursts of packets over time
#[derive(Clone, Debug, Default)]
pub struct Pacer {
    // The capacity of the current departure time slot
    capacity: Counter<u32, Saturating>,
    // The time the next packet should be transmitted
    next_packet_departure_time: Option<Timestamp>,
}

// TODO: Remove when used
#[allow(dead_code)]
impl Pacer {
    /// Called when each packet has been written
    #[inline]
    pub fn on_packet_sent(
        &mut self,
        now: Timestamp,
        bytes_sent: usize,
        rtt_estimator: &RttEstimator,
        congestion_window: u32,
        max_datagram_size: u16,
        slow_start: bool,
    ) {
        if self.capacity == 0 {
            if let Some(next_packet_departure_time) = self.next_packet_departure_time {
                let interval = Self::interval(
                    rtt_estimator,
                    congestion_window,
                    max_datagram_size,
                    slow_start,
                );
                self.next_packet_departure_time =
                    Some((next_packet_departure_time + interval).max(now));
            } else {
                self.next_packet_departure_time = Some(now);
            }
            self.capacity = Counter::new((MAX_BURST_PACKETS * max_datagram_size) as u32);
        }

        self.capacity -= bytes_sent as u32;
    }

    /// Returns the earliest time that a packet may be transmitted.
    ///
    /// If the time is in the past or is `None`, the packet should be transmitted immediately.
    pub fn earliest_departure_time(&self) -> Option<Timestamp> {
        self.next_packet_departure_time
    }

    // Recalculate the interval between bursts of paced packets
    #[inline]
    fn interval(
        rtt_estimator: &RttEstimator,
        congestion_window: u32,
        max_datagram_size: u16,
        slow_start: bool,
    ) -> Duration {
        debug_assert_ne!(congestion_window, 0);

        let n = if slow_start { SLOW_START_N } else { N };

        // `MAX_BURST_PACKETS` is incorporated into the formula since we are trying to spread
        // bursts of packets evenly over time.
        let packet_size = (MAX_BURST_PACKETS * max_datagram_size) as u32;

        //= https://www.rfc-editor.org/rfc/rfc9002.txt#7.7
        //# A perfectly paced sender spreads packets exactly evenly over time.
        //# For a window-based congestion controller, such as the one in this
        //# document, that rate can be computed by averaging the congestion
        //# window over the RTT. Expressed as a rate in units of bytes per time,
        //# where congestion_window is in bytes:
        //#
        //# rate = N * congestion_window / smoothed_rtt
        //#
        //# Or expressed as an inter-packet interval in units of time:
        //#
        //# interval = ( smoothed_rtt * packet_size / congestion_window ) / N
        (rtt_estimator.smoothed_rtt() * packet_size / congestion_window) / n
    }
}

#[cfg(test)]
mod tests;
