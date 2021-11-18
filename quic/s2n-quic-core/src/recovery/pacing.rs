// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    recovery::RttEstimator,
    time::{Duration, Timestamp},
};

//= https://www.rfc-editor.org/rfc/rfc9002.txt#7.7
//# Using a value for "N" that is small, but at least 1 (for example, 1.25) ensures
//# that variations in RTT do not result in underutilization of the congestion window.
const N: f32 = 1.25;

// In Slow Start, the congestion window grows rapidly, so there is a higher likelihood the congestion
// window may be underutilized due to pacing. To prevent that, we use a higher value for `N` while
// in slow start, as done in Linux:
// https://github.com/torvalds/linux/blob/fc02cb2b37fe2cbf1d3334b9f0f0eab9431766c4/net/ipv4/tcp_input.c#L905-L906
const SLOW_START_N: f32 = 2.0;

// TODO: this should be aligned with GSO max segments
const MAX_BURST_PACKETS: u16 = 10;

pub struct Pacer {
    capacity: usize,
    next_packet_departure_time: Option<Timestamp>,
}

// TODO: Remove when used
#[allow(dead_code)]
impl Pacer {
    /// Constructs a new `Pacer` with the given `max_datagram_size`
    pub fn new(max_datagram_size: u16) -> Self {
        Self {
            capacity: (MAX_BURST_PACKETS * max_datagram_size) as usize,
            next_packet_departure_time: None,
        }
    }

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
        match self.next_packet_departure_time {
            None => self.next_packet_departure_time = Some(now),
            Some(next_packet_departure_time) if self.capacity == 0 => {
                let interval = Self::interval(
                    rtt_estimator,
                    congestion_window,
                    max_datagram_size,
                    slow_start,
                );
                self.next_packet_departure_time =
                    Some((next_packet_departure_time + interval).max(now));
                self.capacity = (MAX_BURST_PACKETS * max_datagram_size) as usize;
            }
            Some(_) => {}
        }

        self.capacity = self.capacity.saturating_sub(bytes_sent);
    }

    /// Returns the earliest time that a packet may be transmitted, or `None` if there is
    /// no restriction on the transmission time.
    pub fn earliest_departure_time(&self) -> Option<Timestamp> {
        self.next_packet_departure_time
    }

    // Recalculate the interval between batches of paced packets
    #[inline]
    fn interval(
        rtt_estimator: &RttEstimator,
        congestion_window: u32,
        max_datagram_size: u16,
        slow_start: bool,
    ) -> Duration {
        let n = if slow_start { SLOW_START_N } else { N };

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
        // `MAX_BURST_PACKETS` is incorporated into the formula since we are trying to spread
        // batches of packets evenly over time.
        Duration::from_secs_f32(
            (rtt_estimator.smoothed_rtt() * (MAX_BURST_PACKETS * max_datagram_size) as u32
                / congestion_window)
                .as_secs_f32()
                / n,
        )
    }
}

#[cfg(test)]
mod tests;
