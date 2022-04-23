// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    recovery::{
        bandwidth::{Bandwidth, PacketInfo, RateSample},
        bbr::windowed_filter::{MinRttWindowedFilter, WindowedMaxFilter},
    },
    time::Timestamp,
};
use core::time::Duration;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.1
//# Several aspects of the BBR algorithm depend on counting the progress of "packet-timed" round
//# trips, which start at the transmission of some segment, and then end at the acknowledgement
//# of that segment. BBR.round_count is a count of the number of these "packet-timed" round trips
//# elapsed so far.
#[derive(Clone, Debug, Default)]
struct RoundCounter {
    /// The `delivered_bytes` at which the next round begins
    next_round_delivered_bytes: u64,
    /// True if the current ack being processed started a new round
    round_start: bool,
    /// The number of rounds counted over the lifetime of the path
    round_count: u64,
}
#[allow(dead_code)] // TODO: Remove when used
impl RoundCounter {
    /// Called for each acknowledgement of one or more packets
    pub fn on_ack(&mut self, packet_info: PacketInfo, delivered_bytes: u64) {
        if packet_info.delivered_bytes >= self.next_round_delivered_bytes {
            self.start(delivered_bytes);
            self.round_count += 1;
            self.round_start = true;
        } else {
            self.round_start = false;
        }
    }

    /// Starts a round that ends when the packet sent with `delivered_bytes` is acked
    pub fn start(&mut self, delivered_bytes: u64) {
        self.next_round_delivered_bytes = delivered_bytes;
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Model {
    round_counter: RoundCounter,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.1
    //# The windowed maximum recent bandwidth sample - obtained using the BBR delivery rate sampling
    //# algorithm [draft-cheng-iccrg-delivery-rate-estimation] - measured during the current or
    //# previous bandwidth probing cycle (or during Startup, if the flow is still in that state).
    max_bw_filter: WindowedMaxFilter<Bandwidth, core::num::Wrapping<u8>, core::num::Wrapping<u8>>,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.2
    //# The windowed minimum round-trip time sample measured over the last MinRTTFilterLen = 10 seconds.
    min_rtt_filter: MinRttWindowedFilter,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.11
    //# The virtual time used by the BBR.max_bw filter window.
    cycle_count: core::num::Wrapping<u8>,
}
#[allow(dead_code)] // TODO: Remove when used
impl Model {
    pub fn new() -> Self {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.11
        //# The filter window length for BBR.MaxBwFilter = 2 (representing up to 2 ProbeBW cycles,
        //# the current cycle and the previous full cycle)
        const MAX_BW_FILTER_LEN: core::num::Wrapping<u8> = core::num::Wrapping(2);

        Self {
            round_counter: Default::default(),
            max_bw_filter: WindowedMaxFilter::new(MAX_BW_FILTER_LEN),
            min_rtt_filter: MinRttWindowedFilter::new(),
            cycle_count: Default::default(),
        }
    }

    /// The windowed maximum recent bandwidth sample
    pub fn max_bw(&self) -> Bandwidth {
        self.max_bw_filter.value().unwrap_or(Bandwidth::ZERO)
    }

    /// The windowed minimum round trip time
    pub fn min_rtt(&self) -> Option<Duration> {
        self.min_rtt_filter.min_rtt()
    }

    /// True if the probe RTT has expired and is due for a refresh by entering the ProbeRTT state
    pub fn probe_rtt_expired(&self) -> bool {
        self.min_rtt_filter.probe_rtt_expired()
    }

    /// Overrides the last updated time for Min Probe RTT to ensure ProbeRTT is not entered until
    /// the next PROBE_RTT_INTERVAL.
    ///
    /// Called immediately after ProbeRTT period ends or after resuming from idle
    pub fn schedule_next_probe_rtt(&mut self, now: Timestamp) {
        self.min_rtt_filter.schedule_next_probe_rtt(now)
    }

    /// Called for each acknowledgement of one or more packets
    pub fn on_ack(
        &mut self,
        packet_info: PacketInfo,
        rate_sample: RateSample,
        delivered_bytes: u64,
        rtt: Duration,
        now: Timestamp,
    ) {
        self.round_counter.on_ack(packet_info, delivered_bytes);

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.2.3
        //# By default, the estimator discards application-limited samples, since by definition they
        //# reflect application limits.  However, the estimator does use application-limited samples
        //# if the measured delivery rate happens to be larger than the current BBR.max_bw estimate,
        //# since this indicates the current BBR.Max_bw estimate is too low.
        if rate_sample.delivery_rate() > self.max_bw() || !rate_sample.is_app_limited {
            self.max_bw_filter
                .update(rate_sample.delivery_rate(), self.cycle_count);
        }

        self.min_rtt_filter.update(rtt, now)
    }
}
