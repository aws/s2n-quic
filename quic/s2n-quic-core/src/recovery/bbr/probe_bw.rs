// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Saturating},
    random,
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
    cycle_start_timestamp: Option<Timestamp>,
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
            cycle_start_timestamp: None,
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
    pub fn is_time_to_probe_bw(
        &self,
        target_inflight: u32,
        max_data_size: u16,
        now: Timestamp,
    ) -> bool {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.5.3
        //# BBRCheckTimeToProbeBW()

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
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRProbeInflightHiUpward()

        self.bw_probe_up_acks += bytes_acknowledged as u32;
        // Increase inflight_hi by the number of bw_probe_up_cnt bytes within bw_probe_up_acks
        if self.bw_probe_up_acks >= self.bw_probe_up_cnt {
            let delta = self.bw_probe_up_acks / self.bw_probe_up_cnt;
            self.bw_probe_up_acks -= delta * self.bw_probe_up_cnt;
            let inflight_hi =
                data_volume_model.inflight_hi() + (delta as u64 * max_data_size as u64);
            data_volume_model.update_upper_bound(inflight_hi);
        }
        if round_start {
            self.raise_inflight_hi_slope(cwnd, max_data_size);
        }
    }

    /// Raise inflight_hi slope if appropriate
    fn raise_inflight_hi_slope(&mut self, cwnd: u32, max_data_size: u16) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRRaiseInflightHiSlope()

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
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRHasElapsedInPhase(interval)

        self.cycle_start_timestamp
            .map_or(false, |cycle_stamp| now > cycle_stamp + interval)
    }

    /// Bandwidth probing can cause loss. To help coexistence with loss-based
    /// congestion control we spread out our probing in a Reno-conscious way. Due to
    /// the shape of the Reno sawtooth, the time required between loss epochs for an
    /// idealized Reno flow is a number of round trips that is the BDP of that
    /// flow. We count packet-timed round trips directly, since measured RTT can
    /// vary widely, and Reno is driven by packet-timed round trips.
    fn is_reno_coexistence_probe_time(&self, target_inflight: u32, max_data_size: u16) -> bool {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.5.3
        //# BBRIsRenoCoexistenceProbeTime()

        let reno_rounds = target_inflight / max_data_size as u32;
        let rounds = reno_rounds
            .try_into()
            .unwrap_or(u8::MAX)
            .min(MAX_BW_PROBE_ROUNDS);
        self.rounds_since_bw_probe >= rounds
    }

    /// Start the `Cruise` cycle phase
    fn start_cruise(&mut self) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRStartProbeBW_CRUISE()

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
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRStartProbeBW_UP()

        debug_assert_eq!(self.cycle_phase, CyclePhase::Refill);

        self.bw_probe_samples = true;
        self.ack_phase = AckPhase::ProbeStarting;
        round_counter.set_round_end(delivered_bytes);
        self.cycle_start_timestamp = Some(now);
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
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRStartProbeBW_REFILL()

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
    fn start_down<Rnd: random::Generator>(
        &mut self,
        congestion_state: &mut congestion::State,
        round_counter: &mut round::Counter,
        delivered_bytes: u64,
        random_generator: &mut Rnd,
        now: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRStartProbeBW_DOWN()

        congestion_state.reset();
        self.bw_probe_up_cnt = u32::MAX;
        self.pick_probe_wait(random_generator);
        self.cycle_start_timestamp = Some(now);
        self.ack_phase = AckPhase::ProbeStopping;
        round_counter.set_round_end(delivered_bytes);
        self.cycle_phase = CyclePhase::Down;
    }

    fn pick_probe_wait<Rnd: random::Generator>(&mut self, _random_generator: &mut Rnd) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.5.3
        //# BBRPickProbeWait()

        // TODO:
        //     /* Decide random round-trip bound for wait: */
        //     BBR.rounds_since_bw_probe = random_int_between(0, 1); /* 0 or 1 */
        //     /* Decide the random wall clock bound for wait: */
        //     BBR.bw_probe_wait = 2sec + random_float_between(0.0, 1.0) /* 0..1 sec */
        self.rounds_since_bw_probe = Counter::default();
        self.bw_probe_wait = Duration::from_secs(2);
    }
}

