// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    recovery::{
        bandwidth::{Bandwidth, RateSample},
        bbr,
        bbr::{congestion, data_rate, data_volume, round, BbrCongestionController},
        CongestionController,
    },
    time::Timestamp,
};
use core::time::Duration;
use num_rational::Ratio;
use num_traits::One;

const MAX_BW_PROBE_UP_ROUNDS: u8 = 30;

/// Max number of packet-timed rounds to wait before probing for bandwidth
const MAX_BW_PROBE_ROUNDS: u32 = 63;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3
//# a BBR flow in ProbeBW mode cycles through the four
//# Probe bw states - DOWN, CRUISE, REFILL, and UP
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CyclePhase {
    /// Send slower than the network is delivering data, to reduce the amount of data in flight
    Down,
    /// Send at the same rate the network is delivering data
    Cruise,
    /// Try to fully utilize the network bottleneck without creating any significant queue pressure
    Refill,
    /// Probe for possible increases in available bandwidth
    Up,
}

impl CyclePhase {
    /// The dynamic gain factor used to scale BBR.bw to produce BBR.pacing_rate
    pub fn pacing_gain(&self) -> Ratio<u64> {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.1
        //# In the ProbeBW_DOWN phase of the cycle, a BBR flow pursues the deceleration tactic,
        //# to try to send slower than the network is delivering data, to reduce the amount of data
        //# in flight, with all of the standard motivations for the deceleration tactic (discussed
        //# in "State Machine Tactics", above). It does this by switching to a BBR.pacing_gain of
        //# 0.9, sending at 90% of BBR.bw.
        const DOWN_PACING_GAIN: Ratio<u64> = Ratio::new_raw(9, 10);
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.4
        //# After ProbeBW_REFILL refills the pipe, ProbeBW_UP probes for possible increases in
        //# available bandwidth by using a BBR.pacing_gain of 1.25, sending faster than the current
        //# estimated available bandwidth.
        const UP_PACING_GAIN: Ratio<u64> = Ratio::new_raw(5, 4);
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.3
        //# During ProbeBW_REFILL BBR uses a BBR.pacing_gain of 1.0, to send at a rate that
        //# matches the current estimated available bandwidth

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.2
        //# In the ProbeBW_CRUISE phase of the cycle, a BBR flow pursues the "cruising" tactic
        //# (discussed in "State Machine Tactics", above), attempting to send at the same rate
        //# the network is delivering data. It tries to match the sending rate to the flow's
        //# current available bandwidth, to try to achieve high utilization of the available
        //# bandwidth without increasing queue pressure. It does this by switching to a
        //# pacing_gain of 1.0, sending at 100% of BBR.bw. N
        const CRUISE_REFILL_PACING_GAIN: Ratio<u64> = Ratio::new_raw(1, 1);

        match self {
            CyclePhase::Down => DOWN_PACING_GAIN,
            CyclePhase::Cruise | CyclePhase::Refill => CRUISE_REFILL_PACING_GAIN,
            CyclePhase::Up => UP_PACING_GAIN,
        }
    }
}

/// How the incoming ACK stream relates to our bandwidth probing
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AckPhase {
    /// not probing; not getting probe feedback
    Init,
    /// stopped probing; still getting feedback
    ProbeStopping,
    /// sending at est. bw to fill pipe
    Refilling,
    /// inflight rising to probe bw
    ProbeStarting,
    /// getting feedback from bw probing
    ProbeFeedback,
}

#[derive(Clone, Debug)]
pub(crate) struct State {
    /// The current mode for deciding how fast to send
    cycle_phase: CyclePhase,
    /// How the incoming ACK stream relates to our bandwidth probing
    ack_phase: AckPhase,
    /// A random duration to wait until probing for bandwidth
    bw_probe_wait: Duration,
    /// Packet-timed rounds since probed bw
    rounds_since_bw_probe: u8,
    /// Packets delivered per inflight_hi increment
    bw_probe_up_cnt: u32,
    /// Packets ACKed since inflight_hi increment
    bw_probe_up_acks: u32,
    /// cwnd-limited rounds in PROBE_UP
    bw_probe_up_rounds: u8,
    /// True if the rate samples reflect bandwidth probing
    bw_probe_samples: bool,
    /// Time of this cycle phase start
    cycle_stamp: Option<Timestamp>,
}

impl State {
    #[allow(dead_code)] // TODO: Remove when used
    pub fn new() -> Self {
        Self {
            cycle_phase: CyclePhase::Down,
            ack_phase: AckPhase::Init,
            bw_probe_wait: Duration::ZERO,
            rounds_since_bw_probe: 0,
            bw_probe_up_cnt: 0,
            bw_probe_up_acks: 0,
            bw_probe_up_rounds: 0,
            bw_probe_samples: false,
            cycle_stamp: None,
        }
    }

    /// Returns the current `probe_bw::CyclePhase`
    pub fn cycle_phase(&self) -> CyclePhase {
        self.cycle_phase
    }

