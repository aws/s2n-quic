// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    random,
    recovery::bbr::{startup, BbrCongestionController, State},
    time::Timestamp,
};
use num_rational::Ratio;
use num_traits::One;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.2
//# In Drain, BBR aims to quickly drain any queue created in Startup by switching to a
//# pacing_gain well below 1.0, until any estimated queue has been drained. It uses a
//# pacing_gain that is the inverse of the value used during Startup, chosen to try to
//# drain the queue in one round
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
    pub(super) fn enter_drain(&mut self) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.2
        //# BBREnterDrain():
        //#   BBR.state = Drain
        //#   BBR.pacing_gain = 1/BBRStartupCwndGain  /* pace slowly */
        //#   BBR.cwnd_gain = BBRStartupCwndGain      /* maintain cwnd */
        // pacing_gain and cwnd_gain are managed with the State enum

        // New BBR state requires updating the model
        self.try_fast_path = false;
        self.state.transition_to(State::Drain);
    }

    /// Checks if the `Drain` state is done and enters `ProbeBw` if so
    pub(super) fn check_drain_done<Rnd: random::Generator>(
        &mut self,
        random_generator: &mut Rnd,
        now: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.2
        //# BBRCheckDrain():
        //#   if (BBR.state == Drain and packets_in_flight <= BBRInflight(1.0))
        //#     BBREnterProbeBW()  /* BBR estimates the queue was drained */
        if self.state.is_drain()
            && self.bytes_in_flight <= self.inflight(self.data_rate_model.bw(), Ratio::one())
        {
            self.enter_probe_bw(false, random_generator, now);
        }
    }
}
