// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    random,
    recovery::bbr::{probe_bw::AckPhase, BbrCongestionController},
    time::Timestamp,
};
use core::time::Duration;
use num_rational::Ratio;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.14.2
//# A constant specifying the minimum duration for which ProbeRTT state
//# holds inflight to BBRMinPipeCwnd or fewer packets: 200 ms.
const PROBE_RTT_DURATION: Duration = Duration::from_millis(200);

#[derive(Clone, Debug)]
pub(crate) struct State {
    done_timestamp: Option<Timestamp>,
    round_done: bool,
}

/// Methods related to the ProbeRtt state
impl BbrCongestionController {
    pub fn check_probe_rtt<Rnd: random::Generator>(
        &mut self,
        bytes_in_flight: u32,
        random_generator: &mut Rnd,
        now: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.4
        //# BBRCheckProbeRTT()

        // TODO: check BBR.state != ProbeRtt
        if self.data_volume_model.probe_rtt_expired() && !self.idle_restart {
            // TODO: self.state = ProbeRtt
            self.save_cwnd();
            self.probe_rtt_state.done_timestamp = None;
            self.probe_bw_state.ack_phase = AckPhase::ProbeStopping;
            self.round_counter
                .set_round_end(self.bw_estimator.delivered_bytes());
        }

        // TODO: if BBR.state == ProbeRtt
        self.handle_probe_rtt(bytes_in_flight, random_generator, now);

        if self.bw_estimator.rate_sample().delivered_bytes > 0 {
            self.idle_restart = false;
        }
    }

    fn handle_probe_rtt<Rnd: random::Generator>(
        &mut self,
        bytes_in_flight: u32,
        random_generator: &mut Rnd,
        now: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.4
        //# BBRHandleProbeRTT()

        // Ignore low rate samples during ProbeRTT
        self.bw_estimator.mark_app_limited(bytes_in_flight);

        if self.probe_rtt_state.done_timestamp.is_none() && bytes_in_flight <= self.probe_rtt_cwnd()
        {
            /* Wait for at least ProbeRTTDuration to elapse: */
            self.probe_rtt_state.done_timestamp = Some(now + PROBE_RTT_DURATION);
            /* Wait for at least one round to elapse: */
            self.probe_rtt_state.round_done = false;
            self.round_counter
                .set_round_end(self.bw_estimator.delivered_bytes());
        } else if self.probe_rtt_state.done_timestamp.is_some() {
            if self.round_counter.round_start() {
                self.probe_rtt_state.round_done = true;
            }
            if self.probe_rtt_state.round_done {
                self.check_probe_rtt_done(random_generator, now)
            }
        }
    }

    pub fn check_probe_rtt_done<Rnd: random::Generator>(
        &mut self,
        random_generator: &mut Rnd,
        now: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.4
        //# BBRCheckProbeRTT()

        if self
            .probe_rtt_state
            .done_timestamp
            .map_or(false, |probe_rtt_done_timestamp| {
                now > probe_rtt_done_timestamp
            })
        {
            /* schedule next ProbeRTT: */
            self.data_volume_model.schedule_next_probe_rtt(now);
            self.restore_cwnd();
            self.exit_probe_rtt(random_generator, now);
        }
    }

    pub fn exit_probe_rtt<Rnd: random::Generator>(
        &mut self,
        random_generator: &mut Rnd,
        now: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.5
        //# BBRExitProbeRTT()

        self.data_volume_model.reset_lower_bound();
        self.data_rate_model.reset_lower_bound();

        if self.full_pipe_estimator.filled_pipe() {
            self.probe_bw_state.start_down(
                &mut self.congestion_state,
                &mut self.round_counter,
                self.bw_estimator.delivered_bytes(),
                random_generator,
                now,
            );
            self.probe_bw_state.start_cruise();
        }
    }

    pub fn probe_rtt_cwnd(&self) -> u32 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.5
        //# BBRProbeRTTCwnd()

        let probe_rtt_cwnd_gain = Ratio::new(1u64, 2u64); // TODO State::ProbeRtt.cwnd_gain()
        self.bdp_multiple(self.data_rate_model.bw(), probe_rtt_cwnd_gain)
            .try_into()
            .unwrap_or(u32::MAX)
            .max(self.minimum_window())
    }
}
