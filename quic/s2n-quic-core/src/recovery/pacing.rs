// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Saturating},
    recovery::{
        bandwidth::Bandwidth, congestion_controller::Publisher, RttEstimator, MAX_BURST_PACKETS,
    },
    time::{Duration, Timestamp},
};
use num_rational::Ratio;

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.7
//# Using a value for "N" that is small, but at least 1 (for example, 1.25) ensures
//# that variations in RTT do not result in underutilization of the congestion window.
const N: Ratio<u64> = Ratio::new_raw(5, 4); // 5/4 = 1.25

// In Slow Start, the congestion window grows rapidly, so there is a higher likelihood the congestion
// window may be underutilized due to pacing. To prevent that, we use a higher value for `N` while
// in slow start, as done in Linux:
// https://github.com/torvalds/linux/blob/fc02cb2b37fe2cbf1d3334b9f0f0eab9431766c4/net/ipv4/tcp_input.c#L905-L906
const SLOW_START_N: Ratio<u64> = Ratio::new_raw(2, 1); // 2/1 = 2.00

// Jim Roskind demonstrated the second packet sent on a path has a higher probability of loss due to
// network routers being busy setting up routing tables triggered by the first packet. Setting this
// value to a Duration greater than zero will introduce that delay into the second packet.
// See https://www.ietf.org/proceedings/88/slides/slides-88-tsvarea-10.pdf
// TODO: Determine an appropriate value for this that balances improvements to 2nd packet loss and delay
pub const INITIAL_INTERVAL: Duration = Duration::from_millis(0);

/// low RTT networks should not be using pacing since it'll take longer to wake up from
/// a timer than it would to deliver a packet
pub const MINIMUM_PACING_RTT: Duration = Duration::from_millis(2);

/// A packet pacer that returns departure times that evenly distribute bursts of packets over time
#[derive(Clone, Debug, Default)]
pub struct Pacer {
    // The capacity of the current departure time slot
    capacity: Counter<u32, Saturating>,
    // The time the next packet should be transmitted
    next_packet_departure_time: Option<Timestamp>,
}

impl Pacer {
    /// Called when each packet has been written
    #[inline]
    pub fn on_packet_sent<Pub: Publisher>(
        &mut self,
        now: Timestamp,
        bytes_sent: usize,
        rtt_estimator: &RttEstimator,
        congestion_window: u32,
        max_datagram_size: u16,
        slow_start: bool,
        publisher: &mut Pub,
    ) {
        if rtt_estimator.smoothed_rtt() < MINIMUM_PACING_RTT {
            return;
        }

        if self.capacity == 0 {
            if let Some(next_packet_departure_time) = self.next_packet_departure_time {
                let interval = Self::interval(
                    rtt_estimator.smoothed_rtt(),
                    congestion_window,
                    max_datagram_size,
                    slow_start,
                    publisher,
                );
                self.next_packet_departure_time =
                    Some((next_packet_departure_time + interval).max(now));
            } else {
                self.next_packet_departure_time = Some(now + INITIAL_INTERVAL);
            }
            self.capacity = Counter::new(MAX_BURST_PACKETS * max_datagram_size as u32);
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
    fn interval<Pub: Publisher>(
        rtt: Duration,
        congestion_window: u32,
        max_datagram_size: u16,
        slow_start: bool,
        publisher: &mut Pub,
    ) -> Duration {
        debug_assert_ne!(congestion_window, 0);

        let n = if slow_start { SLOW_START_N } else { N };

        //= https://www.rfc-editor.org/rfc/rfc9002#section-7.7
        //# A perfectly paced sender spreads packets exactly evenly over time.
        //# For a window-based congestion controller, such as the one in this
        //# document, that rate can be computed by averaging the congestion
        //# window over the RTT. Expressed as a rate in units of bytes per time,
        //# where congestion_window is in bytes:
        //#
        //# rate = N * congestion_window / smoothed_rtt
        let pacing_rate = Bandwidth::new(congestion_window as u64, rtt) * n;

        // `MAX_BURST_PACKETS` is incorporated into the formula since we are trying to spread
        // bursts of packets evenly over time.
        let packet_size = MAX_BURST_PACKETS * max_datagram_size as u32;

        publisher.on_pacing_rate_updated(pacing_rate, packet_size, n);

        packet_size as u64 / pacing_rate
    }
}

#[cfg(test)]
mod tests;