/// Methods related to the ProbeBW state
impl BbrCongestionController {
    /// Transition the current Probe BW cycle phase if necessary
    pub fn update_probe_bw_cycle_phase<Rnd: random::Generator>(
        &mut self,
        random_generator: &mut Rnd,
        now: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRUpdateProbeBWCyclePhase()

        debug_assert!(
            self.full_pipe_estimator.filled_pipe(),
            "only handling steady-state behavior here"
        );

        let target_inflight = self.target_inflight();

        // TODO: debug_assert(self.state == Probe_Bw, "only handling ProveBW states here")

        match self.probe_bw_state.cycle_phase {
            CyclePhase::Down | CyclePhase::Cruise => {
                if self.probe_bw_state.is_time_to_probe_bw(
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
                    && self.is_time_to_cruise()
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
                        random_generator,
                        now,
                    );
                }
            }
        }
    }

    /// Adapt the upper bounds lower or higher depending on the loss rate
    pub fn adapt_upper_bounds<Rnd: random::Generator>(
        &mut self,
        rate_sample: RateSample,
        bytes_acknowledged: usize,
        random_generator: &mut Rnd,
        now: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRAdaptUpperBounds()

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
                    random_generator,
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
    pub fn on_inflight_too_high<Rnd: random::Generator>(
        &mut self,
        is_app_limited: bool,
        bytes_in_flight: u32,
        target_inflight: u32,
        random_generator: &mut Rnd,
        now: Timestamp,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.2
        //# BBRHandleInflightTooHigh()

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
                random_generator,
                now,
            );
        }
    }

    /// Returns true if it is time to transition from `Down` to `Cruise`
    fn is_time_to_cruise(&self) -> bool {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRCheckTimeToCruise())

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
    use crate::{
        recovery::bandwidth::{Bandwidth, PacketInfo},
        time::{Clock, NoopClock},
    };

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
        assert_eq!(None, state.cycle_start_timestamp);
    }

    #[test]
    fn is_time_to_probe_bw() {
        let mut state = State::new();
        let now = NoopClock.get_time();

        // cycle_stamp hasn't been set yet
        assert!(!state.is_time_to_probe_bw(12000, 1200, now));

        state.cycle_start_timestamp = Some(now);
        let bw_probe_wait = Duration::from_millis(500);
        state.bw_probe_wait = bw_probe_wait;
        // not ready to probe yet
        assert!(!state.is_time_to_probe_bw(12000, 1200, now + bw_probe_wait));
        // now we're ready to probe
        assert!(state.is_time_to_probe_bw(
            100,
            1200,
            now + bw_probe_wait + Duration::from_millis(1)
        ));

        state.rounds_since_bw_probe = Counter::new(10);
        // 13200 / 1200 = 11 reno rounds, not in reno coexistence probe time
        assert!(!state.is_time_to_probe_bw(13200, 1200, now));
        // 12000 / 1200 = 10 reno rounds, now we are in reno coexistence probe time
        assert!(state.is_time_to_probe_bw(12000, 1200, now));

        // At high BDPs, we probe when MAX_BW_PROBE_ROUNDS is reached
        state.rounds_since_bw_probe = Counter::new(MAX_BW_PROBE_ROUNDS - 1);
        assert!(!state.is_time_to_probe_bw(u32::MAX, 1200, now));
        state.rounds_since_bw_probe = Counter::new(MAX_BW_PROBE_ROUNDS);
        assert!(state.is_time_to_probe_bw(u32::MAX, 1200, now));
    }

    #[test]
    fn probe_inflight_hi_upward() {
        let mut state = State::new();
        let now = NoopClock.get_time();

        let bytes_acknowledged = 2400;
        let mut data_volume_model = data_volume::Model::new(now);
        let cwnd = 12000;
        let max_data_size = 1200;
        let round_start = true;

        state.bw_probe_up_rounds = 3;
        data_volume_model.update_upper_bound(12000);

        state.probe_inflight_hi_upward(
            bytes_acknowledged,
            &mut data_volume_model,
            cwnd,
            max_data_size,
            round_start,
        );

        assert_eq!(bytes_acknowledged as u32, state.bw_probe_up_acks);
        assert_eq!(12000, data_volume_model.inflight_hi());
        assert_eq!(4, state.bw_probe_up_rounds);
        // bw_probe_up_cnt = cwnd (12000) / 1 << 3
        assert_eq!(cwnd / 8, state.bw_probe_up_cnt);

        let new_bytes_acknowledged = (cwnd / 8) as usize;
        state.probe_inflight_hi_upward(
            new_bytes_acknowledged,
            &mut data_volume_model,
            cwnd,
            max_data_size,
            false,
        );

        // bw_probe_up_acks = bytes_acknowledged + new_bytes_acknowledged = 3900
        // delta = 3900 / bw_probe_up_cnt  = 3900 / 1500 = 2
        // bw_probe_up_acks = bw_probe_up_acks - delta * bw_probe_up_cnt = 3900 - 2 * 1500 = 900
        assert_eq!(900, state.bw_probe_up_acks);
        // inflight_hi = inflight_hi + delta * max_data_size = 12000 + 2 * 1200 = 14400
        assert_eq!(14400, data_volume_model.inflight_hi());
        // bw_probe_up_rounds stays the same, since round_start was false
        assert_eq!(4, state.bw_probe_up_rounds);
        // bw_probe_up_cnt stays the same, since round_start was false
        assert_eq!(cwnd / 8, state.bw_probe_up_cnt);
    }

    #[test]
    fn start_cruise() {
        let mut state = State::new();

        state.start_cruise();

        assert_eq!(CyclePhase::Cruise, state.cycle_phase());
    }

    #[test]
    fn start_up() {
        let mut state = State::new();
        let mut round_counter = round::Counter::default();
        let delivered_bytes = 100;
        let cwnd = 12000;
        let max_data_size = 1200;
        let now = NoopClock.get_time();

        state.cycle_phase = CyclePhase::Refill;

        state.start_up(
            &mut round_counter,
            delivered_bytes,
            cwnd,
            max_data_size,
            now,
        );

        assert_eq!(CyclePhase::Up, state.cycle_phase());
        assert!(state.bw_probe_samples);
        assert_eq!(AckPhase::ProbeStarting, state.ack_phase);
        assert_eq!(Some(now), state.cycle_start_timestamp);

        // raise_inflight_hi_slope is called
        assert_eq!(1, state.bw_probe_up_rounds);
        assert_eq!(cwnd, state.bw_probe_up_cnt);

        // verify the end of round is set to delivered_bytes
        // verify the end of round is set to delivered_bytes
        assert_round_end(round_counter, delivered_bytes);
    }

    #[test]
    fn start_refill() {
        let mut state = State::new();
        let mut round_counter = round::Counter::default();
        let delivered_bytes = 100;
        let now = NoopClock.get_time();
        let mut data_volume_model = data_volume::Model::new(now);
        let mut data_rate_model = data_rate::Model::new();
        data_volume_model.update_lower_bound(12000, 12000);
        data_rate_model.update_lower_bound(Bandwidth::ZERO);

        state.cycle_phase = CyclePhase::Cruise;

        state.start_refill(
            &mut data_volume_model,
            &mut data_rate_model,
            &mut round_counter,
            delivered_bytes,
        );

        assert_eq!(CyclePhase::Refill, state.cycle_phase());
        // Lower bounds are reset
        assert_eq!(u64::MAX, data_volume_model.inflight_lo());
        assert_eq!(Bandwidth::MAX, data_rate_model.bw_lo());

        assert_eq!(0, state.bw_probe_up_rounds);
        assert_eq!(0, state.bw_probe_up_acks);
        assert_eq!(AckPhase::Refilling, state.ack_phase);

        // verify the end of round is set to delivered_bytes
        assert_round_end(round_counter, delivered_bytes);
    }

    #[test]
    fn start_down() {
        let mut state = State::new();
        let mut congestion_state = congestion::testing::test_state();
        let mut round_counter = round::Counter::default();
        let delivered_bytes = 100;
        let now = NoopClock.get_time();
        let random = &mut random::testing::Generator::default();

        state.cycle_phase = CyclePhase::Up;

        state.start_down(
            &mut congestion_state,
            &mut round_counter,
            delivered_bytes,
            random,
            now,
        );

        assert_eq!(CyclePhase::Down, state.cycle_phase());
        assert_eq!(u32::MAX, state.bw_probe_up_cnt);
        assert!(state.rounds_since_bw_probe >= 0 && state.rounds_since_bw_probe <= 1);
        assert!(
            state.bw_probe_wait >= Duration::from_secs(2)
                && state.bw_probe_wait <= Duration::from_secs(3)
        );
        assert_eq!(Some(now), state.cycle_start_timestamp);
        assert_eq!(AckPhase::ProbeStopping, state.ack_phase);

        // verify congestion state is reset
        congestion::testing::assert_reset(congestion_state);

        // verify the end of round is set to delivered_bytes
        assert_round_end(round_counter, delivered_bytes);
    }

    fn assert_round_end(mut round_counter: round::Counter, expected_end: u64) {
        let now = NoopClock.get_time();
        // verify the end of round is set to delivered_bytes
        let mut packet_info = PacketInfo {
            delivered_bytes: expected_end - 1,
            delivered_time: now,
            lost_bytes: 0,
            first_sent_time: now,
            bytes_in_flight: 0,
            is_app_limited: false,
        };
        round_counter.on_ack(packet_info, expected_end);
        assert!(!round_counter.round_start());

        packet_info.delivered_bytes = expected_end;
        round_counter.on_ack(packet_info, expected_end);
        assert!(round_counter.round_start());
    }
}
