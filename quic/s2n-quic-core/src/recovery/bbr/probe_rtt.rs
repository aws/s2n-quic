// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    random,
    recovery::{
        bandwidth, bbr,
        bbr::{probe_rtt, round, BbrCongestionController},
        congestion_controller::Publisher,
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
    #[inline]
    pub(super) fn check_probe_rtt<Pub: Publisher>(
        &mut self,
        random_generator: &mut dyn random::Generator,
        now: Timestamp,
        publisher: &mut Pub,
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

        // `BBR.probe_rtt_done_stamp = 0`, which is equivalent to `probe_rtt::State.timer.cancel`, is
        // not necessary, as the timer is contained with the `probe_rtt::State`, and is thus unarmed
        // whenever BBR is not in the `ProbeRTT` state

        // `BBR.ack_phase = ACKS_PROBE_STOPPING` is not performed here as `ack_phase` is only used by
        // the `ProbeBW` state, which is initialized to `ACKS_PROBE_STOPPING` every time it is
        // reentered

        if !self.state.is_probing_rtt()
            && self.data_volume_model.probe_rtt_expired()
            && !self.idle_restart
        {
            // New BBR state requires updating the model
            self.try_fast_path = false;
            self.state
                .transition_to(bbr::State::ProbeRtt(State::default()), publisher);
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
            // The RFC pseudocode exits `ProbeRTT` internal to `BBRHandleProbeRTT`, whereas this
            // code checks if the `ProbeRTT` state is ready to exit here
            if probe_rtt_state.is_done(now) {
                self.exit_probe_rtt(random_generator, now, publisher);
            }
        }

        if self.bw_estimator.rate_sample().delivered_bytes > 0 {
            self.idle_restart = false;
        }
    }

    /// Exits the `ProbeRtt` state
    #[inline]
    pub(super) fn exit_probe_rtt<Pub: Publisher>(
        &mut self,
        random_generator: &mut dyn random::Generator,
        now: Timestamp,
        publisher: &mut Pub,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.4
        //# BBRCheckProbeRTTDone():
        //#  if (BBR.probe_rtt_done_stamp != 0 and
        //#      Now() > BBR.probe_rtt_done_stamp)
        //#    /* schedule next ProbeRTT: */
        //#    BBR.probe_rtt_min_stamp = Now()
        //#    BBRRestoreCwnd()
        //#    BBRExitProbeRTT()

        if cfg!(debug_assertions) {
            // BBR.probe_rtt_done_stamp != 0 and Now() > BBR.probe_rtt_done_stamp should be
            // checked by calling `probe_rtt_state.is_done(now)` prior to calling `exit_probe_rtt`
            assert!(self.state.is_probing_rtt());
            if let bbr::State::ProbeRtt(probe_rtt_state) = &self.state {
                assert!(probe_rtt_state.is_done(now));
            }
        }

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
            self.enter_probe_bw(cruise_immediately, random_generator, now, publisher);
        } else {
            self.enter_startup(publisher);
        }
    }

    /// Returns the congestion window that should be used during the `ProbeRTT` state
    #[inline]
    pub(super) fn probe_rtt_cwnd(&self) -> u32 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.5
        //# BBRProbeRTTCwnd():
        //#    probe_rtt_cwnd = BBRBDPMultiple(BBR.bw, BBRProbeRTTCwndGain)
        //#    probe_rtt_cwnd = max(probe_rtt_cwnd, BBRMinPipeCwnd)
        //#    return probe_rtt_cwnd

        self.bdp_multiple(self.data_rate_model.bw(), probe_rtt::CWND_GAIN)
            .try_into()
            .unwrap_or(u32::MAX)
            .max(self.minimum_window())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        event, path,
        path::MINIMUM_MAX_DATAGRAM_SIZE,
        recovery::{
            bandwidth::{Bandwidth, PacketInfo, RateSample},
            bbr::windowed_filter::PROBE_RTT_INTERVAL,
            congestion_controller::PathPublisher,
        },
        time::{Clock, NoopClock},
    };

    #[test]
    fn check_probe_rtt_enter_probe_rtt() {
        let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let now = NoopClock.get_time();
        let mut rng = random::testing::Generator::default();
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        bbr.bw_estimator.set_delivered_bytes_for_test(1000);

        bbr.data_volume_model
            .update_min_rtt(Duration::from_millis(100), now);
        let now = now + PROBE_RTT_INTERVAL;
        bbr.data_volume_model
            .update_min_rtt(Duration::from_millis(100), now);
        assert!(bbr.data_volume_model.probe_rtt_expired());
        assert!(!bbr.idle_restart);

        bbr.check_probe_rtt(&mut rng, now, &mut publisher);
        assert!(bbr.state.is_probing_rtt());
        assert!(!bbr.try_fast_path);
        assert_eq!(bbr.prior_cwnd, bbr.cwnd);
        assert_eq!(1000, bbr.round_counter.round_end());
    }

    #[test]
    fn check_probe_rtt_exit_probe_rtt() {
        let mut bbr = bbr_in_probe_rtt_ready_to_exit();
        let mut rng = random::testing::Generator::default();
        let now = NoopClock.get_time();
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        bbr.idle_restart = true;
        bbr.check_probe_rtt(&mut rng, now, &mut publisher);
        assert!(!bbr.state.is_probing_rtt());
        // No delivered bytes in the rate sample, so remain in idle restart
        assert!(bbr.idle_restart);

        // Next probe rtt is scheduled
        assert_eq!(
            Some(now + PROBE_RTT_INTERVAL),
            bbr.data_volume_model.next_probe_rtt()
        );

        let mut bbr = bbr_in_probe_rtt_ready_to_exit();
        bbr.idle_restart = true;
        bbr.bw_estimator.set_rate_sample_for_test(RateSample {
            delivered_bytes: 1000,
            ..Default::default()
        });

        bbr.check_probe_rtt(&mut rng, now, &mut publisher);
        assert!(!bbr.state.is_probing_rtt());
        // Positive elivered bytes in the rate sample, so set idle restart to false
        assert!(!bbr.idle_restart);

        // Next probe rtt is scheduled
        assert_eq!(
            Some(now + PROBE_RTT_INTERVAL),
            bbr.data_volume_model.next_probe_rtt()
        );
    }

    #[test]
    fn exit_probe_rtt() {
        let mut bbr = bbr_in_probe_rtt_ready_to_exit();
        let mut rng = random::testing::Generator::default();
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        let now = NoopClock.get_time();
        bbr.cwnd = 1000;
        bbr.prior_cwnd = 2000;
        bbr.data_volume_model.set_inflight_lo_for_test(100_000);
        bbr.data_rate_model
            .update_lower_bound(Bandwidth::new(1000, Duration::from_millis(1)));
        bbr.exit_probe_rtt(&mut rng, now, &mut publisher);

        // cwnd restored
        assert_eq!(2000, bbr.cwnd);
        // lower bounds reset
        assert_eq!(u64::MAX, bbr.data_volume_model.inflight_lo());
        assert_eq!(Bandwidth::INFINITY, bbr.data_rate_model.bw_lo());
        assert!(bbr.state.is_startup());

        // If full pipe then transition to probe bw cruise
        let mut bbr = bbr_in_probe_rtt_ready_to_exit();
        bbr.full_pipe_estimator.set_filled_pipe_for_test(true);
        bbr.exit_probe_rtt(&mut rng, now, &mut publisher);
        assert!(bbr.state.is_probing_bw_cruise());

        // Next probe rtt is scheduled
        assert_eq!(
            Some(now + PROBE_RTT_INTERVAL),
            bbr.data_volume_model.next_probe_rtt()
        );
    }

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.5
    //= type=test
    //# BBRProbeRTTCwnd():
    //#    probe_rtt_cwnd = BBRBDPMultiple(BBR.bw, BBRProbeRTTCwndGain)
    //#    probe_rtt_cwnd = max(probe_rtt_cwnd, BBRMinPipeCwnd)
    //#    return probe_rtt_cwnd
    #[test]
    fn probe_rtt_cwnd() {
        let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let now = NoopClock.get_time();

        // bdp_multiple > minimum_window
        assert_eq!(
            BbrCongestionController::initial_window(MINIMUM_MAX_DATAGRAM_SIZE),
            bbr.probe_rtt_cwnd()
        );

        bbr.data_volume_model
            .update_min_rtt(Duration::from_millis(100), now);

        // bdp_multiple < minimum_window
        assert_eq!(bbr.minimum_window(), bbr.probe_rtt_cwnd());
    }

    /// Helper method to return a BBR congestion controller in the ProbeRtt
    /// but ready to exit that state
    fn bbr_in_probe_rtt_ready_to_exit() -> BbrCongestionController {
        let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let now = NoopClock.get_time();
        let mut probe_rtt_state = probe_rtt::State {
            timer: Default::default(),
            round_done: false,
        };
        probe_rtt_state.timer.set(now);
        probe_rtt_state.round_done = true;
        bbr.state = bbr::State::ProbeRtt(probe_rtt_state);
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
        bbr.bw_estimator.set_delivered_bytes_for_test(200);
        bbr
    }
}