    pub fn check_time_to_probe_bw(
        &mut self,
        target_inflight: u32,
        max_data_size: u16,
        now: Timestamp,
    ) -> bool {
        debug_assert!(
            self.cycle_phase == CyclePhase::Down || self.cycle_phase == CyclePhase::Cruise
        );

        if self.has_elapsed_in_phase(self.bw_probe_wait, now)
            || self.is_reno_coexistence_probe_time(target_inflight, max_data_size)
        {
            return true;
        }
        false
    }

    pub fn probe_inflight_hi_upward(
        &mut self,
        bytes_acknowledged: usize,
        data_volume_model: &mut data_volume::Model,
        cwnd: u32,
        max_data_size: u16,
        round_start: bool,
    ) {
        self.bw_probe_up_acks += bytes_acknowledged as u32;
        if self.bw_probe_up_acks >= self.bw_probe_up_cnt {
            let delta = self.bw_probe_up_acks / self.bw_probe_up_cnt;
            self.bw_probe_up_acks -= delta * self.bw_probe_up_cnt;
            let inflight_hi = data_volume_model.inflight_hi() + delta as u64;
            data_volume_model.update_upper_bound(inflight_hi);
        }
        if round_start {
            self.raise_inflight_hi_slope(cwnd, max_data_size);
        }
    }

    /// Raise inflight_hi slope if appropriate
    fn raise_inflight_hi_slope(&mut self, cwnd: u32, max_data_size: u16) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.4
        //# BBR takes an approach where the additive increase to BBR.inflight_hi
        //# exponentially doubles each round trip
        let growth_this_round = max_data_size << self.bw_probe_up_rounds;
        self.bw_probe_up_rounds = (self.bw_probe_up_rounds + 1).min(MAX_BW_PROBE_UP_ROUNDS);
        self.bw_probe_up_cnt = (cwnd / growth_this_round as u32).max(1);
    }

    fn has_elapsed_in_phase(&self, interval: Duration, now: Timestamp) -> bool {
        self.cycle_stamp
            .map_or(false, |cycle_stamp| now > cycle_stamp + interval)
    }

    fn is_reno_coexistence_probe_time(&self, target_inflight: u32, max_data_size: u16) -> bool {
        let rounds = target_inflight.min(MAX_BW_PROBE_ROUNDS * max_data_size as u32);
        self.rounds_since_bw_probe as u32 >= rounds
    }

    fn start_probe_bw_cruise(&mut self) {
        debug_assert_eq!(self.cycle_phase, CyclePhase::Down);

        self.cycle_phase = CyclePhase::Cruise
    }

    fn start_probe_bw_up(
        &mut self,
        round_counter: &mut round::Counter,
        delivered_bytes: u64,
        cwnd: u32,
        max_data_size: u16,
        now: Timestamp,
    ) {
        debug_assert_eq!(self.cycle_phase, CyclePhase::Refill);

        self.bw_probe_samples = true;
        self.ack_phase = AckPhase::ProbeStarting;
        round_counter.set_round_end(delivered_bytes);
        self.cycle_stamp = Some(now);
        self.cycle_phase = CyclePhase::Up;
        self.raise_inflight_hi_slope(cwnd, max_data_size);
    }

    fn start_probe_bw_refill(
        &mut self,
        data_volume_model: &mut data_volume::Model,
        data_rate_model: &mut data_rate::Model,
        round_counter: &mut round::Counter,
        delivered_bytes: u64,
    ) {
        debug_assert!(
            self.cycle_phase == CyclePhase::Down || self.cycle_phase == CyclePhase::Cruise
        );

        data_volume_model.reset_lower_bound();
        data_rate_model.reset_lower_bound();
        self.bw_probe_up_rounds = 0;
        self.bw_probe_up_acks = 0;
        self.ack_phase = AckPhase::Refilling;
        round_counter.set_round_end(delivered_bytes);
        self.cycle_phase = CyclePhase::Refill;
    }

    fn start_probe_bw_down(
        &mut self,
        congestion_state: &mut congestion::State,
        round_counter: &mut round::Counter,
        delivered_bytes: u64,
        now: Timestamp,
    ) {
        congestion_state.reset();
        self.bw_probe_up_cnt = u32::MAX;
        // TODO: BBRPickProbeWait
        self.cycle_stamp = Some(now);
        self.ack_phase = AckPhase::ProbeStopping;
        round_counter.set_round_end(delivered_bytes);
        self.cycle_phase = CyclePhase::Down;
    }

    fn check_time_to_cruise(
        &self,
        bytes_in_flight: u32,
        inflight_with_headroom: u32,
        bdp: u32,
    ) -> bool {
        debug_assert_eq!(self.cycle_phase, CyclePhase::Down);

        if bytes_in_flight > inflight_with_headroom {
            return false; // not enough headroom
        }
        if bytes_in_flight <= bdp {
            return true; // inflight <= estimated BDP
        }
        false
    }
}

