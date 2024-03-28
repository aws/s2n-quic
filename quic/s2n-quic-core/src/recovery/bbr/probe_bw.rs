// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::{Counter, Saturating},
    event,
    event::IntoEvent,
    random,
    recovery::{
        bandwidth::RateSample,
        bbr,
        bbr::{congestion, data_rate, data_volume, round, BbrCongestionController},
        congestion_controller::Publisher,
    },
    time::Timestamp,
};
use core::time::Duration;
use num_rational::Ratio;
use num_traits::One;

const MAX_BW_PROBE_UP_ROUNDS: u8 = 30;

/// Max number of packet-timed rounds to wait before probing for bandwidth
const MAX_BW_PROBE_ROUNDS: u8 = 63;

/// The number of discontiguous bursts of loss required before inflight_hi is lowered
/// Value from:
/// https://source.chromium.org/chromium/chromium/src/+/main:net/third_party/quiche/src/quiche/quic/core/quic_protocol_flags_list.h;l=139;bpv=1;bpt=0
pub(super) const PROBE_BW_FULL_LOSS_COUNT: u8 = 2;

/// Cwnd gain used in the Probe BW state
///
/// This value is defined in the table in
/// https://www.ietf.org/archive/id/draft-cardwell-iccrg-bbr-congestion-control-02.html#section-4.6.1
pub(crate) const CWND_GAIN: Ratio<u64> = Ratio::new_raw(2, 1);

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

    /// Transition to the given `new_phase`
    fn transition_to<Pub: Publisher>(&mut self, new_phase: CyclePhase, publisher: &mut Pub) {
        if cfg!(debug_assertions) {
            match new_phase {
                CyclePhase::Down => assert_eq!(*self, CyclePhase::Up),
                CyclePhase::Cruise => assert_eq!(*self, CyclePhase::Down),
                CyclePhase::Refill => {
                    assert!(*self == CyclePhase::Down || *self == CyclePhase::Cruise)
                }
                CyclePhase::Up => assert_eq!(*self, CyclePhase::Refill),
            }
        }

        publisher.on_bbr_state_changed(new_phase.into_event());

        *self = new_phase;
    }
}

