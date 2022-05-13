// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Saturating},
    recovery::{
        bandwidth::RateSample,
        bbr,
        bbr::{congestion, data_rate, data_volume, round, BbrCongestionController},
        CongestionController,
    },
    time::Timestamp,
};
use core::{convert::TryInto, time::Duration};
use num_rational::Ratio;
use num_traits::One;

const MAX_BW_PROBE_UP_ROUNDS: u8 = 30;

/// Max number of packet-timed rounds to wait before probing for bandwidth
const MAX_BW_PROBE_ROUNDS: u8 = 63;

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
        //# pacing_gain of 1.0, sending at 100% of BBR.bw.
        const CRUISE_REFILL_PACING_GAIN: Ratio<u64> = Ratio::new_raw(1, 1);

        match self {
            CyclePhase::Down => DOWN_PACING_GAIN,
            CyclePhase::Cruise | CyclePhase::Refill => CRUISE_REFILL_PACING_GAIN,
            CyclePhase::Up => UP_PACING_GAIN,
        }
    }
}

/// How the incoming ACK stream relates to our bandwidth probing
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    rounds_since_bw_probe: Counter<u8, Saturating>,
    /// Bytes delivered per inflight_hi increment
    bw_probe_up_cnt: u32,
    /// Bytes ACKed since inflight_hi increment
    bw_probe_up_acks: u32,
    /// cwnd-limited rounds in PROBE_UP
    bw_probe_up_rounds: u8,
    /// True if the rate samples reflect bandwidth probing
    bw_probe_samples: bool,
    /// Time of this cycle phase start
    cycle_stamp: Option<Timestamp>,
}

