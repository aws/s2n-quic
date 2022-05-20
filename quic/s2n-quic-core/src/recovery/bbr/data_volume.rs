// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    recovery::{
        bandwidth::Bandwidth,
        bbr::{
            windowed_filter::{MinRttWindowedFilter, WindowedMaxFilter},
            BETA,
        },
    },
    time::Timestamp,
};
use core::time::Duration;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.2
//# The data volume model parameters together estimate both the volume of in-flight data required to
//# reach the full bandwidth available to the flow (BBR.max_inflight), and the maximum volume of
//# data that is consistent with the queue pressure objective (cwnd).

#[derive(Clone, Debug)]
pub(crate) struct Model {
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.2
    //# The windowed minimum round-trip time sample measured over the last MinRTTFilterLen = 10 seconds.
    min_rtt_filter: MinRttWindowedFilter,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.2
    //# A volume of data that is the estimate of the recent degree of aggregation in the network path.
    extra_acked_filter: WindowedMaxFilter<u64, u64, u64>,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.12
    //# the start of the time interval for estimating the excess amount of data acknowledged due to aggregation effects.
    extra_acked_interval_start: Timestamp,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.12
    //# the volume of data marked as delivered since BBR.extra_acked_interval_start.
    extra_acked_delivered: u64,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.2
    //# Analogous to BBR.bw_hi, the long-term maximum volume of in-flight data that the algorithm
    //# estimates will produce acceptable queue pressure, based on signals in the current or
    //# previous bandwidth probing cycle, as measured by loss.
    inflight_hi: u64,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.2
    //# Analogous to BBR.bw_lo, the short-term maximum volume of in-flight data that the algorithm
    //# estimates is safe for matching the current network path delivery process, based on any loss
    //# signals in the current bandwidth probing cycle.
    inflight_lo: u64,
}

#[allow(dead_code)] // TODO: Remove when used
impl Model {
    /// Constructs a new `data_volume::Model`
    pub fn new(now: Timestamp) -> Self {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.12
        //# The window length of the BBR.ExtraACKedFilter max filter window:
        //# 10 (in units of packet-timed round trips).
        const EXTRA_ACKED_FILTER_LEN: u64 = 10;

        Self {
            min_rtt_filter: MinRttWindowedFilter::new(),
            extra_acked_filter: WindowedMaxFilter::new(EXTRA_ACKED_FILTER_LEN),
            extra_acked_interval_start: now,
            extra_acked_delivered: 0,
            inflight_hi: u64::MAX,
            inflight_lo: u64::MAX,
        }
    }

    /// The windowed maximum recent estimate in bytes of the degree of aggregation in the path
    pub fn extra_acked(&self) -> u64 {
        self.extra_acked_filter.value().unwrap_or(0)
    }

    /// The windowed minimum round trip time
    pub fn min_rtt(&self) -> Option<Duration> {
        self.min_rtt_filter.min_rtt()
    }

    /// The long-term maximum volume of in-flight data that the algorithm
    /// estimates will produce acceptable queue pressure
    pub fn inflight_hi(&self) -> u64 {
        self.inflight_hi
    }

