// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    random,
    recovery::{
        bandwidth, bbr,
        bbr::{probe_rtt, round, BbrCongestionController},
    },
    time::{Timer, Timestamp},
};
use core::time::Duration;
use num_rational::Ratio;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.14.2
//# A constant specifying the minimum duration for which ProbeRTT state
//# holds inflight to BBRMinPipeCwnd or fewer packets: 200 ms.
const PROBE_RTT_DURATION: Duration = Duration::from_millis(200);

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.4
//# BBREnterProbeRTT():
//#     BBR.state = ProbeRTT
//#     BBR.pacing_gain = 1
pub(crate) const PACING_GAIN: Ratio<u64> = Ratio::new_raw(1, 1);

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.14.2
//# A constant specifying the gain value for calculating the cwnd during ProbeRTT: 0.5
pub(crate) const CWND_GAIN: Ratio<u64> = Ratio::new_raw(1, 2);

#[derive(Clone, Debug, Default)]
pub(crate) struct State {
    timer: Timer,
    round_done: bool,
}

impl State {
    /// Keeps BBR in the `ProbeRTT` state for max of (PROBE_RTT_DURATION, 1 round)
    fn handle_probe_rtt(
        &mut self,
        bw_estimator: &mut bandwidth::Estimator,
        round_counter: &mut round::Counter,
        probe_rtt_cwnd: u32,
        bytes_in_flight: u32,
        now: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.4
        //# BBRHandleProbeRTT()
        //#     /* Ignore low rate samples during ProbeRTT: */
        //#     MarkConnectionAppLimited()
        //#     if (BBR.probe_rtt_done_stamp == 0 and
        //#         packets_in_flight <= BBRProbeRTTCwnd())
        //#       /* Wait for at least ProbeRTTDuration to elapse: */
        //#      BBR.probe_rtt_done_stamp =
        //#         Now() + ProbeRTTDuration
        //#       /* Wait for at least one round to elapse: */
        //#       BBR.probe_rtt_round_done = false
        //#       BBRStartRound()
        //#     else if (BBR.probe_rtt_done_stamp != 0)
        //#       if (BBR.round_start)
        //#         BBR.probe_rtt_round_done = true
        //#       if (BBR.probe_rtt_round_done)
        //#         BBRCheckProbeRTTDone()

        // Ignore low rate samples during ProbeRTT
        bw_estimator.on_app_limited(bytes_in_flight);

        if !self.timer.is_armed() && bytes_in_flight <= probe_rtt_cwnd {
            // Wait for at least ProbeRTTDuration to elapse:
            self.timer.set(now + PROBE_RTT_DURATION);
            // Wait for at least one round to elapse:
            self.round_done = false;
            round_counter.set_round_end(bw_estimator.delivered_bytes());
        } else if self.timer.is_armed() && round_counter.round_start() {
            self.round_done = true;
        }
    }

    /// Returns true if the `ProbeRtt` state is done and should be exited
    pub fn is_done(&self, now: Timestamp) -> bool {
        self.round_done && self.timer.is_expired(now)
    }
}

/// Methods related to the `ProbeRtt` state
impl BbrCongestionController {
    /// Check if it is time to start probing for RTT changes, and enter the ProbeRtt state if so
    pub fn check_probe_rtt<Rnd: random::Generator>(
        &mut self,
        random_generator: &mut Rnd,
        now: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.4
        //# BBRCheckProbeRTT()
        //#    if (BBR.state != ProbeRTT and
        //#         BBR.probe_rtt_expired and
        //#         not BBR.idle_restart)
        //#       BBREnterProbeRTT()
        //#       BBRSaveCwnd()
        //#       BBR.probe_rtt_done_stamp = 0
        //#       BBR.ack_phase = ACKS_PROBE_STOPPING
        //#      BBRStartRound()
        //#     if (BBR.state == ProbeRTT)
        //#       BBRHandleProbeRTT()
        //#     if (rs.delivered > 0)
        //#       BBR.idle_restart = false

        if !self.state.is_probing_rtt()
            && self.data_volume_model.probe_rtt_expired()
            && !self.idle_restart
        {
            self.state = bbr::State::ProbeRtt(State::default());
            self.save_cwnd();
            self.round_counter
                .set_round_end(self.bw_estimator.delivered_bytes());
        }

        let probe_rtt_cwnd = self.probe_rtt_cwnd();
        if let bbr::State::ProbeRtt(probe_rtt_state) = &mut self.state {
            probe_rtt_state.handle_probe_rtt(
                &mut self.bw_estimator,
                &mut self.round_counter,
                probe_rtt_cwnd,
                *self.bytes_in_flight,
                now,
            );
            if probe_rtt_state.is_done(now) {
                self.exit_probe_rtt(random_generator, now);
            }
        }

        if self.bw_estimator.rate_sample().delivered_bytes > 0 {
            self.idle_restart = false;
        }
    }

    /// Exits the `ProbeRtt` state
    pub fn exit_probe_rtt<Rnd: random::Generator>(
        &mut self,
        random_generator: &mut Rnd,
        now: Timestamp,
    ) {
        // schedule next ProbeRTT:
        self.data_volume_model.schedule_next_probe_rtt(now);
        self.restore_cwnd();

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.5
        //# BBRExitProbeRTT()
        //#     BBRResetLowerBounds()
        //#     if (BBR.filled_pipe)
        //#       BBRStartProbeBW_DOWN()
        //#       BBRStartProbeBW_CRUISE()
        //#     else
        //#       BBREnterStartup()

        self.data_volume_model.reset_lower_bound();
        self.data_rate_model.reset_lower_bound();

        if self.full_pipe_estimator.filled_pipe() {
            //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.5
            //# as an optimization, since the connection is exiting ProbeRTT, we know that infligh
            //# is already below the estimated BDP, so the connection can proceed immediately to
            //# ProbeBW_CRUISE
            let cruise_immediately = true;
            self.enter_probe_bw(cruise_immediately, random_generator, now);
        } else {
            self.enter_startup();
        }
    }

    /// Returns the congestion window that should be used during the `ProbeRTT` state
    pub fn probe_rtt_cwnd(&self) -> u32 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.5
        //# BBRProbeRTTCwnd():
        //#    probe_rtt_cwnd = BBRBDPMultiple(BBR.bw, BBRProbeRTTCwndGain)
        //#    probe_rtt_cwnd = max(probe_rtt_cwnd, BBRMinPipeCwnd)
        //#    return probe_rtt_cwnd#

        self.bdp_multiple(self.data_rate_model.bw(), probe_rtt::CWND_GAIN)
            .try_into()
            .unwrap_or(u32::MAX)
            .max(self.minimum_window())
    }
}

#[cfg(test)]
mod tests {
    // TODO
}
