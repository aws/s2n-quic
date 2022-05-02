// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::recovery::{
    bandwidth::{Bandwidth, RateSample},
    bbr::{windowed_filter::WindowedMaxFilter, BETA},
};

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.1
//# The data rate model parameters together estimate both the sending rate required to reach the
//# full bandwidth available to the flow (BBR.max_bw), and the maximum pacing rate control parameter
//# that is consistent with the queue pressure objective (BBR.bw).

#[derive(Clone, Debug)]
pub(crate) struct Model {
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.1
    //# The windowed maximum recent bandwidth sample - obtained using the BBR delivery rate sampling
    //# algorithm [draft-cheng-iccrg-delivery-rate-estimation] - measured during the current or
    //# previous bandwidth probing cycle (or during Startup, if the flow is still in that state).
    max_bw_filter: WindowedMaxFilter<Bandwidth, core::num::Wrapping<u8>, core::num::Wrapping<u8>>,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.1
    //# The long-term maximum sending bandwidth that the algorithm estimates will produce acceptable
    //# queue pressure, based on signals in the current or previous bandwidth probing cycle, as
    //# measured by loss.
    bw_hi: Bandwidth,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.1
    //# The short-term maximum sending bandwidth that the algorithm estimates is safe for matching
    //# the current network path delivery rate, based on any loss signals in the current bandwidth
    //# probing cycle. This is generally lower than max_bw or bw_hi (thus the name).
    bw_lo: Bandwidth,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.1
    //# The maximum sending bandwidth that the algorithm estimates is appropriate for matching the
    //# current network path delivery rate, given all available signals in the model, at any time
    //# scale. It is the min() of max_bw, bw_hi, and bw_lo.
    bw: Bandwidth,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.11
    //# The virtual time used by the BBR.max_bw filter window.
    cycle_count: core::num::Wrapping<u8>,
}
#[allow(dead_code)] // TODO: Remove when used
impl Model {
    /// Constructs a new `data_rate::Model`
    pub fn new() -> Self {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.11
        //# The filter window length for BBR.MaxBwFilter = 2 (representing up to 2 ProbeBW cycles,
        //# the current cycle and the previous full cycle)
        const MAX_BW_FILTER_LEN: core::num::Wrapping<u8> = core::num::Wrapping(2);

        Self {
            max_bw_filter: WindowedMaxFilter::new(MAX_BW_FILTER_LEN),
            bw_hi: Bandwidth::MAX,
            bw_lo: Bandwidth::MAX,
            bw: Bandwidth::ZERO,
            cycle_count: Default::default(),
        }
    }

    /// The windowed maximum recent bandwidth sample
    pub fn max_bw(&self) -> Bandwidth {
        self.max_bw_filter.value().unwrap_or(Bandwidth::ZERO)
    }

    /// The long-term maximum sending bandwidth that the algorithm estimates
    /// will produce acceptable queue pressure
    pub fn bw_hi(&self) -> Bandwidth {
        self.bw_hi
    }

    /// The short-term maximum sending bandwidth that the algorithm estimates
    /// is safe for matching the current network path delivery rate
    pub fn bw_lo(&self) -> Bandwidth {
        self.bw_lo
    }

    /// The maximum sending bandwidth that the algorithm estimates is appropriate for
    /// matching the current network path delivery rate
    pub fn bw(&self) -> Bandwidth {
        self.bw
    }

    /// Increments the virtual time tracked for counting cyclical progression through ProbeBW cycles
    pub fn advance_max_bw_filter(&mut self) {
        self.cycle_count += core::num::Wrapping(1)
    }

    /// Updates `max_bw` with the given `rate_sample`
    pub fn update_max_bw(&mut self, rate_sample: RateSample) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.2.3
        //# By default, the estimator discards application-limited samples, since by definition they
        //# reflect application limits.  However, the estimator does use application-limited samples
        //# if the measured delivery rate happens to be larger than the current BBR.max_bw estimate,
        //# since this indicates the current BBR.Max_bw estimate is too low.
        if rate_sample.delivery_rate() > self.max_bw() || !rate_sample.is_app_limited {
            self.max_bw_filter
                .update(rate_sample.delivery_rate(), self.cycle_count);
        }
    }

    /// Updates `bw_hi` with the given `bw` if it exceeds the current `bw_hi`
    pub fn update_upper_bound(&mut self, bw: Bandwidth) {
        if self.bw_hi == Bandwidth::MAX {
            self.bw_hi = bw;
        } else {
            self.bw_hi = bw.max(self.bw_hi)
        }
    }

    /// Updates `bw_lo` with the given `bw` if it exceeds the current `bw_lo` * `bbr::BETA`
    pub fn update_lower_bound(&mut self, bw: Bandwidth) {
        if self.bw_lo == Bandwidth::MAX {
            self.bw_lo = self.max_bw()
        }

        self.bw_lo = bw.max(self.bw_lo * BETA);
    }

    /// Resets `bw_lo` to its initial value
    pub fn reset_lower_bound(&mut self) {
        self.bw_lo = Bandwidth::MAX
    }

    /// Bounds `bw` to min(`max_bw`, `bw_lo`, `bw_hi)
    pub fn bound_bw_for_model(&mut self) {
        self.bw = self.max_bw().min(self.bw_lo).min(self.bw_hi)
    }
}
