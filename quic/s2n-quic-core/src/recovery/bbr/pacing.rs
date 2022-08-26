// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Saturating},
    recovery::{
        bandwidth::Bandwidth,
        bbr::{BbrCongestionController, State},
        pacing::{INITIAL_INTERVAL, MINIMUM_PACING_RTT},
        MAX_BURST_PACKETS,
    },
    time::{Duration, Timestamp},
};
use num_rational::Ratio;

/// A packet pacer that returns departure times that evenly distribute bursts of packets over time
#[derive(Clone, Debug, Default)]
pub struct Pacer {
    // The capacity of the current departure time slot
    capacity: Counter<u32, Saturating>,
    // The time the next packet should be transmitted
    next_packet_departure_time: Option<Timestamp>,
    // The current pacing rate for a BBR flow, which controls inter-packet spacing
    pacing_rate: Bandwidth,
    // The maximum size of a data aggregate scheduled and transmitted together
    send_quantum: usize,
}

impl Pacer {
    pub(super) fn new(max_datagram_size: u16) -> Self {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.2
        //# BBRInitPacingRate():
        //#   nominal_bandwidth = InitialCwnd / (SRTT ? SRTT : 1ms)
        //# BBR.pacing_rate =  BBRStartupPacingGain * nominal_bandwidth
        let initial_cwnd = BbrCongestionController::initial_window(max_datagram_size);
        let nominal_bandwidth = Bandwidth::new(initial_cwnd as u64, Duration::from_millis(1));
        let pacing_rate = nominal_bandwidth * State::Startup.pacing_gain();

        Self {
            capacity: Default::default(),
            next_packet_departure_time: None,
            pacing_rate,
            send_quantum: Self::max_send_quantum(max_datagram_size),
        }
    }

    /// Called when each packet has been written
    #[inline]
    pub fn on_packet_sent(&mut self, now: Timestamp, bytes_sent: usize, rtt: Duration) {
        if rtt < MINIMUM_PACING_RTT {
            return;
        }

        if self.capacity == 0 {
            if let Some(next_packet_departure_time) = self.next_packet_departure_time {
                self.next_packet_departure_time =
                    Some((next_packet_departure_time + self.interval()).max(now));
            } else {
                self.next_packet_departure_time = Some(now + INITIAL_INTERVAL);
            }
            self.capacity = Counter::new(self.send_quantum as u32);
        }

        self.capacity -= bytes_sent as u32;
    }

    /// Sets the pacing rate used for determining the earliest departure time
    #[inline]
    pub(super) fn set_pacing_rate(&mut self, bw: Bandwidth, gain: Ratio<u64>, filled_pipe: bool) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.5
        //# The static discount factor of 1% used to scale BBR.bw to produce BBR.pacing_rate.
        const PACING_MARGIN_PERCENT: u64 = 1;
        const PACING_RATIO: Ratio<u64> = Ratio::new_raw(100 - PACING_MARGIN_PERCENT, 100);

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.2
        //# BBRSetPacingRateWithGain(pacing_gain):
        //#   rate = pacing_gain * bw * (100 - BBRPacingMarginPercent) / 100
        //#   if (BBR.filled_pipe || rate > BBR.pacing_rate)
        //#     BBR.pacing_rate = rate
        let rate = bw * gain * PACING_RATIO;