impl IntoEvent<event::builder::BbrState> for CyclePhase {
    #[inline]
    fn into_event(self) -> event::builder::BbrState {
        use event::builder::BbrState;
        match self {
            CyclePhase::Down => BbrState::ProbeBwDown,
            CyclePhase::Cruise => BbrState::ProbeBwCruise,
            CyclePhase::Refill => BbrState::ProbeBwRefill,
            CyclePhase::Up => BbrState::ProbeBwUp,
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

impl AckPhase {
    /// Transition to the given `new_phase`
    fn transition_to(&mut self, new_phase: AckPhase) {
        if cfg!(debug_assertions) {
            match new_phase {
                AckPhase::ProbeStopping => {
                    assert!(
                        *self == AckPhase::Init
                            || *self == AckPhase::ProbeStarting
                            || *self == AckPhase::ProbeFeedback
                    )
                }
                AckPhase::Refilling => {
                    assert!(*self == AckPhase::Init || *self == AckPhase::ProbeStopping)
                }
                AckPhase::ProbeStarting => assert_eq!(*self, AckPhase::Refilling),
                AckPhase::ProbeFeedback => assert_eq!(*self, AckPhase::ProbeStarting),
                AckPhase::Init => assert_eq!(*self, AckPhase::ProbeStopping),
            }
        }

        *self = new_phase;
    }
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
    /// Time of this cycle phase start
    cycle_start_timestamp: Option<Timestamp>,
}

impl State {
    /// Constructs new `probe_bw::State`
    fn new() -> Self {
        Self {
            cycle_phase: CyclePhase::Up,
            ack_phase: AckPhase::Init,
            bw_probe_wait: Duration::ZERO,
            rounds_since_bw_probe: Counter::default(),
            bw_probe_up_cnt: u32::MAX,
            bw_probe_up_acks: 0,
            bw_probe_up_rounds: 0,
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
        //#   if (BBRHasElapsedInPhase(BBR.bw_probe_wait) ||
        //#       BBRIsRenoCoexistenceProbeTime())
        //#     BBRStartProbeBW_REFILL()
        //#     return true
        //#   return false

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
        //#   if (!is_cwnd_limited or cwnd < BBR.inflight_hi)
        //#       return  /* not fully using inflight_hi, so don't grow it */
        //#   BBR.bw_probe_up_acks += rs.newly_acked
        //#   if (BBR.bw_probe_up_acks >= BBR.probe_up_cnt)
        //#      delta = BBR.bw_probe_up_acks / BBR.probe_up_cnt
        //#      BBR.bw_probe_up_acks -= delta * BBR.bw_probe_up_cnt
        //#      BBR.inflight_hi += delta
        //#   if (BBR.round_start)
        //#      BBRRaiseInflightHiSlope()

        // is_cwnd_limited and cwnd < BBR.inflight_hi is checked upstream

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
        //# BBRRaiseInflightHiSlope():
        //#   growth_this_round = 1MSS << BBR.bw_probe_up_rounds
        //#   BBR.bw_probe_up_rounds = min(BBR.bw_probe_up_rounds + 1, 30)
        //#   BBR.probe_up_cnt = max(cwnd / growth_this_round, 1)

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
        //#   return Now() > BBR.cycle_stamp + interval

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
        //#   reno_rounds = BBRTargetInflight()
        //#   rounds = min(reno_rounds, 63)
        //#   return BBR.rounds_since_bw_probe >= rounds

        let reno_rounds = target_inflight / max_data_size as u32;
        let rounds = reno_rounds
            .try_into()
            .unwrap_or(u8::MAX)
            .min(MAX_BW_PROBE_ROUNDS);
        self.rounds_since_bw_probe >= rounds
    }

    /// Start the `Cruise` cycle phase
    fn start_cruise<Pub: Publisher>(&mut self, publisher: &mut Pub) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRStartProbeBW_CRUISE()
        //#   BBR.state = ProbeBW_CRUISE

        self.cycle_phase
            .transition_to(CyclePhase::Cruise, publisher);
    }

    /// Start the `Up` cycle phase
    fn start_up<Pub: Publisher>(
        &mut self,
        round_counter: &mut round::Counter,
        delivered_bytes: u64,
        cwnd: u32,
        max_data_size: u16,
        now: Timestamp,
        publisher: &mut Pub,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRStartProbeBW_UP()
        //#   BBR.ack_phase = ACKS_PROBE_STARTING
        //#   BBRStartRound()
        //#   BBR.cycle_stamp = Now() /* start wall clock */
        //#   BBR.state = ProbeBW_UP
        //#   BBRRaiseInflightHiSlope()

        self.ack_phase.transition_to(AckPhase::ProbeStarting);
        round_counter.set_round_end(delivered_bytes);
        self.cycle_start_timestamp = Some(now);
        self.cycle_phase.transition_to(CyclePhase::Up, publisher);
        self.raise_inflight_hi_slope(cwnd, max_data_size);
    }

    /// Start the `Refill` cycle phase
    fn start_refill<Pub: Publisher>(
        &mut self,
        data_volume_model: &mut data_volume::Model,
        data_rate_model: &mut data_rate::Model,
        round_counter: &mut round::Counter,
        delivered_bytes: u64,
        publisher: &mut Pub,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRStartProbeBW_REFILL()
        //# BBRResetLowerBounds()
        //#   BBR.bw_probe_up_rounds = 0
        //#   BBR.bw_probe_up_acks = 0
        //#   BBR.ack_phase = ACKS_REFILLING
        //#   BBRStartRound()
        //#   BBR.state = ProbeBW_REFILL

        data_volume_model.reset_lower_bound();
        data_rate_model.reset_lower_bound();
        self.bw_probe_up_rounds = 0;
        self.bw_probe_up_acks = 0;
        self.ack_phase.transition_to(AckPhase::Refilling);
        round_counter.set_round_end(delivered_bytes);
        self.cycle_phase
            .transition_to(CyclePhase::Refill, publisher);
    }

    /// Start the `Down` cycle phase
    fn start_down<Pub: Publisher>(
        &mut self,
        congestion_state: &mut congestion::State,
        round_counter: &mut round::Counter,
        delivered_bytes: u64,
        random_generator: &mut dyn random::Generator,
        now: Timestamp,
        publisher: &mut Pub,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRStartProbeBW_DOWN()
        //#   BBRResetCongestionSignals()
        //#   BBR.probe_up_cnt = Infinity /* not growing inflight_hi */
        //#   BBRPickProbeWait()
        //#   BBR.cycle_stamp = Now()  /* start wall clock */
        //#   BBR.ack_phase  = ACKS_PROBE_STOPPING
        //#   BBRStartRound()
        //#   BBR.state = ProbeBW_DOWN

        congestion_state.reset();
        self.bw_probe_up_cnt = u32::MAX;
        self.pick_probe_wait(random_generator);
        self.cycle_start_timestamp = Some(now);
        self.ack_phase.transition_to(AckPhase::ProbeStopping);
        round_counter.set_round_end(delivered_bytes);
        self.cycle_phase.transition_to(CyclePhase::Down, publisher);
    }

    /// Randomly determine how long to wait before probing again
    ///
    /// Note: This uses a method for determining a number in a random range that has a very slight
    ///       bias. In practice, this bias should not result in a detectable impact to BBR performance.
    fn pick_probe_wait(&mut self, random_generator: &mut dyn random::Generator) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.5.3
        //# BBRPickProbeWait()
        //#    /* Decide random round-trip bound for wait: */
        //#     BBR.rounds_since_bw_probe =
        //#       random_int_between(0, 1); /* 0 or 1 */
        //#     /* Decide the random wall clock bound for wait: */
        //#     BBR.bw_probe_wait =
        //#       2sec + random_float_between(0.0, 1.0) /* 0..1 sec */
        self.rounds_since_bw_probe
            .set(random::gen_range_biased(random_generator, 0..=1) as u8);
        self.bw_probe_wait =
            Duration::from_millis(random::gen_range_biased(random_generator, 2000..=3000) as u64);
    }

    #[cfg(test)]
    pub fn set_cycle_phase_for_test(&mut self, cycle_phase: CyclePhase) {
        self.cycle_phase = cycle_phase;

        match cycle_phase {
            CyclePhase::Down => self.ack_phase = AckPhase::ProbeStopping,
            CyclePhase::Refill => self.ack_phase = AckPhase::Refilling,
            CyclePhase::Up => self.ack_phase = AckPhase::ProbeStarting,
            CyclePhase::Cruise => {}
        }
    }
}

/// Methods related to the ProbeBW state
impl BbrCongestionController {
    /// Enters the `ProbeBw` state
    ///
    /// If `cruise_immediately` is true, `CyclePhase::Cruise` will be entered immediately
    /// after entering `CyclePhase::Down`
    #[inline]
    pub(super) fn enter_probe_bw<Pub: Publisher>(
        &mut self,
        cruise_immediately: bool,
        random_generator: &mut dyn random::Generator,
        now: Timestamp,
        publisher: &mut Pub,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBREnterProbeBW():
        //#     BBRStartProbeBW_DOWN()

        let mut state = State::new();
        state.start_down(
            &mut self.congestion_state,
            &mut self.round_counter,
            self.bw_estimator.delivered_bytes(),
            random_generator,
            now,
            publisher,
        );

        if cruise_immediately {
            state.start_cruise(publisher);
        }

        // New BBR state requires updating the model
        self.try_fast_path = false;
        self.state
            .transition_to(bbr::State::ProbeBw(state), publisher);
    }

    /// Transition the current Probe BW cycle phase if necessary
    #[inline]
    pub(super) fn update_probe_bw_cycle_phase<Pub: Publisher>(
        &mut self,
        random_generator: &mut dyn random::Generator,
        now: Timestamp,
        publisher: &mut Pub,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRUpdateProbeBWCyclePhase():
        //#    if (!BBR.filled_pipe)
        //#      return  /* only handling steady-state behavior here */
        //#    BBRAdaptUpperBounds()
        //#    if (!IsInAProbeBWState())
        //#      return /* only handling ProbeBW states here: */
        //#
        //#    switch (state)
        //#
        //#    ProbeBW_DOWN:
        //#      if (BBRCheckTimeToProbeBW())
        //#        return /* already decided state transition */
        //#      if (BBRCheckTimeToCruise())
        //#        BBRStartProbeBW_CRUISE()
        //#
        //#    ProbeBW_CRUISE:
        //#      if (BBRCheckTimeToProbeBW())
        //#        return /* already decided state transition */
        //#
        //#    ProbeBW_REFILL:
        //#      /* After one round of REFILL, start UP */
        //#      if (BBR.round_start)
        //#        BBR.bw_probe_samples = 1
        //#        BBRStartProbeBW_UP()
        //#
        //#    ProbeBW_UP:
        //#      if (BBRHasElapsedInPhase(BBR.min_rtt) and
        //#          inflight > BBRInflight(BBR.max_bw, 1.25))
        //#       BBRStartProbeBW_DOWN()

        debug_assert!(
            self.full_pipe_estimator.filled_pipe(),
            "only handling steady-state behavior here"
        );

        debug_assert!(
            self.state.is_probing_bw(),
            "only handling ProbeBw states here"
        );

        let target_inflight = self.target_inflight();
        let inflight = self.inflight(self.data_rate_model.max_bw(), self.state.pacing_gain());
        let time_to_cruise = self.is_time_to_cruise(now);

        if let bbr::State::ProbeBw(ref mut probe_bw_state) = self.state {
            let prior_cycle_phase = probe_bw_state.cycle_phase();

            if self.round_counter.round_start() {
                probe_bw_state.on_round_start();
            }

            match probe_bw_state.cycle_phase() {
                CyclePhase::Down | CyclePhase::Cruise => {
                    if probe_bw_state.is_time_to_probe_bw(
                        target_inflight,
                        self.max_datagram_size,
                        now,
                    ) {
                        probe_bw_state.start_refill(
                            &mut self.data_volume_model,
                            &mut self.data_rate_model,
                            &mut self.round_counter,
                            self.bw_estimator.delivered_bytes(),
                            publisher,
                        );
                    } else if probe_bw_state.cycle_phase == CyclePhase::Down && time_to_cruise {
                        probe_bw_state.start_cruise(publisher);
                    }
                }
                CyclePhase::Refill => {
                    // After one round of Refill, start Up
                    if self.round_counter.round_start() {
                        self.bw_probe_samples = true;
                        probe_bw_state.start_up(
                            &mut self.round_counter,
                            self.bw_estimator.delivered_bytes(),
                            self.cwnd,
                            self.max_datagram_size,
                            now,
                            publisher,
                        );
                    }
                }
                CyclePhase::Up => {
                    let min_rtt = self
                        .data_volume_model
                        .min_rtt()
                        .expect("at least one RTT has passed");

                    if probe_bw_state.has_elapsed_in_phase(min_rtt, now)
                        && self.bytes_in_flight > inflight
                    {
                        probe_bw_state.start_down(
                            &mut self.congestion_state,
                            &mut self.round_counter,
                            self.bw_estimator.delivered_bytes(),
                            random_generator,
                            now,
                            publisher,
                        );
                    }
                }
            }

            if prior_cycle_phase != probe_bw_state.cycle_phase() {
                // New phase, so need to update cwnd and pacing rate
                self.try_fast_path = false;
            }
        }
    }

    /// Adapt the upper bounds lower or higher depending on the loss rate
    #[inline]
    pub(super) fn adapt_upper_bounds<Pub: Publisher>(
        &mut self,
        bytes_acknowledged: usize,
        random_generator: &mut dyn random::Generator,
        now: Timestamp,
        publisher: &mut Pub,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRAdaptUpperBounds()
        //#   if (BBR.ack_phase == ACKS_PROBE_STARTING and BBR.round_start)
        //#      /* starting to get bw probing samples */
        //#      BBR.ack_phase = ACKS_PROBE_FEEDBACK
        //#    if (BBR.ack_phase == ACKS_PROBE_STOPPING and BBR.round_start)
        //#      /* end of samples from bw probing phase */
        //#      if (IsInAProbeBWState() and !rs.is_app_limited)
        //#        BBRAdvanceMaxBwFilter()
        //#
        //#    if (!CheckInflightTooHigh())
        //#      /* Loss rate is safe. Adjust upper bounds upward. */
        //#      if (BBR.inflight_hi == Infinity or BBR.bw_hi == Infinity)
        //#        return /* no upper bounds to raise */
        //#      if (rs.tx_in_flight > BBR.inflight_hi)
        //#        BBR.inflight_hi = rs.tx_in_flight

        debug_assert!(
            self.full_pipe_estimator.filled_pipe(),
            "only handling steady-state behavior here"
        );

        let rate_sample = self.bw_estimator.rate_sample();

        // Update AckPhase once per round
        if self.round_counter.round_start() {
            self.update_ack_phase(rate_sample);
        }

        if Self::is_inflight_too_high(
            rate_sample,
            self.max_datagram_size,
            self.congestion_state.loss_bursts_in_round(),
            PROBE_BW_FULL_LOSS_COUNT,
        ) {
            if self.bw_probe_samples {
                // Inflight is too high and the sample is from bandwidth probing: lower inflight downward
                self.on_inflight_too_high(
                    rate_sample.is_app_limited,
                    rate_sample.bytes_in_flight,
                    random_generator,
                    now,
                    publisher,
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

            if let bbr::State::ProbeBw(ref mut probe_bw_state) = self.state {
                if probe_bw_state.cycle_phase() == CyclePhase::Up
                    && self.cwnd_limited_in_round
                    && self.cwnd as u64 >= self.data_volume_model.inflight_hi()
                {
                    // inflight_hi is being fully utilized, so probe if we can increase it
                    probe_bw_state.probe_inflight_hi_upward(
                        bytes_acknowledged,
                        &mut self.data_volume_model,
                        self.cwnd,
                        self.max_datagram_size,
                        self.round_counter.round_start(),
                    );
                }
            }
        }
    }

    /// Update AckPhase and advance the Max BW filter if necessary
    #[inline]
    fn update_ack_phase(&mut self, rate_sample: RateSample) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //#   if (BBR.ack_phase == ACKS_PROBE_STARTING and BBR.round_start)
        //#      /* starting to get bw probing samples */
        //#      BBR.ack_phase = ACKS_PROBE_FEEDBACK
        //#    if (BBR.ack_phase == ACKS_PROBE_STOPPING and BBR.round_start)
        //#      /* end of samples from bw probing phase */
        //#      if (IsInAProbeBWState() and !rs.is_app_limited)
        //#        BBRAdvanceMaxBwFilter()

        debug_assert!(self.round_counter.round_start());

        if let bbr::State::ProbeBw(ref mut probe_bw_state) = self.state {
            match probe_bw_state.ack_phase {
                AckPhase::ProbeStarting => {
                    // starting to get bw probing samples
                    probe_bw_state
                        .ack_phase
                        .transition_to(AckPhase::ProbeFeedback);
                }
                AckPhase::ProbeStopping => {
                    // end of samples from bw probing phase
                    self.bw_probe_samples = false;
                    probe_bw_state.ack_phase.transition_to(AckPhase::Init);
                    if !rate_sample.is_app_limited {
                        self.data_rate_model.advance_max_bw_filter();
                    }
                }
                _ => {}
            }
        } else {
            self.bw_probe_samples = false;
        }
    }

    /// Called when loss indicates the current inflight amount is too high
    #[inline]
    pub(super) fn on_inflight_too_high<Pub: Publisher>(
        &mut self,
        is_app_limited: bool,
        bytes_in_flight: u32,
        random_generator: &mut dyn random::Generator,
        now: Timestamp,
        publisher: &mut Pub,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.2
        //# BBRHandleInflightTooHigh()
        //# BBR.bw_probe_samples = 0;  /* only react once per bw probe */
        //#    if (!rs.is_app_limited)
        //#      BBR.inflight_hi = max(rs.tx_in_flight,
        //#                            BBRTargetInflight() * BBRBeta))
        //#    If (BBR.state == ProbeBW_UP)
        //#      BBRStartProbeBW_DOWN()

        self.bw_probe_samples = false; // only react once per bw probe
        if !is_app_limited {
            self.data_volume_model.update_upper_bound(
                (bytes_in_flight as u64)
                    .max((bbr::BETA * self.target_inflight() as u64).to_integer()),
            )
        }

        if let bbr::State::ProbeBw(ref mut probe_bw_state) = self.state {
            if probe_bw_state.cycle_phase() == CyclePhase::Up {
                probe_bw_state.start_down(
                    &mut self.congestion_state,
                    &mut self.round_counter,
                    self.bw_estimator.delivered_bytes(),
                    random_generator,
                    now,
                    publisher,
                );
            }
        }
    }

    /// Returns true if it is time to transition from `Down` to `Cruise`
    #[inline]
    fn is_time_to_cruise(&self, now: Timestamp) -> bool {
        if let (bbr::State::ProbeBw(probe_bw_state), Some(min_rtt)) =
            (&self.state, self.data_volume_model.min_rtt())
        {
            // Chromium and Linux TCP both limit the time spent in ProbeBW_Down to min_rtt
            // See https://github.com/google/bbr/blob/1a45fd4faf30229a3d3116de7bfe9d2f933d3562/net/ipv4/tcp_bbr2.c#L1982-L1981
            //  and https://source.chromium.org/chromium/chromium/src/+/main:net/third_party/quiche/src/quiche/quic/core/congestion_control/bbr2_probe_bw.cc;l=276
            if probe_bw_state.has_elapsed_in_phase(min_rtt, now) {
                return true;
            }
        }

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRCheckTimeToCruise())
        //#   if (inflight > BBRInflightWithHeadroom())
        //#      return false /* not enough headroom */
        //#   if (inflight <= BBRInflight(BBR.max_bw, 1.0))
        //#      return true  /* inflight <= estimated BDP */
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
        path,
        path::MINIMUM_MAX_DATAGRAM_SIZE,
        recovery::{
            bandwidth::{Bandwidth, PacketInfo},
            congestion_controller::PathPublisher,
        },
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

        assert_eq!(CyclePhase::Up, state.cycle_phase);
        assert_eq!(AckPhase::Init, state.ack_phase);
        assert_eq!(Duration::ZERO, state.bw_probe_wait);
        assert_eq!(Counter::new(0), state.rounds_since_bw_probe);
        assert_eq!(0, state.bw_probe_up_acks);
        assert_eq!(0, state.bw_probe_up_rounds);
        assert_eq!(None, state.cycle_start_timestamp);
    }

    #[test]
    fn is_time_to_probe_bw() {
        let mut state = State::new();
        let now = NoopClock.get_time();
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        state
            .cycle_phase
            .transition_to(CyclePhase::Down, &mut publisher);

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

        let bytes_acknowledged = 2400;
        let mut data_volume_model = data_volume::Model::new();
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
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        state
            .cycle_phase
            .transition_to(CyclePhase::Down, &mut publisher);

        state.start_cruise(&mut publisher);

        assert_eq!(CyclePhase::Cruise, state.cycle_phase());
    }

    #[test]
    fn start_up() {
        let mut state = State::new();
        let mut round_counter = round::Counter::default();
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        let delivered_bytes = 100;
        let cwnd = 12000;
        let max_data_size = 1200;
        let now = NoopClock.get_time();

        state.ack_phase = AckPhase::Refilling;
        state.cycle_phase = CyclePhase::Refill;

        state.start_up(
            &mut round_counter,
            delivered_bytes,
            cwnd,
            max_data_size,
            now,
            &mut publisher,
        );

        assert_eq!(CyclePhase::Up, state.cycle_phase());
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
        let mut data_volume_model = data_volume::Model::new();
        let mut data_rate_model = data_rate::Model::new();
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        data_volume_model.update_lower_bound(12000, 12000, true, false, 1.0);
        data_rate_model.update_lower_bound(Bandwidth::ZERO);

        state.ack_phase = AckPhase::ProbeStopping;
        state.cycle_phase = CyclePhase::Cruise;

        state.start_refill(
            &mut data_volume_model,
            &mut data_rate_model,
            &mut round_counter,
            delivered_bytes,
            &mut publisher,
        );

        assert_eq!(CyclePhase::Refill, state.cycle_phase());
        // Lower bounds are reset
        assert_eq!(u64::MAX, data_volume_model.inflight_lo());
        assert_eq!(Bandwidth::INFINITY, data_rate_model.bw_lo());

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
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
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
            &mut publisher,
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
            ecn_ce_count: 0,
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

    #[test]
    fn ack_phase_valid_transitions() {
        let mut ack_phase = AckPhase::Init;
        ack_phase.transition_to(AckPhase::ProbeStopping);
        assert_eq!(ack_phase, AckPhase::ProbeStopping);
        ack_phase.transition_to(AckPhase::Refilling);
        assert_eq!(ack_phase, AckPhase::Refilling);
        ack_phase.transition_to(AckPhase::ProbeStarting);
        assert_eq!(ack_phase, AckPhase::ProbeStarting);
        ack_phase.transition_to(AckPhase::ProbeFeedback);
        assert_eq!(ack_phase, AckPhase::ProbeFeedback);
        ack_phase.transition_to(AckPhase::ProbeStopping);
        assert_eq!(ack_phase, AckPhase::ProbeStopping);
        ack_phase.transition_to(AckPhase::Refilling);
        assert_eq!(ack_phase, AckPhase::Refilling);
        ack_phase.transition_to(AckPhase::ProbeStarting);
        assert_eq!(ack_phase, AckPhase::ProbeStarting);
        ack_phase.transition_to(AckPhase::ProbeStopping);
        assert_eq!(ack_phase, AckPhase::ProbeStopping);
        ack_phase.transition_to(AckPhase::Init);
        assert_eq!(ack_phase, AckPhase::Init);
        ack_phase.transition_to(AckPhase::Refilling);
        assert_eq!(ack_phase, AckPhase::Refilling);
    }

    #[test]
    fn enter_probe_bw() {
        let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let mut rng = random::testing::Generator::default();
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        let now = NoopClock.get_time();
        bbr.state = bbr::State::Drain;

        // cruise_immediately = false
        bbr.enter_probe_bw(false, &mut rng, now, &mut publisher);

        assert!(bbr.state.is_probing_bw());
        if let bbr::State::ProbeBw(probe_bw_state) = bbr.state {
            assert_eq!(CyclePhase::Down, probe_bw_state.cycle_phase());
        }

        assert!(!bbr.try_fast_path);

        // cruise_immediately = true
        bbr.state = bbr::State::Drain;
        bbr.enter_probe_bw(true, &mut rng, now, &mut publisher);
        assert!(bbr.state.is_probing_bw_cruise());
    }

    #[test]
    fn update_ack_phase() {
        let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
        let mut rng = random::testing::Generator::default();
        let mut publisher = event::testing::Publisher::snapshot();
        let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
        let now = NoopClock.get_time();
        bbr.state = bbr::State::Drain;

        // cruise_immediately = false
        bbr.enter_probe_bw(false, &mut rng, now, &mut publisher);

        // Start a new round
        let packet_info = PacketInfo {
            delivered_bytes: 0,
            delivered_time: now,
            lost_bytes: 0,
            ecn_ce_count: 0,
            first_sent_time: now,
            bytes_in_flight: 3000,
            is_app_limited: false,
        };
        bbr.round_counter.on_ack(packet_info, 5000);

        bbr.bw_probe_samples = true;

        assert!(bbr.state.is_probing_bw());
        if let bbr::State::ProbeBw(ref mut probe_bw_state) = bbr.state {
            assert_eq!(probe_bw_state.ack_phase, AckPhase::ProbeStopping);
            assert_eq!(bbr.data_rate_model.cycle_count(), 0);
        }

        let rate_sample = RateSample {
            is_app_limited: false,
            ..Default::default()
        };
        bbr.update_ack_phase(rate_sample);

        // Moving from ProbeStopping to Init increments the cycle count
        if let bbr::State::ProbeBw(ref mut probe_bw_state) = bbr.state {
            assert_eq!(probe_bw_state.ack_phase, AckPhase::Init);
            assert_eq!(bbr.data_rate_model.cycle_count(), 1);
            assert!(!bbr.bw_probe_samples);
        }

        bbr.update_ack_phase(rate_sample);

        // Updating the ack phase again does not increment the cycle count
        if let bbr::State::ProbeBw(ref mut probe_bw_state) = bbr.state {
            assert_eq!(probe_bw_state.ack_phase, AckPhase::Init);
            assert_eq!(bbr.data_rate_model.cycle_count(), 1);

            // set ack phase for the next test
            probe_bw_state.ack_phase = AckPhase::ProbeStarting;
        }

        bbr.bw_probe_samples = true;
        bbr.update_ack_phase(rate_sample);

        if let bbr::State::ProbeBw(ref mut probe_bw_state) = bbr.state {
            assert_eq!(probe_bw_state.ack_phase, AckPhase::ProbeFeedback);
            assert_eq!(bbr.data_rate_model.cycle_count(), 1);
            assert!(bbr.bw_probe_samples);
        }
    }
}