/// Methods related to the ProbeBW state
impl BbrCongestionController {
    pub fn update_probe_bw_cycle_phase(&mut self, now: Timestamp) {
        let target_inflight = self.target_inflight();

        // TODO: debug_assert(self.state == Probe_Bw, "only handling ProveBW states here")

        match self.probe_bw_state.cycle_phase {
            CyclePhase::Down | CyclePhase::Cruise => {
                if self.probe_bw_state.check_time_to_probe_bw(
                    target_inflight,
                    self.max_datagram_size,
                    now,
                ) {
                    self.probe_bw_state.start_probe_bw_refill(
                        &mut self.data_volume_model,
                        &mut self.data_rate_model,
                        &mut self.round_counter,
                        self.bw_estimator.delivered_bytes(),
                    );
                } else if self.probe_bw_state.cycle_phase == CyclePhase::Down
                    && self.probe_bw_state.check_time_to_cruise(
                        self.bytes_in_flight(),
                        self.inflight_with_headroom(),
                        self.inflight(self.data_rate_model.max_bw(), Ratio::one()),
                    )
                {
                    self.probe_bw_state.start_probe_bw_cruise();
                }
            }
            CyclePhase::Refill => {
                // After one round of Refill, start Up
                if self.round_counter.round_start() {
                    self.probe_bw_state.start_probe_bw_up(
                        &mut self.round_counter,
                        self.bw_estimator.delivered_bytes(),
                        self.cwnd,
                        self.max_datagram_size,
                        now,
                    );
                }
            }
            CyclePhase::Up => {
                let min_rtt = self
                    .data_volume_model
                    .min_rtt()
                    .expect("at least one RTT has passed");

                if self.probe_bw_state.has_elapsed_in_phase(min_rtt, now)
                    && self.bytes_in_flight()
                        > self.inflight(
                            self.data_rate_model.max_bw(),
                            self.probe_bw_state.cycle_phase.pacing_gain(),
                        )
                {
                    self.probe_bw_state.start_probe_bw_down(
                        &mut self.congestion_state,
                        &mut self.round_counter,
                        self.bw_estimator.delivered_bytes(),
                        now,
                    );
                }
            }
        }
    }

    pub fn adapt_upper_bounds(
        &mut self,
        rate_sample: RateSample,
        bytes_acknowledged: usize,
        now: Timestamp,
    ) {
        if !self.full_pipe_estimator.filled_pipe() {
            return; // only handling steady-state behavior here
        }

        // TODO: let is_probe_bw = self.state == ProbeBw
        let is_probe_bw = true;

        if self.probe_bw_state.ack_phase == AckPhase::ProbeStarting
            && self.round_counter.round_start()
        {
            // starting to get bw probing samples
            self.probe_bw_state.ack_phase = AckPhase::ProbeFeedback;
        }
        if self.probe_bw_state.ack_phase == AckPhase::ProbeStopping
            && self.round_counter.round_start()
        {
            self.probe_bw_state.bw_probe_samples = false;
            self.probe_bw_state.ack_phase = AckPhase::Init;

            if is_probe_bw && !rate_sample.is_app_limited {
                self.data_rate_model.advance_max_bw_filter();
            }
        }
        if !self.check_inflight_too_high(rate_sample, now) {
            if self.data_volume_model.inflight_hi() == u64::MAX
                || self.data_rate_model.bw_hi() == Bandwidth::MAX
            {
                // No upper bounds to raise
                return;
            }
            self.data_volume_model
                .update_upper_bound(rate_sample.bytes_in_flight as u64);
            self.data_rate_model
                .update_upper_bound(rate_sample.delivery_rate());

            if self.probe_bw_state.cycle_phase == CyclePhase::Up
                && self.is_congestion_limited()
                && self.cwnd as u64 >= self.data_volume_model.inflight_hi()
            {
                self.probe_bw_state.probe_inflight_hi_upward(
                    bytes_acknowledged,
                    &mut self.data_volume_model,
                    self.cwnd,
                    self.max_datagram_size,
                    self.round_counter.round_start(),
                );
            }
        }
    }

    fn check_inflight_too_high(&mut self, rate_sample: RateSample, now: Timestamp) -> bool {
        let inflight_too_high = BbrCongestionController::is_inflight_too_high(
            rate_sample.lost_bytes,
            rate_sample.bytes_in_flight,
        );

        if inflight_too_high && self.probe_bw_state.bw_probe_samples {
            self.on_inflight_too_high(
                rate_sample.is_app_limited,
                rate_sample.bytes_in_flight,
                self.target_inflight(),
                now,
            );
        }

        inflight_too_high
    }

    pub fn on_inflight_too_high(
        &mut self,
        is_app_limited: bool,
        bytes_in_flight: u32,
        target_inflight: u32,
        now: Timestamp,
    ) {
        self.probe_bw_state.bw_probe_samples = false; // only react once per bw probe
        if !is_app_limited {
            // TODO: fix update_upper_bound
            self.data_volume_model.update_upper_bound(
                (bytes_in_flight as u64).max((bbr::BETA * target_inflight as u64).to_integer()),
            )
        }

        // TODO: Check self.state == State::ProbeBw
        if self.probe_bw_state.cycle_phase == CyclePhase::Up {
            self.probe_bw_state.start_probe_bw_down(
                &mut self.congestion_state,
                &mut self.round_counter,
                self.bw_estimator.delivered_bytes(),
                now,
            );
        }
    }
}