    /// The short-term maximum volume of in-flight data that the algorithm
    /// estimates is safe for matching the current network path delivery process
    pub fn inflight_lo(&self) -> u64 {
        self.inflight_lo
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

    /// Update the min_rtt estimate with the given `rtt`
    pub fn update_min_rtt(&mut self, rtt: Duration, now: Timestamp) {
        self.min_rtt_filter.update(rtt, now)
    }

    /// Update the ack aggregation estimate
    pub fn update_ack_aggregation(
        &mut self,
        bw: Bandwidth,
        bytes_acknowledged: usize,
        cwnd: u32,
        round_count: u64,
        now: Timestamp,
    ) {
        // Find excess ACKed beyond expected amount over this interval
        let interval = now - self.extra_acked_interval_start;
        let mut expected_delivered = bw * interval;
        // Reset interval if ACK rate is below expected rate
        if self.extra_acked_delivered <= expected_delivered {
            self.extra_acked_delivered = 0;
            self.extra_acked_interval_start = now;
            expected_delivered = 0;
        }
        self.extra_acked_delivered += bytes_acknowledged as u64;
        let extra = (self.extra_acked_delivered - expected_delivered).min(cwnd as u64);
        self.extra_acked_filter.update(extra, round_count);
    }

    /// Updates `inflight_hi` with the given `inflight_hi`
    pub fn update_upper_bound(&mut self, inflight_hi: u64) {
        self.inflight_hi = inflight_hi;
    }

    /// Updates `inflight_lo` with the given `inflight_latest` if it exceeds
    /// the current `inflight_lo` * `bbr::BETA`
    pub fn update_lower_bound(&mut self, cwnd: u32, inflight_latest: u64) {
        if self.inflight_lo == u64::MAX {
            self.inflight_lo = cwnd as u64;
        }

        self.inflight_lo = inflight_latest.max((BETA * self.inflight_lo).to_integer());
    }

    /// Resets `inflight_lo` to its initial value
    pub fn reset_lower_bound(&mut self) {
        self.inflight_lo = u64::MAX
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{Clock, NoopClock};

    #[test]
    fn new() {
        let now = NoopClock.get_time();
        let model = Model::new(now);

        assert_eq!(0, model.extra_acked());
        assert_eq!(None, model.min_rtt());
        assert_eq!(u64::MAX, model.inflight_hi());
        assert_eq!(u64::MAX, model.inflight_lo());
    }

    #[test]
    fn update_ack_aggregation() {
        let now = NoopClock.get_time();
        let mut model = Model::new(now);

        let now = now + Duration::from_millis(200);
        let bw = Bandwidth::new(1500, Duration::from_secs(1));

        // The first call to update_ack_aggregation starts a new ack aggregation epoch
        model.update_ack_aggregation(bw, 1600, 12000, 0, now);

        assert_eq!(1600, model.extra_acked());
        assert_eq!(now, model.extra_acked_interval_start);
        assert_eq!(1600, model.extra_acked_delivered);

        let now = now + Duration::from_secs(1);

        model.update_ack_aggregation(bw, 1600, 12000, 1, now);

        // The BW sample indicates 1500 bytes should be ACKed over the interval, but instead 1600 were,
        // meaning the extra acked amount is 100 bytes. This is added to the initial 1600 extra acked
        // amount to arrive at 1700 bytes.
        assert_eq!(1700, model.extra_acked());

        let now = now + Duration::from_secs(1);

        // Even more extra data is acked, but since the cwnd is lower than the extra amount, that
        // value is used as the extra acked (1600 bytes). 1700 remains the max extra acked.
        model.update_ack_aggregation(bw, 1700, 1600, 2, now);
        assert_eq!(1700, model.extra_acked());
    }

    #[test]
    fn update_lower_bound() {
        let now = NoopClock.get_time();
        let mut model = Model::new(now);

        model.update_lower_bound(1000, 100);

        // We didn't have a valid inflight_lo value yet, and the given inflight_latest is lower than cwnd * BETA,
        // so inflight_lo is set to cwnd * BETA
        assert_eq!((BETA * 1000).to_integer(), model.inflight_lo());

        model.update_upper_bound(50);

        // The new sample is lower than inflight_lo, so don't update inflight_lo
        assert_eq!((BETA * 1000).to_integer(), model.inflight_lo());

        let inflight_lo = 1500;
        model.update_lower_bound(1000, inflight_lo);

        // The new sample is higher than inflight_lo, so update inflight_lo
        assert_eq!(inflight_lo, model.inflight_lo());

        // Resetting the lower bound sets inflight_lo to u64::MAX
        model.reset_lower_bound();
        assert_eq!(u64::MAX, model.inflight_lo());
    }
}
