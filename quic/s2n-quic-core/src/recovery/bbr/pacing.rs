// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Saturating},
    recovery::{
        bandwidth::Bandwidth,
        bbr::{BbrCongestionController, State},
        congestion_controller::Publisher,
        pacing::{INITIAL_INTERVAL, MINIMUM_PACING_RTT},
        MAX_BURST_PACKETS,
    },
    time::{Duration, Timestamp},
};
use num_rational::Ratio;

/// A packet pacer that returns departure times that evenly distribute bursts of packets over time
#[derive(Clone, Debug)]
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
        let pacing_rate =
            Self::bandwidth_to_pacing_rate(nominal_bandwidth, State::Startup.pacing_gain());

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

    /// Initialize the pacing rate with the given rtt and cwnd
    #[inline]
    pub(super) fn initialize_pacing_rate<Pub: Publisher>(
        &mut self,
        cwnd: u32,
        rtt: Duration,
        gain: Ratio<u64>,
        publisher: &mut Pub,
    ) {
        let bw = Bandwidth::new(cwnd as u64, rtt);

        let rate = Self::bandwidth_to_pacing_rate(bw, gain);
        self.pacing_rate = rate;
        publisher.on_pacing_rate_updated(rate, self.send_quantum as u32, gain);
    }

    /// Sets the pacing rate used for determining the earliest departure time
    #[inline]
    pub(super) fn set_pacing_rate<Pub: Publisher>(
        &mut self,
        bw: Bandwidth,
        gain: Ratio<u64>,
        filled_pipe: bool,
        publisher: &mut Pub,
    ) {
        let rate = Self::bandwidth_to_pacing_rate(bw, gain);

        if filled_pipe || rate > self.pacing_rate {
            self.pacing_rate = rate;
            publisher.on_pacing_rate_updated(rate, self.send_quantum as u32, gain);
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

    // Calculate the pacing rate based on the given bandwidth, pacing gain, and the pacing margin
    #[inline]
    fn bandwidth_to_pacing_rate(bw: Bandwidth, gain: Ratio<u64>) -> Bandwidth {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.5
        //# The static discount factor of 1% used to scale BBR.bw to produce BBR.pacing_rate.
        const PACING_MARGIN_PERCENT: u64 = 1;
        const PACING_RATIO: Ratio<u64> = Ratio::new_raw(100 - PACING_MARGIN_PERCENT, 100);

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.2
        //# BBRSetPacingRateWithGain(pacing_gain):
        //#   rate = pacing_gain * bw * (100 - BBRPacingMarginPercent) / 100
        //#   if (BBR.filled_pipe || rate > BBR.pacing_rate)
        //#     BBR.pacing_rate = rate
        bw * gain * PACING_RATIO
    }

    #[cfg(test)]
    pub fn set_send_quantum_for_test(&mut self, send_quantum: usize) {
        self.send_quantum = send_quantum
    }

    #[cfg(test)]
    pub fn pacing_rate(&self) -> Bandwidth {
        self.pacing_rate
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        event, path,
        path::MINIMUM_MAX_DATAGRAM_SIZE,
        recovery::{
            bandwidth::Bandwidth,
            bbr::{pacing::Pacer, State, State::Startup},
            congestion_controller::PathPublisher,
            pacing::INITIAL_INTERVAL,
        },
        time::{Clock, NoopClock},
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
        // nominal_bandwidth = 12_000 / 1ms = ~83.3333nanos/byte
        // pacing_rate = 2.77 * 83.333nanos/byte * 99% = ~30.388nanos/byte

        let pacer = Pacer::new(MINIMUM_MAX_DATAGRAM_SIZE);

        assert_eq!(
            Bandwidth::new(1000, Duration::from_nanos(30388)),
            pacer.pacing_rate
        );
    }

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.3
    //= type=test
    //# BBR.send_quantum = min(BBR.pacing_rate * 1ms, 64KBytes)
    #[test]
    fn max_send_quantum() {
        // BBR specifies a maximum send_quantum of 64KB, but since s2n-quic has a MAX_BURST_PACKETS
        // of 10 and 10 * MINIMUM_MAX_DATAGRMA_SIZE is less than 64KB, this limit will always be higher
        // than the limit s2n-quic imposes. This test ensures that this remains true if MAX_BURST_PACKETS
        // is increased.
        assert_eq!(Pacer::max_send_quantum(MINIMUM_MAX_DATAGRAM_SIZE), 12_000);
        assert!(Pacer::max_send_quantum(MINIMUM_MAX_DATAGRAM_SIZE) < 64_000);
    }

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.2
    //= type=test
    //# BBRSetPacingRateWithGain(pacing_gain):
    //#   rate = pacing_gain * bw * (100 - BBRPacingMarginPercent) / 100
    //#   if (BBR.filled_pipe || rate > BBR.pacing_rate)
    //#     BBR.pacing_rate = rate
    #[test]
    fn set_pacing_rate() {
        let mut pacer = Pacer::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        let bandwidth = Bandwidth::new(1000, Duration::from_millis(1));
        pacer.set_pacing_rate(bandwidth, Ratio::new(5, 4), true, &mut publisher);

        // pacing rate = pacing_gain * bw * (100 - BBRPacingMarginPercent) / 100
        //             = 1.25 * 1000bytes/ms * 99/100
        //             = 1237.5bytes/ms
        assert_eq!(
            Bandwidth::new(12375, Duration::from_millis(10)),
            pacer.pacing_rate
        );
    }

    #[test]
    fn initialize_pacing_rate() {
        let mut pacer = Pacer::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

        // pacing_rate = 14_000/100ms * 2.77 * .99 = 383_922 bytes/sec
        pacer.initialize_pacing_rate(
            14_000,
            Duration::from_millis(100),
            Startup.pacing_gain(),
            &mut publisher,
        );
        assert_eq!(
            Bandwidth::new(383_922, Duration::from_secs(1)),
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
        let mut pacer = Pacer::new(MINIMUM_MAX_DATAGRAM_SIZE);
        // pacing_rate < 1.2 Mbps, floor = MINIMUM_MAX_DATAGRAM_SIZE
        pacer.pacing_rate = Bandwidth::new(1_100_000 / 8, Duration::from_secs(1));
        pacer.set_send_quantum(MINIMUM_MAX_DATAGRAM_SIZE);
        // pacing_Rate * 1ms = 137 bytes
        // send_quantum = min(137, 12_000) = 137
        // send_quantum = max(137, MINIMUM_MAX_DATAGRAM_SIZE) = MINIMUM_MAX_DATAGRAM_SIZE
        assert_eq!(MINIMUM_MAX_DATAGRAM_SIZE as usize, pacer.send_quantum);

        // pacing_rate = 1.2 Mbps, floor = 2 * MINIMUM_MAX_DATAGRAM_SIZE
        pacer.pacing_rate = Bandwidth::new(1_200_000 / 8, Duration::from_secs(1));
        pacer.set_send_quantum(MINIMUM_MAX_DATAGRAM_SIZE);
        // pacing_Rate * 1ms = 150 bytes
        // send_quantum = min(150, 12_000) = 150
        // send_quantum = max(150, 2 * MINIMUM_MAX_DATAGRAM_SIZE) = 2 * MINIMUM_MAX_DATAGRAM_SIZE
        assert_eq!(2 * MINIMUM_MAX_DATAGRAM_SIZE as usize, pacer.send_quantum);

        // pacing_rate = 10.0 MBps, floor = 2 * MINIMUM_MAX_DATAGRAM_SIZE
        pacer.pacing_rate = Bandwidth::new(10_000_000, Duration::from_secs(1));
        pacer.set_send_quantum(MINIMUM_MAX_DATAGRAM_SIZE);
        // pacing_Rate * 1ms = 10000 bytes
        // send_quantum = min(10000, 12_000) = 10000
        // send_quantum = max(10000, 2 * MINIMUM_MAX_DATAGRAM_SIZE) = 10000
        assert_eq!(10000, pacer.send_quantum);

        // pacing_rate = 100.0 MBps, floor = 2 * MINIMUM_MAX_DATAGRAM_SIZE
        pacer.pacing_rate = Bandwidth::new(100_000_000, Duration::from_secs(1));
        pacer.set_send_quantum(MINIMUM_MAX_DATAGRAM_SIZE);
        // pacing_Rate * 1ms = 100000 bytes
        // send_quantum = min(100000, 12_000) = 12_000
        // send_quantum = max(12_000, 2 * MINIMUM_MAX_DATAGRAM_SIZE) = 12_000
        assert_eq!(12_000, pacer.send_quantum);
    }

    #[test]
    fn test_one_rtt() {
        let mut pacer = Pacer::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let now = NoopClock.get_time();
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

        let rtt = Duration::from_millis(100);
        let bw = Bandwidth::new(100_000, rtt);

        pacer.set_pacing_rate(bw, State::Startup.pacing_gain(), true, &mut publisher);

        let bytes_to_send = pacer.pacing_rate * rtt;

        // Send one packet to move beyond the initial interval
        pacer.on_packet_sent(now, MINIMUM_MAX_DATAGRAM_SIZE as usize, rtt);
        assert_eq!(
            Some(now + INITIAL_INTERVAL),
            pacer.earliest_departure_time()
        );

        let mut sent_bytes = 0;
        let now = now + INITIAL_INTERVAL;
        while sent_bytes < bytes_to_send {
            // Confirm the current departure time is less than 1 rtt
            assert!(pacer
                .earliest_departure_time()
                .map_or(true, |departure_time| departure_time < now + rtt));
            pacer.on_packet_sent(now, MINIMUM_MAX_DATAGRAM_SIZE as usize, rtt);
            sent_bytes += MINIMUM_MAX_DATAGRAM_SIZE as u64;
        }
        assert!(pacer
            .earliest_departure_time()
            .unwrap()
            .has_elapsed(now + rtt));
    }
}