        if filled_pipe || rate > self.pacing_rate {
            self.pacing_rate = rate;
        }
    }

    /// Sets the maximum size of data aggregate scheduled and transmitted together
    #[inline]
    pub(super) fn set_send_quantum(&mut self, max_datagram_size: u16) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.3
        //# if (BBR.pacing_rate < 1.2 Mbps)
        //#   floor = 1 * SMSS
        //# else
        //#   floor = 2 * SMSS
        //# BBR.send_quantum = min(BBR.pacing_rate * 1ms, 64KBytes)
        //# BBR.send_quantum = max(BBR.send_quantum, floor)

        // 1.2 Mbps
        const SEND_QUANTUM_THRESHOLD: Bandwidth =
            Bandwidth::new(1_200_000 / 8, Duration::from_secs(1));

        let floor = if self.pacing_rate < SEND_QUANTUM_THRESHOLD {
            max_datagram_size
        } else {
            max_datagram_size * 2
        } as usize;

        let send_quantum = (self.pacing_rate * Duration::from_millis(1)) as usize;
        self.send_quantum = send_quantum
            .max(floor)
            .min(Self::max_send_quantum(max_datagram_size));
    }

    /// Returns the earliest time that a packet may be transmitted.
    ///
    /// If the time is in the past or is `None`, the packet should be transmitted immediately.
    pub(super) fn earliest_departure_time(&self) -> Option<Timestamp> {
        self.next_packet_departure_time
    }

    /// Returns the maximum size of data aggregate scheduled and transmitted together
    pub(super) fn send_quantum(&self) -> usize {
        self.send_quantum
    }

    /// Returns the maximum value for send_quantum
    #[inline]
    fn max_send_quantum(max_datagram_size: u16) -> usize {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.3
        //= type=exception
        //= reason=QUIC recommends limiting bursts to the initial congestion window
        //# BBR.send_quantum = min(BBR.pacing_rate * 1ms, 64KBytes)
        MAX_BURST_PACKETS as usize * max_datagram_size as usize
    }

    // Recalculate the interval between bursts of paced packets
    #[inline]
    fn interval(&self) -> Duration {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.2
        //# BBR.next_departure_time = max(Now(), BBR.next_departure_time)
        //# packet.departure_time = BBR.next_departure_time
        //# pacing_delay = packet.size / BBR.pacing_rate

        self.send_quantum as u64 / self.pacing_rate
    }

    #[cfg(test)]
    pub fn set_send_quantum_for_test(&mut self, send_quantum: usize) {
        self.send_quantum = send_quantum
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        path::MINIMUM_MTU,
        recovery::{bandwidth::Bandwidth, bbr::pacing::Pacer},
    };
    use core::time::Duration;
    use num_rational::Ratio;

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.2
    //= type=test
    // BBRInitPacingRate():
    //     nominal_bandwidth = InitialCwnd / (SRTT ? SRTT : 1ms)
    //     BBR.pacing_rate =  BBRStartupPacingGain * nominal_bandwidth
    #[test]
    fn new() {
        // nominal_bandwidth = 12_000 / 1ms = ~83nanos/byte
        // pacing_rate = 2.77 * 83nanos/byte = ~29nanos/byte

        let pacer = Pacer::new(MINIMUM_MTU);

        assert_eq!(
            Bandwidth::new(1, Duration::from_nanos(29)),
            pacer.pacing_rate
        );
    }

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.3
    //= type=test
    //# BBR.send_quantum = min(BBR.pacing_rate * 1ms, 64KBytes)
    #[test]
    fn max_send_quantum() {
        // BBR specifies a maximum send_quantum of 64KB, but since s2n-quic has a MAX_BURST_PACKETS
        // of 10 and 10 * MINIMUM_MTU is less than 64KB, this limit will always be higher than the
        // limit s2n-quic imposes. This test ensures that this remains true if MAX_BURST_PACKETS is
        // increased.
        assert_eq!(Pacer::max_send_quantum(MINIMUM_MTU), 12_000);
        assert!(Pacer::max_send_quantum(MINIMUM_MTU) < 64_000);
    }

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.2
    //= type=test
    //# BBRSetPacingRateWithGain(pacing_gain):
    //#   rate = pacing_gain * bw * (100 - BBRPacingMarginPercent) / 100
    //#   if (BBR.filled_pipe || rate > BBR.pacing_rate)
    //#     BBR.pacing_rate = rate
    #[test]
    fn set_pacing_rate() {
        let mut pacer = Pacer::new(MINIMUM_MTU);
        let bandwidth = Bandwidth::new(1000, Duration::from_millis(1));
        pacer.set_pacing_rate(bandwidth, Ratio::new(5, 4), true);

        // pacing rate = pacing_gain * bw * (100 - BBRPacingMarginPercent) / 100
        //             = 1.25 * 1000bytes/ms * 99/100
        //             = 1237.5bytes/ms
        assert_eq!(
            Bandwidth::new(12375, Duration::from_millis(10)),
            pacer.pacing_rate
        );
    }

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.3
    //= type=test
    //# if (BBR.pacing_rate < 1.2 Mbps)
    //#   floor = 1 * SMSS
    //# else
    //#   floor = 2 * SMSS
    //# BBR.send_quantum = min(BBR.pacing_rate * 1ms, 64KBytes)
    //# BBR.send_quantum = max(BBR.send_quantum, floor)
    #[test]
    fn set_send_quantum() {
        let mut pacer = Pacer::new(MINIMUM_MTU);
        // pacing_rate < 1.2 Mbps, floor = MINIMUM_MTU
        pacer.pacing_rate = Bandwidth::new(1_100_000 / 8, Duration::from_secs(1));
        pacer.set_send_quantum(MINIMUM_MTU);
        // pacing_Rate * 1ms = 137 bytes
        // send_quantum = min(137, 12_000) = 137
        // send_quantum = max(137, MINIMUM_MTU) = MINIMUM_MTU
        assert_eq!(MINIMUM_MTU as usize, pacer.send_quantum);

        // pacing_rate = 1.2 Mbps, floor = 2 * MINIMUM_MTU
        pacer.pacing_rate = Bandwidth::new(1_200_000 / 8, Duration::from_secs(1));
        pacer.set_send_quantum(MINIMUM_MTU);
        // pacing_Rate * 1ms = 150 bytes
        // send_quantum = min(150, 12_000) = 150
        // send_quantum = max(150, 2 * MINIMUM_MTU) = 2 * MINIMUM_MTU
        assert_eq!(2 * MINIMUM_MTU as usize, pacer.send_quantum);

        // pacing_rate = 10.0 MBps, floor = 2 * MINIMUM_MTU
        pacer.pacing_rate = Bandwidth::new(10_000_000, Duration::from_secs(1));
        pacer.set_send_quantum(MINIMUM_MTU);
        // pacing_Rate * 1ms = 10000 bytes
        // send_quantum = min(10000, 12_000) = 10000
        // send_quantum = max(10000, 2 * MINIMUM_MTU) = 10000
        assert_eq!(10000, pacer.send_quantum);

        // pacing_rate = 100.0 MBps, floor = 2 * MINIMUM_MTU
        pacer.pacing_rate = Bandwidth::new(100_000_000, Duration::from_secs(1));
        pacer.set_send_quantum(MINIMUM_MTU);
        // pacing_Rate * 1ms = 100000 bytes
        // send_quantum = min(100000, 12_000) = 12_000
        // send_quantum = max(12_000, 2 * MINIMUM_MTU) = 12_000
        assert_eq!(12_000, pacer.send_quantum);
    }
}
