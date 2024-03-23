// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event::builder::SlowStartExitCause,
    recovery::{
        bbr::{BbrCongestionController, State},
        congestion_controller::Publisher,
    },
};
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
    #[inline]
    pub(super) fn enter_startup<Pub: Publisher>(&mut self, publisher: &mut Pub) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.1.1
        //# BBREnterStartup():
        //#   BBR.state = Startup
        //#   BBR.pacing_gain = BBRStartupPacingGain
        //#   BBR.cwnd_gain = BBRStartupCwndGain

        // pacing_gain and cwnd_gain are managed with the State enum

        // New BBR state requires updating the model
        self.try_fast_path = false;
        self.state.transition_to(State::Startup, publisher);
    }

    /// Checks if the `Startup` state is done and enters `Drain` if so
    #[inline]
    pub(super) fn check_startup_done<Pub: Publisher>(&mut self, publisher: &mut Pub) {
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
            self.full_pipe_estimator.on_loss_round_start(
                self.bw_estimator.rate_sample(),
                self.congestion_state.loss_bursts_in_round(),
                self.max_datagram_size,
            )
        }

        if self.state.is_startup() && self.full_pipe_estimator.filled_pipe() {
            publisher.on_slow_start_exited(SlowStartExitCause::Other, self.cwnd);
            self.enter_drain(publisher);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        event, path,
        path::MINIMUM_MAX_DATAGRAM_SIZE,
        recovery::{bandwidth::PacketInfo, bbr::probe_rtt, congestion_controller::PathPublisher},
        time::{Clock, NoopClock},
    };

    #[test]
    fn enter_startup() {
        let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

        // Startup can only be re-entered from ProbeRtt
        bbr.state = State::ProbeRtt(probe_rtt::State::default());

        bbr.enter_startup(&mut publisher);

        assert!(bbr.state.is_startup());
        assert!(!bbr.try_fast_path);
    }

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.1.1
    //= type=test
    //# BBRCheckStartupDone():
    //#   BBRCheckStartupFullBandwidth()
    //#   BBRCheckStartupHighLoss()
    //#   if (BBR.state == Startup and BBR.filled_pipe)
    //#     BBREnterDrain()
    #[test]
    fn check_startup_done() {
        let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

        // Not in startup
        bbr.state = State::ProbeRtt(probe_rtt::State::default());
        bbr.full_pipe_estimator.set_filled_pipe_for_test(true);

        bbr.check_startup_done(&mut publisher);

        assert!(bbr.state.is_probing_rtt());

        bbr.state = State::Startup;
        bbr.full_pipe_estimator.set_filled_pipe_for_test(false);

        // Filled pipe = false
        bbr.check_startup_done(&mut publisher);
        assert!(bbr.state.is_startup());

        // Now startup is done
        bbr.state = State::Startup;
        bbr.full_pipe_estimator.set_filled_pipe_for_test(true);
        bbr.check_startup_done(&mut publisher);
        assert!(bbr.state.is_drain());
    }

    #[test]
    fn check_startup_done_filled_pipe_on_round_start() {
        let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        let now = NoopClock.get_time();

        // Set ECN state to be too high, which would cause the full pipe estimator to be filled
        bbr.ecn_state.on_explicit_congestion(1000);
        bbr.ecn_state.on_round_start(
            1000 * MINIMUM_MAX_DATAGRAM_SIZE as u64,
            MINIMUM_MAX_DATAGRAM_SIZE,
        );
        assert!(!bbr.round_counter.round_start());

        // ECN must be too high over 2 rounds to fill the pipe
        bbr.check_startup_done(&mut publisher);
        bbr.check_startup_done(&mut publisher);

        // Still in startup since it wasn't the start of a round when ECN was measured
        assert!(!bbr.full_pipe_estimator.filled_pipe());
        assert!(bbr.state.is_startup());

        bbr.round_counter.set_round_end(100);
        let packet_info = PacketInfo {
            delivered_bytes: 100,
            delivered_time: now,
            lost_bytes: 0,
            ecn_ce_count: 0,
            first_sent_time: now,
            bytes_in_flight: 0,
            is_app_limited: false,
        };
        bbr.round_counter.on_ack(packet_info, 200);
        assert!(bbr.round_counter.round_start());

        // ECN must be too high over 2 rounds to fill the pipe
        bbr.check_startup_done(&mut publisher);
        bbr.check_startup_done(&mut publisher);

        assert!(bbr.full_pipe_estimator.filled_pipe());
        assert!(bbr.state.is_drain());
    }

    #[test]
    fn check_startup_done_filled_pipe_on_loss_round_start() {
        let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        let now = NoopClock.get_time();

        // Set loss to be too high, which would cause the full pipe estimator to be filled
        bbr.bw_estimator.on_loss(1000);
        bbr.bw_estimator.set_delivered_bytes_for_test(100);
        // 8 loss bursts must occur for the pipe to be full
        for _ in 0..8 {
            bbr.congestion_state.on_packet_lost(100, true);
        }
        assert!(!bbr.congestion_state.loss_round_start());
        bbr.check_startup_done(&mut publisher);

        // Still in startup since it wasn't the start of a loss round when loss was measured
        assert!(!bbr.full_pipe_estimator.filled_pipe());
        assert!(bbr.state.is_startup());

        let packet_info = PacketInfo {
            delivered_bytes: 100,
            delivered_time: now,
            lost_bytes: 0,
            ecn_ce_count: 0,
            first_sent_time: now,
            bytes_in_flight: 0,
            is_app_limited: false,
        };
        bbr.update_latest_signals(packet_info);
        assert!(bbr.congestion_state.loss_round_start());

        bbr.check_startup_done(&mut publisher);

        assert!(bbr.full_pipe_estimator.filled_pipe());
        assert!(bbr.state.is_drain());
    }
}
