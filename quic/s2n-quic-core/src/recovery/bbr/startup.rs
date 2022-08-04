// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::recovery::bbr::{BbrCongestionController, State};
use num_rational::Ratio;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.6
//# A constant specifying the minimum gain value for calculating the pacing rate that will
//# allow the sending rate to double each round (4*ln(2) ~= 2.77)
pub(crate) const PACING_GAIN: Ratio<u64> = Ratio::new_raw(277, 100);

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.6
//# A constant specifying the minimum gain value for calculating the
//# cwnd that will allow the sending rate to double each round (2.0)
pub(crate) const CWND_GAIN: Ratio<u64> = Ratio::new_raw(2, 1);

/// Methods related to the Startup state
impl BbrCongestionController {
    /// Enter the `Startup` state
    pub(super) fn enter_startup(&mut self) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.1.1
        //# BBREnterStartup():
        //#   BBR.state = Startup
        //#   BBR.pacing_gain = BBRStartupPacingGain
        //#   BBR.cwnd_gain = BBRStartupCwndGain

        // pacing_gain and cwnd_gain are managed with the State enum

        self.state.transition_to(State::Startup);
    }

    /// Checks if the `Startup` state is done and enters `Drain` if so
    pub(super) fn check_startup_done(&mut self) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.1.1
        //# BBRCheckStartupDone():
        //#   BBRCheckStartupFullBandwidth()
        //#   BBRCheckStartupHighLoss()
        //#   if (BBR.state == Startup and BBR.filled_pipe)
        //#     BBREnterDrain()
        if self.round_counter.round_start() {
            self.full_pipe_estimator.on_round_start(
                self.bw_estimator.rate_sample(),
                self.data_rate_model.max_bw(),
                self.ecn_state.is_ce_too_high_in_round(),
            );
        }

        if self.congestion_state.loss_round_start() {
            // Excessive inflight is checked at the end of a loss round, not a regular round, as done
            // in tcp_bbr2.c/bbr2_check_loss_too_high_in_startup
            //
            // See https://github.com/google/bbr/blob/1a45fd4faf30229a3d3116de7bfe9d2f933d3562/net/ipv4/tcp_bbr2.c#L2133
            self.full_pipe_estimator
                .on_loss_round_start(self.bw_estimator.rate_sample(), self.max_datagram_size)
        }

        if self.state.is_startup() && self.full_pipe_estimator.filled_pipe() {
            self.enter_drain();
        }
    }
}