impl State {
    /// Constructs new `probe_bw::State`
    #[allow(dead_code)] // TODO: Remove when used
    pub fn new() -> Self {
        Self {
            cycle_phase: CyclePhase::Down,
            ack_phase: AckPhase::Init,
            bw_probe_wait: Duration::ZERO,
            rounds_since_bw_probe: Counter::default(),
            bw_probe_up_cnt: u32::MAX,
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

    pub fn on_round_start(&mut self) {
        self.rounds_since_bw_probe += 1;
    }

    /// Returns true if enough time has passed to transition the cycle phase
    pub fn check_time_to_probe_bw(
        &self,
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

    /// Probe for possible increases in bandwidth
    fn probe_inflight_hi_upward(
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
        let growth_this_round = 1 << self.bw_probe_up_rounds;
        // The MAX_BW_PROBE_UP_ROUNDS (30) number below means `growth_this_round` is capped at 1G
        // and the lower bound of `bw_probe_up_cnt` is (practically) 1 mss, at this speed inflight_hi
        // grows by approximately 1 packet per packet acked.
        self.bw_probe_up_rounds = (self.bw_probe_up_rounds + 1).min(MAX_BW_PROBE_UP_ROUNDS);
        self.bw_probe_up_cnt = (cwnd / growth_this_round).max(max_data_size as u32);
    }

    /// True if the given `interval` duration has elapsed since the current cycle phase began
    fn has_elapsed_in_phase(&self, interval: Duration, now: Timestamp) -> bool {
        self.cycle_stamp
            .map_or(false, |cycle_stamp| now > cycle_stamp + interval)
    }

    /// Bandwidth probing can cause loss. To help coexistence with loss-based
    /// congestion control we spread out our probing in a Reno-conscious way. Due to
    /// the shape of the Reno sawtooth, the time required between loss epochs for an
    /// idealized Reno flow is a number of round trips that is the BDP of that
    /// flow. We count packet-timed round trips directly, since measured RTT can
    /// vary widely, and Reno is driven by packet-timed round trips.
    fn is_reno_coexistence_probe_time(&self, target_inflight: u32, max_data_size: u16) -> bool {
        let reno_rounds = target_inflight / max_data_size as u32;
        let rounds = reno_rounds
            .try_into()
            .unwrap_or(u8::MAX)
            .min(MAX_BW_PROBE_ROUNDS);
        self.rounds_since_bw_probe >= rounds
    }

    /// Start the `Cruise` cycle phase
    fn start_cruise(&mut self) {
        debug_assert_eq!(self.cycle_phase, CyclePhase::Down);

        self.cycle_phase = CyclePhase::Cruise
    }

    /// Start the `Up` cycle phase
    fn start_up(
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

    /// Start the `Refill` cycle phase
    fn start_refill(
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

    /// Start the `Down` cycle phase
    fn start_down(
        &mut self,
        congestion_state: &mut congestion::State,
        round_counter: &mut round::Counter,
        delivered_bytes: u64,
        now: Timestamp,
    ) {
        congestion_state.reset();
        self.bw_probe_up_cnt = u32::MAX;
        self.rounds_since_bw_probe = Counter::default(); // TODO: BBRPickProbeWait
        self.bw_probe_wait = Duration::from_secs(2); // TODO: BBRPickProbeWait
        self.cycle_stamp = Some(now);
        self.ack_phase = AckPhase::ProbeStopping;
        round_counter.set_round_end(delivered_bytes);
        self.cycle_phase = CyclePhase::Down;
    }
}

/// Methods related to the ProbeBW state
impl BbrCongestionController {
    /// Transition the current Probe BW cycle phase if necessary
    pub fn update_probe_bw_cycle_phase(&mut self, now: Timestamp) {
        debug_assert!(
            self.full_pipe_estimator.filled_pipe(),
            "only handling steady-state behavior here"
        );

        let target_inflight = self.target_inflight();

        // TODO: debug_assert(self.state == Probe_Bw, "only handling ProveBW states here")

        match self.probe_bw_state.cycle_phase {
            CyclePhase::Down | CyclePhase::Cruise => {
                if self.probe_bw_state.check_time_to_probe_bw(
                    target_inflight,
                    self.max_datagram_size,
                    now,
                ) {
                    self.probe_bw_state.start_refill(
                        &mut self.data_volume_model,
                        &mut self.data_rate_model,
                        &mut self.round_counter,
                        self.bw_estimator.delivered_bytes(),
                    );
                } else if self.probe_bw_state.cycle_phase == CyclePhase::Down
                    && self.check_time_to_cruise()
                {
                    self.probe_bw_state.start_cruise();
                }
            }
            CyclePhase::Refill => {
                // After one round of Refill, start Up
                if self.round_counter.round_start() {
                    self.probe_bw_state.start_up(
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
                    self.probe_bw_state.start_down(
                        &mut self.congestion_state,
                        &mut self.round_counter,
                        self.bw_estimator.delivered_bytes(),
                        now,
                    );
                }
            }
        }
    }

    /// Adapt the upper bounds lower or higher depending on the loss rate
    pub fn adapt_upper_bounds(
        &mut self,
        rate_sample: RateSample,
        bytes_acknowledged: usize,
        now: Timestamp,
    ) {
        debug_assert!(
            self.full_pipe_estimator.filled_pipe(),
            "only handling steady-state behavior here"
        );

        // Update AckPhase once per round
        if self.round_counter.round_start() {
            self.update_ack_phase(rate_sample);
        }

        if Self::is_inflight_too_high(rate_sample.lost_bytes, rate_sample.bytes_in_flight) {
            if self.probe_bw_state.bw_probe_samples {
                // Inflight is too high and the sample is from bandwidth probing: lower inflight downward
                self.on_inflight_too_high(
                    rate_sample.is_app_limited,
                    rate_sample.bytes_in_flight,
                    self.target_inflight(),
                    now,
                );
            }
        } else {
            // Loss rate is safe. Adjust upper bounds upward

            if self.data_volume_model.inflight_hi() == u64::MAX {
                // no upper bounds to raise
                return;
            }

            // draft-cardwell-iccrg-bbr-congestion-control-02 also considers raising bw_hi at this
            // point, but since the draft never lowers bw_hi from its initial value of Infinity, this
            // doesn't have any effect. bw_hi in the current Linux V2Alpha BBR2 branch corresponds
            // to max_hi from the draft, there is no equivalent to the bw_hi in the draft
            // TODO: Update this logic based on subsequent draft updates or consider lowering
            //       bw_hi in `on_inflight_too_high`
            if rate_sample.bytes_in_flight as u64 > self.data_volume_model.inflight_hi() {
                self.data_volume_model
                    .update_upper_bound(rate_sample.bytes_in_flight as u64);
            }

            if self.probe_bw_state.cycle_phase == CyclePhase::Up
                && self.is_congestion_limited()
                && self.cwnd as u64 >= self.data_volume_model.inflight_hi()
            {
                // inflight_hi is being fully utilized, so probe if we can increase it
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

    /// Update AckPhase and advance the Max BW filter if necessary
    fn update_ack_phase(&mut self, rate_sample: RateSample) {
        // TODO: let is_probe_bw = self.state == ProbeBw
        let is_probe_bw = true;

        match self.probe_bw_state.ack_phase {
            AckPhase::ProbeStarting => {
                // starting to get bw probing samples
                self.probe_bw_state.ack_phase = AckPhase::ProbeFeedback;
            }
            AckPhase::ProbeStopping => {
                self.probe_bw_state.bw_probe_samples = false;
                self.probe_bw_state.ack_phase = AckPhase::Init;

                if is_probe_bw && !rate_sample.is_app_limited {
                    self.data_rate_model.advance_max_bw_filter();
                }
            }
            _ => {}
        }
    }

    /// Called when loss indicates the current inflight amount is too high
    pub fn on_inflight_too_high(
        &mut self,
        is_app_limited: bool,
        bytes_in_flight: u32,
        target_inflight: u32,
        now: Timestamp,
    ) {
        self.probe_bw_state.bw_probe_samples = false; // only react once per bw probe
        if !is_app_limited {
            self.data_volume_model.update_upper_bound(
                (bytes_in_flight as u64).max((bbr::BETA * target_inflight as u64).to_integer()),
            )
        }

        // TODO: Check self.state == State::ProbeBw
        if self.probe_bw_state.cycle_phase == CyclePhase::Up {
            self.probe_bw_state.start_down(
                &mut self.congestion_state,
                &mut self.round_counter,
                self.bw_estimator.delivered_bytes(),
                now,
            );
        }
    }

    /// Returns true if it is time to transition from `Down` to `Cruise`
    fn check_time_to_cruise(&self) -> bool {
        debug_assert_eq!(self.probe_bw_state.cycle_phase, CyclePhase::Down);

        if self.bytes_in_flight > self.inflight_with_headroom() {
            return false; // not enough headroom
        }
        if self.bytes_in_flight <= self.inflight(self.data_rate_model.max_bw(), Ratio::one()) {
            return true; // inflight <= estimated BDP
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::{Clock, NoopClock};

    #[test]
    fn pacing_gain() {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.1
        //= type=test
        //# In the ProbeBW_DOWN phase of the cycle, a BBR flow pursues the deceleration tactic,
        //# to try to send slower than the network is delivering data, to reduce the amount of data
        //# in flight, with all of the standard motivations for the deceleration tactic (discussed
        //# in "State Machine Tactics", above). It does this by switching to a BBR.pacing_gain of
        //# 0.9, sending at 90% of BBR.bw.
        assert_eq!(Ratio::new_raw(9, 10), CyclePhase::Down.pacing_gain());

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.4
        //= type=test
        //# After ProbeBW_REFILL refills the pipe, ProbeBW_UP probes for possible increases in
        //# available bandwidth by using a BBR.pacing_gain of 1.25, sending faster than the current
        //# estimated available bandwidth.
        assert_eq!(Ratio::new_raw(5, 4), CyclePhase::Up.pacing_gain());

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.3
        //= type=test
        //# During ProbeBW_REFILL BBR uses a BBR.pacing_gain of 1.0, to send at a rate that
        //# matches the current estimated available bandwidth
        assert_eq!(Ratio::new_raw(1, 1), CyclePhase::Refill.pacing_gain());

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.2
        //= type=test
        //# In the ProbeBW_CRUISE phase of the cycle, a BBR flow pursues the "cruising" tactic
        //# (discussed in "State Machine Tactics", above), attempting to send at the same rate
        //# the network is delivering data. It tries to match the sending rate to the flow's
        //# current available bandwidth, to try to achieve high utilization of the available
        //# bandwidth without increasing queue pressure. It does this by switching to a
        //# pacing_gain of 1.0, sending at 100% of BBR.bw.
        assert_eq!(Ratio::new_raw(1, 1), CyclePhase::Cruise.pacing_gain());
    }

    #[test]
    fn new_probe_bw_state() {
        let state = State::new();

        assert_eq!(CyclePhase::Down, state.cycle_phase);
        assert_eq!(AckPhase::Init, state.ack_phase);
        assert_eq!(Duration::ZERO, state.bw_probe_wait);
        assert_eq!(Counter::new(0), state.rounds_since_bw_probe);
        assert_eq!(0, state.bw_probe_up_acks);
        assert_eq!(0, state.bw_probe_up_rounds);
        assert!(!state.bw_probe_samples);
        assert_eq!(None, state.cycle_stamp);
    }

    #[test]
    fn check_time_to_probe_bw() {
        let mut state = State::new();
        let now = NoopClock.get_time();

        // cycle_stamp hasn't been set yet
        assert!(!state.check_time_to_probe_bw(12000, 1200, now));

        state.cycle_stamp = Some(now);
        let bw_probe_wait = Duration::from_millis(500);
        state.bw_probe_wait = bw_probe_wait;
        // not ready to probe yet
        assert!(!state.check_time_to_probe_bw(12000, 1200, now + bw_probe_wait));
        // now we're ready to probe
        assert!(state.check_time_to_probe_bw(
            100,
            1200,
            now + bw_probe_wait + Duration::from_millis(1)
        ));

        state.rounds_since_bw_probe = Counter::new(10);
        // 13200 / 1200 = 11 reno rounds, not in reno coexistence probe time
        assert!(!state.check_time_to_probe_bw(13200, 1200, now));
        // 12000 / 1200 = 10 reno rounds, now we are in reno coexistence probe time
        assert!(state.check_time_to_probe_bw(12000, 1200, now));

        // At high BDPs, we probe when MAX_BW_PROBE_ROUNDS is reached
        state.rounds_since_bw_probe = Counter::new(MAX_BW_PROBE_ROUNDS - 1);
        assert!(!state.check_time_to_probe_bw(u32::MAX, 1200, now));
        state.rounds_since_bw_probe = Counter::new(MAX_BW_PROBE_ROUNDS);
        assert!(state.check_time_to_probe_bw(u32::MAX, 1200, now));
    }
}
