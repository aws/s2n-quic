// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    random,
    recovery::{
        bbr::{startup, BbrCongestionController, State},
        congestion_controller::Publisher,
    },
    time::Timestamp,
};
use num_rational::Ratio;
use num_traits::One;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.2
//# In Drain, BBR aims to quickly drain any queue created in Startup by switching to a
//# pacing_gain well below 1.0, until any estimated queue has been drained. It uses a
//# pacing_gain that is the inverse of the value used during Startup, chosen to try to
//# drain the queue in one round

// The wording above is somewhat ambiguous over whether the drain pacing_gain should be
// the inverse of the startup pacing_gain or startup cwnd_gain. However, the citation below
// makes it clear it is the inverse of the startup cwnd_gain. This is also supported
// by the following derivation:
// https://github.com/google/bbr/blob/master/Documentation/startup/gain/analysis/bbr_drain_gain.pdf

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.2
//#     BBR.pacing_gain = 1/BBRStartupCwndGain  /* pace slowly */
pub(crate) const PACING_GAIN: Ratio<u64> = Ratio::new_raw(1, 2);

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.2
//# BBREnterDrain():
//#     BBR.state = Drain
//#     BBR.pacing_gain = 1/BBRStartupCwndGain  /* pace slowly */
//#     BBR.cwnd_gain = BBRStartupCwndGain      /* maintain cwnd */
pub(crate) const CWND_GAIN: Ratio<u64> = startup::CWND_GAIN;

/// Methods related to the Drain state
impl BbrCongestionController {
    /// Enter the `Drain` state
    #[inline]
    pub(super) fn enter_drain<Pub: Publisher>(&mut self, publisher: &mut Pub) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.2
        //# BBREnterDrain():
        //#   BBR.state = Drain
        //#   BBR.pacing_gain = 1/BBRStartupCwndGain  /* pace slowly */
        //#   BBR.cwnd_gain = BBRStartupCwndGain      /* maintain cwnd */
        // pacing_gain and cwnd_gain are managed with the State enum

        // New BBR state requires updating the model
        self.try_fast_path = false;
        self.state.transition_to(State::Drain, publisher);
    }

    /// Checks if the `Drain` state is done and enters `ProbeBw` if so
    #[inline]
    pub(super) fn check_drain_done<Pub: Publisher>(
        &mut self,
        random_generator: &mut dyn random::Generator,
        now: Timestamp,
        publisher: &mut Pub,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.2
        //# BBRCheckDrain():
        //#   if (BBR.state == Drain and packets_in_flight <= BBRInflight(1.0))
        //#     BBREnterProbeBW()  /* BBR estimates the queue was drained */
        if self.state.is_drain()
            && self.bytes_in_flight <= self.inflight(self.data_rate_model.bw(), Ratio::one())
        {
            self.enter_probe_bw(false, random_generator, now, publisher);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        counter::Counter,
        event, path,
        path::MINIMUM_MAX_DATAGRAM_SIZE,
        random,
        recovery::{
            bandwidth::RateSample, bbr::BbrCongestionController,
            congestion_controller::PathPublisher,
        },
        time::{Clock, NoopClock},
    };
    use core::time::Duration;

    #[test]
    fn enter_drain() {
        let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

        bbr.enter_drain(&mut publisher);

        assert!(bbr.state.is_drain());
        assert!(!bbr.try_fast_path);
    }

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.2
    //= type=test
    //# BBRCheckDrain():
    //#   if (BBR.state == Drain and packets_in_flight <= BBRInflight(1.0))
    //#     BBREnterProbeBW()  /* BBR estimates the queue was drained */
    #[test]
    fn check_drain_done() {
        let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let now = NoopClock.get_time();
        let mut rng = random::testing::Generator::default();
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

        // Not in drain yet
        bbr.check_drain_done(&mut rng, now, &mut publisher);
        assert!(bbr.state.is_startup());

        bbr.enter_drain(&mut publisher);
        bbr.bytes_in_flight = Counter::new(100);

        // bytes_in_flight > inflight
        bbr.check_drain_done(&mut rng, now, &mut publisher);
        assert!(!bbr.state.is_drain());

        let rate_sample = RateSample {
            delivered_bytes: 100_000,
            interval: Duration::from_millis(1),
            ..Default::default()
        };
        bbr.data_rate_model.update_max_bw(rate_sample);
        bbr.data_rate_model.bound_bw_for_model();

        // Now drain is done
        bbr.check_drain_done(&mut rng, now, &mut publisher);
        assert!(bbr.state.is_probing_bw());
    }
}
