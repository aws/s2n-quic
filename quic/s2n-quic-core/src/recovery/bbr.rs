// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::Counter,
    event,
    event::IntoEvent,
    random,
    recovery::{
        bandwidth,
        bandwidth::{Bandwidth, RateSample},
        bbr::{
            pacing::Pacer,
            probe_bw::{CyclePhase, PROBE_BW_FULL_LOSS_COUNT},
        },
        congestion_controller,
        congestion_controller::Publisher,
        CongestionController, RttEstimator,
    },
    time::Timestamp,
};
use core::{
    cmp::{max, min},
    convert::TryInto,
    time::Duration,
};
use num_rational::Ratio;
use num_traits::{CheckedMul, Inv, One};

mod congestion;
mod data_rate;
mod data_volume;
mod drain;
mod ecn;
mod full_pipe;
mod pacing;
mod probe_bw;
mod probe_rtt;
mod recovery;
mod round;
mod startup;
mod windowed_filter;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.8
//# The maximum tolerated per-round-trip packet loss rate when probing for bandwidth (the default is 2%).
const LOSS_THRESH: Ratio<u32> = Ratio::new_raw(1, 50);

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.8
//# The default multiplicative decrease to make upon each round trip during which
//# the connection detects packet loss (the value is 0.7)
const BETA: Ratio<u64> = Ratio::new_raw(7, 10);

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.8
//# The multiplicative factor to apply to BBR.inflight_hi when attempting to leave free headroom in
//# the path (e.g. free space in the bottleneck buffer or free time slots in the bottleneck link)
//# that can be used by cross traffic (the value is 0.85).
const HEADROOM: Ratio<u64> = Ratio::new_raw(85, 100);

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.8
//# The minimal cwnd value BBR targets, to allow pipelining with TCP endpoints
//# that follow an "ACK every other packet" delayed-ACK policy: 4 * SMSS.
const MIN_PIPE_CWND_PACKETS: u16 = 4;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.1.1
//# The following state transition diagram summarizes the flow of control and the relationship between the different states:
//#
//#              |
//#              V
//#     +---> Startup  ------------+
//#     |        |                 |
//#     |        V                 |
//#     |     Drain  --------------+
//#     |        |                 |
//#     |        V                 |
//#     +---> ProbeBW_DOWN  -------+
//#     | ^      |                 |
//#     | |      V                 |
//#     | |   ProbeBW_CRUISE ------+
//#     | |      |                 |
//#     | |      V                 |
//#     | |   ProbeBW_REFILL  -----+
//#     | |      |                 |
//#     | |      V                 |
//#     | |   ProbeBW_UP  ---------+
//#     | |      |                 |
//#     | +------+                 |
//#     |                          |
//#     +---- ProbeRTT <-----------+
#[derive(Clone, Debug)]
enum State {
    Startup,
    Drain,
    ProbeBw(probe_bw::State),
    ProbeRtt(probe_rtt::State),
}

impl State {
    /// The dynamic gain factor used to scale BBR.bw to produce BBR.pacing_rate
    fn pacing_gain(&self) -> Ratio<u64> {
        match self {
            State::Startup => startup::PACING_GAIN,
            State::Drain => drain::PACING_GAIN,
            State::ProbeBw(probe_bw_state) => probe_bw_state.cycle_phase().pacing_gain(),
            State::ProbeRtt(_) => probe_rtt::PACING_GAIN,
        }
    }

    /// The dynamic gain factor used to scale the estimated BDP to produce a congestion window (cwnd)
    fn cwnd_gain(&self) -> Ratio<u64> {
        match self {
            State::Startup => startup::CWND_GAIN,
            State::Drain => drain::CWND_GAIN,
            State::ProbeBw(_) => probe_bw::CWND_GAIN,
            State::ProbeRtt(_) => probe_rtt::CWND_GAIN,
        }
    }

    /// True if the current state is Startup
    fn is_startup(&self) -> bool {
        matches!(self, State::Startup)
    }

    /// True if the current state is Drain
    fn is_drain(&self) -> bool {
        matches!(self, State::Drain)
    }

    /// True if the current state is ProbeBw
    fn is_probing_bw(&self) -> bool {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# IsInAProbeBWState()
        //#   state = BBR.state
        //#   return (state == ProbeBW_DOWN or
        //#           state == ProbeBW_CRUISE or
        //#           state == ProbeBW_REFILL or
        //#           state == ProbeBW_UP)
        matches!(self, State::ProbeBw(_))
    }

    /// True if the current state is ProbeBw and the CyclePhase is `Up`
    fn is_probing_bw_up(&self) -> bool {
        if let State::ProbeBw(probe_bw_state) = self {
            return probe_bw_state.cycle_phase() == CyclePhase::Up;
        }
        false
    }

    /// True if the current state is ProbeBw and the CyclePhase is `Cruise`
    fn is_probing_bw_cruise(&self) -> bool {
        if let State::ProbeBw(probe_bw_state) = self {
            return probe_bw_state.cycle_phase() == CyclePhase::Cruise;
        }
        false
    }

    /// True if the current state is ProbeBw and the CyclePhase is `Refill`
    fn is_probing_bw_refill(&self) -> bool {
        if let State::ProbeBw(probe_bw_state) = self {
            return probe_bw_state.cycle_phase() == CyclePhase::Refill;
        }
        false
    }

    /// True if the current state is ProbeRtt
    fn is_probing_rtt(&self) -> bool {
        matches!(self, State::ProbeRtt(_))
    }

    /// True if BBR is accelerating sending in order to probe for bandwidth
    ///
    /// Note: This is not the same as `is_probing_bw`, as states other than
    ///       `State::ProbingBw` are also considered as probing for bandwidth
    ///       and not every `ProbingBw` sub-state is actually probing.
    ///
    /// See https://github.com/google/bbr/blob/a23c4bb59e0c5a505fc0f5cc84c4d095a64ed361/net/ipv4/tcp_bbr2.c#L1348
    fn is_probing_for_bandwidth(&self) -> bool {
        self.is_startup() || self.is_probing_bw_up() || self.is_probing_bw_refill()
    }

    /// Transition to the given `new_state`
    fn transition_to<Pub: Publisher>(&mut self, new_state: State, publisher: &mut Pub) {
        if cfg!(debug_assertions) {
            match &new_state {
                // BBR is initialized in the Startup state, but may re-enter Startup after ProbeRtt
                State::Startup => assert!(self.is_probing_rtt()),
                State::Drain => assert!(self.is_startup()),
                State::ProbeBw(_) => assert!(self.is_drain() || self.is_probing_rtt()),
                State::ProbeRtt(_) => {} // ProbeRtt may be entered anytime
            }
        }

        if !new_state.is_probing_bw() {
            // ProbeBw::CyclePhase emits this metric for the ProbingBw state
            publisher.on_bbr_state_changed(new_state.into_event());
        }

        *self = new_state;
    }
}

impl IntoEvent<event::builder::BbrState> for &State {
    fn into_event(self) -> event::builder::BbrState {
        use event::builder::BbrState;
        match self {
            State::Startup => BbrState::Startup,
            State::Drain => BbrState::Drain,
            State::ProbeBw(probe_bw_state) => probe_bw_state.cycle_phase().into_event(),
            State::ProbeRtt(_) => BbrState::ProbeRtt,
        }
    }
}

/// A congestion controller that implements "Bottleneck Bandwidth and Round-trip propagation time"
/// version 2 (BBRv2) as specified in <https://datatracker.ietf.org/doc/draft-cardwell-iccrg-bbr-congestion-control/>.
///
/// Based in part on the Chromium BBRv2 implementation, see <https://source.chromium.org/chromium/chromium/src/+/main:net/third_party/quiche/src/quic/core/congestion_control/bbr2_sender.cc>
/// and the Linux Kernel TCP BBRv2 implementation, see <https://github.com/google/bbr/blob/v2alpha/net/ipv4/tcp_bbr2.c>
#[derive(Debug, Clone)]
pub struct BbrCongestionController {
    state: State,
    round_counter: round::Counter,
    bw_estimator: bandwidth::Estimator,
    full_pipe_estimator: full_pipe::Estimator,
    //= https://www.rfc-editor.org/rfc/rfc9002#section-B.2
    //# The sum of the size in bytes of all sent packets
    //# that contain at least one ack-eliciting or PADDING frame and have
    //# not been acknowledged or declared lost.  The size does not include
    //# IP or UDP overhead, but does include the QUIC header and
    //# Authenticated Encryption with Associated Data (AEAD) overhead.
    //# Packets only containing ACK frames do not count toward
    //# bytes_in_flight to ensure congestion control does not impede
    //# congestion feedback.
    bytes_in_flight: BytesInFlight,
    cwnd: u32,
    prior_cwnd: u32,
    recovery_state: recovery::State,
    congestion_state: congestion::State,
    ecn_state: ecn::State,
    data_rate_model: data_rate::Model,
    data_volume_model: data_volume::Model,
    max_datagram_size: u16,
    /// A boolean that is true if and only if a connection is restarting after being idle
    idle_restart: bool,
    /// True if rate samples reflect bandwidth probing
    bw_probe_samples: bool,
    /// Controls the departure time and send quantum of packets
    pacer: Pacer,
    /// If true, we can attempt to avoid updating control parameters and/or model parameters
    try_fast_path: bool,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.1
    //# True if the connection has fully utilized its cwnd at any point in the last packet-timed round trip.
    cwnd_limited_in_round: bool,
}

type BytesInFlight = Counter<u32>;

impl CongestionController for BbrCongestionController {
    type PacketInfo = bandwidth::PacketInfo;

    #[inline]
    fn congestion_window(&self) -> u32 {
        self.cwnd
    }

    #[inline]
    fn bytes_in_flight(&self) -> u32 {
        *self.bytes_in_flight
    }

    #[inline]
    fn is_congestion_limited(&self) -> bool {
        let available_congestion_window = self
            .congestion_window()
            .saturating_sub(*self.bytes_in_flight);
        available_congestion_window < self.max_datagram_size as u32
    }

    #[inline]
    fn requires_fast_retransmission(&self) -> bool {
        self.recovery_state.requires_fast_retransmission()
    }

    #[inline]
    fn on_packet_sent<Pub: Publisher>(
        &mut self,
        time_sent: Timestamp,
        sent_bytes: usize,
        app_limited: Option<bool>,
        rtt_estimator: &RttEstimator,
        publisher: &mut Pub,
    ) -> Self::PacketInfo {
        let prior_bytes_in_flight = *self.bytes_in_flight;

        if sent_bytes > 0 {
            self.recovery_state.on_packet_sent();

            //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.2.2
            //# BBROnTransmit():
            //#   BBRHandleRestartFromIdle()
            self.handle_restart_from_idle(time_sent, publisher);

            self.bytes_in_flight
                .try_add(sent_bytes)
                .expect("sent_bytes should not exceed u32::MAX");
            self.pacer
                .on_packet_sent(time_sent, sent_bytes, rtt_estimator.smoothed_rtt());
            self.cwnd_limited_in_round |= self.is_congestion_limited();
        }

        self.bw_estimator
            .on_packet_sent(prior_bytes_in_flight, sent_bytes, app_limited, time_sent)
    }

    #[inline]
    fn on_rtt_update<Pub: Publisher>(
        &mut self,
        _time_sent: Timestamp,
        _now: Timestamp,
        rtt_estimator: &RttEstimator,
        publisher: &mut Pub,
    ) {
        if self.data_volume_model.min_rtt().is_none() {
            // This is the first RTT estimate, so initialize the pacing rate to
            // override the default initialized value with a more realistic value
            self.pacer.initialize_pacing_rate(
                self.cwnd,
                rtt_estimator.smoothed_rtt(),
                self.state.pacing_gain(),
                publisher,
            );
        }

        // BBRUpdateMinRTT() called in `on_ack`
    }

    #[inline]
    fn on_ack<Pub: Publisher>(
        &mut self,
        newest_acked_time_sent: Timestamp,
        bytes_acknowledged: usize,
        newest_acked_packet_info: Self::PacketInfo,
        rtt_estimator: &RttEstimator,
        random_generator: &mut dyn random::Generator,
        ack_receive_time: Timestamp,
        publisher: &mut Pub,
    ) {
        let is_cwnd_limited = self.is_congestion_limited();
        self.bytes_in_flight
            .try_sub(bytes_acknowledged)
            .expect("bytes_acknowledged should not exceed u32::MAX");
        self.bw_estimator.on_ack(
            bytes_acknowledged,
            newest_acked_time_sent,
            newest_acked_packet_info,
            ack_receive_time,
            publisher,
        );
        self.round_counter.on_ack(
            newest_acked_packet_info,
            self.bw_estimator.delivered_bytes(),
        );
        if self
            .recovery_state
            .on_ack(self.round_counter.round_start(), newest_acked_time_sent)
        {
            // This ack caused recovery to be exited
            self.on_exit_fast_recovery();
        }
        if self.round_counter.round_start() {
            self.ecn_state
                .on_round_start(self.bw_estimator.delivered_bytes(), self.max_datagram_size);
            self.cwnd_limited_in_round = is_cwnd_limited;
        }

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.2.3
        //# On every ACK, the BBR algorithm executes the following BBRUpdateOnACK() steps in order
        //# to update its network path model, update its state machine, and adjust its control
        //# parameters to adapt to the updated model:
        //# BBRUpdateOnACK():
        //#   BBRUpdateModelAndState()
        //#   BBRUpdateControlParameters()

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.2.3
        //# BBRUpdateModelAndState():
        //#   BBRUpdateLatestDeliverySignals()
        //#   BBRUpdateCongestionSignals()
        // implements BBRUpdateLatestDeliverySignals() and BBRUpdateCongestionSignals()

        // Check if we need to update model parameters
        let update_model = self.model_update_required();

        if update_model {
            self.update_latest_signals(newest_acked_packet_info);
            //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.2.3
            //# BBRUpdateACKAggregation()
            self.data_volume_model.update_ack_aggregation(
                self.data_rate_model.bw(),
                bytes_acknowledged,
                self.cwnd,
                self.round_counter.round_count(),
                ack_receive_time,
            );

            //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.2.3
            //# BBRCheckStartupDone()
            //# BBRCheckDrain()
            self.check_startup_done(publisher);
        }
        self.check_drain_done(random_generator, ack_receive_time, publisher);

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.2.3
        //# BBRUpdateProbeBWCyclePhase()
        if self.full_pipe_estimator.filled_pipe() {
            // BBRUpdateProbeBWCyclePhase() internally calls BBRAdaptUpperBounds() if BBR.filled_pipe == true
            self.adapt_upper_bounds(
                bytes_acknowledged,
                random_generator,
                ack_receive_time,
                publisher,
            );
            if self.state.is_probing_bw() {
                self.update_probe_bw_cycle_phase(random_generator, ack_receive_time, publisher);
            }
        }

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.2.3
        //# BBRUpdateMinRTT()
        //# BBRCheckProbeRTT()
        //# BBRAdvanceLatestDeliverySignals()
        //# BBRBoundBWForModel()
        let prev_min_rtt = self.data_volume_model.min_rtt();
        self.data_volume_model
            .update_min_rtt(rtt_estimator.latest_rtt(), ack_receive_time);
        self.check_probe_rtt(random_generator, ack_receive_time, publisher);

        // Update control parameters if required
        if self.control_update_required(update_model, prev_min_rtt) {
            self.congestion_state
                .advance(self.bw_estimator.rate_sample());
            self.data_rate_model.bound_bw_for_model();

            //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.2.3
            //# BBRUpdateControlParameters():
            //#   BBRSetPacingRate()
            //#   BBRSetSendQuantum()
            //#   BBRSetCwnd()
            self.pacer.set_pacing_rate(
                self.data_rate_model.bw(),
                self.state.pacing_gain(),
                self.full_pipe_estimator.filled_pipe(),
                publisher,
            );
            self.pacer.set_send_quantum(self.max_datagram_size);
            self.set_cwnd(bytes_acknowledged);
        }
    }

    #[inline]
    fn on_packet_lost<Pub: Publisher>(
        &mut self,
        lost_bytes: u32,
        packet_info: Self::PacketInfo,
        _persistent_congestion: bool,
        new_loss_burst: bool,
        random_generator: &mut dyn random::Generator,
        timestamp: Timestamp,
        publisher: &mut Pub,
    ) {
        debug_assert!(lost_bytes > 0);

        self.bytes_in_flight -= lost_bytes;
        self.bw_estimator.on_loss(lost_bytes as usize);
        if self.recovery_state.on_congestion_event(timestamp) {
            // this congestion event caused the connection to enter recovery
            self.on_enter_fast_recovery();
        }
        self.congestion_state
            .on_packet_lost(self.bw_estimator.delivered_bytes(), new_loss_burst);

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.2.4
        //# BBRUpdateOnLoss(packet):
        //#   BBRHandleLostPacket(packet)
        self.handle_lost_packet(
            lost_bytes,
            packet_info,
            random_generator,
            timestamp,
            publisher,
        );
    }

    #[inline]
    fn on_explicit_congestion<Pub: Publisher>(
        &mut self,
        ce_count: u64,
        event_time: Timestamp,
        _publisher: &mut Pub,
    ) {
        self.bw_estimator.on_explicit_congestion(ce_count);
        self.ecn_state.on_explicit_congestion(ce_count);
        self.congestion_state.on_explicit_congestion();
        if self.recovery_state.on_congestion_event(event_time) {
            self.on_enter_fast_recovery();
        }
    }

    //= https://www.rfc-editor.org/rfc/rfc8899#section-3
    //= type=exception
    //= reason=See https://github.com/aws/s2n-quic/issues/959
    //# An update to the PLPMTU (or MPS) MUST NOT increase the congestion
    //# window measured in bytes [RFC4821].

    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.2
    //= type=exception
    //= reason=The maximum datagram size remains at the minimum (1200 bytes) during the handshake
    //# If the maximum datagram size is decreased in order to complete the
    //# handshake, the congestion window SHOULD be set to the new initial
    //# congestion window.
    #[inline]
    fn on_mtu_update<Pub: Publisher>(&mut self, max_datagram_size: u16, _publisher: &mut Pub) {
        let old_max_datagram_size = self.max_datagram_size;
        self.max_datagram_size = max_datagram_size;

        self.cwnd =
            ((self.cwnd as f32 / old_max_datagram_size as f32) * max_datagram_size as f32) as u32;
    }

    #[inline]
    fn on_packet_discarded<Pub: Publisher>(&mut self, bytes_sent: usize, _publisher: &mut Pub) {
        self.bytes_in_flight
            .try_sub(bytes_sent)
            .expect("bytes sent should not exceed u32::MAX");
        self.bw_estimator.on_packet_discarded(bytes_sent);
        self.recovery_state.on_packet_discarded();
    }

    #[inline]
    fn earliest_departure_time(&self) -> Option<Timestamp> {
        self.pacer.earliest_departure_time()
    }

    #[inline]
    fn send_quantum(&self) -> Option<usize> {
        Some(self.pacer.send_quantum())
    }
}

impl BbrCongestionController {
    /// Constructs a new `BbrCongestionController`
    pub fn new(max_datagram_size: u16) -> Self {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.2.1
        //# BBROnInit():
        //#   init_windowed_max_filter(filter=BBR.MaxBwFilter, value=0, time=0)
        //#   BBR.min_rtt = SRTT ? SRTT : Inf
        //#   BBR.min_rtt_stamp = Now()
        //#   BBR.probe_rtt_done_stamp = 0
        //#   BBR.probe_rtt_round_done = false
        //#   BBR.prior_cwnd = 0
        //#   BBR.idle_restart = false
        //#   BBR.extra_acked_interval_start = Now()
        //#   BBR.extra_acked_delivered = 0
        //#   BBRResetCongestionSignals()
        //#   BBRResetLowerBounds()
        //#   BBRInitRoundCounting()
        //#   BBRInitFullPipe()
        //#   BBRInitPacingRate()
        //#   BBREnterStartup()

        // BBRResetCongestionSignals() is implemented by the default congestion::State
        // BBRResetLowerBounds() is implemented by data_rate::Model::new() and data_volume::Model::new()
        // BBRInitRoundCounting() is implemented by round::Counter::default()
        // BBRInitFullPipe() is implemented by full_pipe::Estimator::default()

        Self {
            state: State::Startup,
            round_counter: Default::default(),
            bw_estimator: Default::default(),
            full_pipe_estimator: Default::default(),
            bytes_in_flight: Default::default(),
            cwnd: Self::initial_window(max_datagram_size),
            prior_cwnd: 0,
            recovery_state: recovery::State::Recovered,
            congestion_state: Default::default(),
            ecn_state: Default::default(),
            data_rate_model: data_rate::Model::new(),
            // initialize extra_acked_interval_start and extra_acked_delivered
            data_volume_model: data_volume::Model::new(),
            max_datagram_size,
            idle_restart: false,
            bw_probe_samples: false,
            pacer: Pacer::new(max_datagram_size),
            try_fast_path: false,
            cwnd_limited_in_round: false,
        }
    }
    /// The bandwidth-delay product
    ///
    /// Based on the current estimate of maximum sending bandwidth and minimum RTT
    #[inline]
    fn bdp(&self) -> u64 {
        self.bdp_multiple(self.data_rate_model.bw(), Ratio::one())
    }

    /// Calculates a bandwidth-delay product using the supplied `Bandwidth` and `gain`
    #[inline]
    fn bdp_multiple(&self, bw: Bandwidth, gain: Ratio<u64>) -> u64 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
        //# BBRBDPMultiple(gain):
        //#   if (BBR.min_rtt == Inf)
        //#       return InitialCwnd /* no valid RTT samples yet */
        //#     BBR.bdp = BBR.bw * BBR.min_rtt
        //#     return gain * BBR.bdp

        if let Some(min_rtt) = self.data_volume_model.min_rtt() {
            gain.checked_mul(&(bw * min_rtt).into())
                .map_or(u64::MAX, |bdp| bdp.to_integer())
        } else {
            Self::initial_window(self.max_datagram_size).into()
        }
    }

    /// How much data do we want in flight
    ///
    /// Based on the estimated BDP, unless congestion reduced the cwnd
    #[inline]
    fn target_inflight(&self) -> u32 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.5.3
        //# BBRTargetInflight()
        //#   return min(BBR.bdp, cwnd)

        self.bdp().min(self.cwnd as u64) as u32
    }

    /// The estimate of the volume of in-flight data required to fully utilize the bottleneck
    /// bandwidth available to the flow
    ///
    /// Based on the BDP estimate (BBR.bdp), the aggregation estimate (BBR.extra_acked), the
    /// offload budget (BBR.offload_budget), and BBRMinPipeCwnd.
    #[inline]
    fn max_inflight(&self) -> u64 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
        //# BBRUpdateMaxInflight()
        //#   BBRUpdateAggregationBudget()
        //#   inflight = BBRBDPMultiple(BBR.cwnd_gain)
        //#   inflight += BBR.extra_acked
        //#   BBR.max_inflight = BBRQuantizationBudget(inflight)

        // max_inflight is calculated and returned from this function
        // as needed, rather than maintained as a field

        let bdp = self.bdp_multiple(self.data_rate_model.bw(), self.state.cwnd_gain());
        let inflight = bdp.saturating_add(self.data_volume_model.extra_acked());
        self.quantization_budget(inflight)
    }

    /// Inflight based on min RTT and the estimated bottleneck bandwidth
    #[inline]
    fn inflight(&self, bw: Bandwidth, gain: Ratio<u64>) -> u32 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
        //# BBRInflight(gain)
        //#   inflight = BBRBDPMultiple(gain)
        //#   return BBRQuantizationBudget(inflight)

        // BBRInflight is defined in the RFC with and without a Bandwidth input

        let inflight = self.bdp_multiple(bw, gain);
        self.quantization_budget(inflight)
            .try_into()
            .unwrap_or(u32::MAX)
    }

    /// The volume of data that tries to leave free headroom in the bottleneck buffer or link for
    /// other flows, for fairness convergence and lower RTTs and loss
    #[inline]
    fn inflight_with_headroom(&self) -> u32 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRInflightWithHeadroom()
        //#   if (BBR.inflight_hi == Infinity)
        //#     return Infinity
        //#   headroom = max(1, BBRHeadroom * BBR.inflight_hi)
        //#     return max(BBR.inflight_hi - headroom,
        //#                BBRMinPipeCwnd)

        if self.data_volume_model.inflight_hi() == u64::MAX {
            return u32::MAX;
        }

        // The RFC pseudocode mistakenly subtracts headroom (representing 85% of inflight_hi)
        // from inflight_hi, resulting a reduction to 15% of inflight_hi. Since the intention is
        // to reduce inflight_hi to 85% of inflight_hi, we can just multiply by `HEADROOM`.
        // See https://groups.google.com/g/bbr-dev/c/xmley7VkeoE/m/uXDlnxiuCgAJ
        let inflight_with_headroom = (HEADROOM * self.data_volume_model.inflight_hi())
            .to_integer()
            .try_into()
            .unwrap_or(u32::MAX);

        inflight_with_headroom.max(self.minimum_window())
    }

    /// Calculates the quantization budget
    #[inline]
    fn quantization_budget(&self, inflight: u64) -> u64 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
        //# BBRQuantizationBudget(inflight)
        //#   BBRUpdateOffloadBudget()
        //#   inflight = max(inflight, BBR.offload_budget)
        //#   inflight = max(inflight, BBRMinPipeCwnd)
        //#   if (BBR.state == ProbeBW && BBR.cycle_idx == ProbeBW_UP)
        //#     inflight += 2
        //#   return inflight

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.4
        //# BBRUpdateOffloadBudget():
        //#   BBR.offload_budget = 3 * BBR.send_quantum
        let offload_budget = 3 * self.pacer.send_quantum() as u64;

        let mut inflight = inflight
            .max(offload_budget)
            .max(self.minimum_window() as u64);

        if self.state.is_probing_bw_up() {
            inflight = inflight.saturating_add(2 * self.max_datagram_size as u64);
        }

        inflight
    }

    /// True if the amount of loss or ECN CE markings exceed the BBR thresholds
    #[inline]
    fn is_inflight_too_high(
        rate_sample: RateSample,
        max_datagram_size: u16,
        loss_bursts: u8,
        loss_burst_limit: u8,
    ) -> bool {
        if Self::is_loss_too_high(
            rate_sample.lost_bytes,
            rate_sample.bytes_in_flight,
            loss_bursts,
            loss_burst_limit,
        ) {
            return true;
        }

        if rate_sample.delivered_bytes > 0 {
            let ecn_ce_ratio = ecn::ce_ratio(
                rate_sample.ecn_ce_count,
                rate_sample.delivered_bytes,
                max_datagram_size,
            );
            return ecn::is_ce_too_high(ecn_ce_ratio);
        }

        false
    }

    /// True if the amount of `lost_bytes` exceeds the BBR loss threshold and the count of loss
    /// bursts is greater than or equal to the loss burst limit
    #[inline]
    fn is_loss_too_high(
        lost_bytes: u64,
        bytes_inflight: u32,
        loss_bursts: u8,
        loss_burst_limit: u8,
    ) -> bool {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.2
        //# IsInflightTooHigh()
        //#   return (rs.lost > rs.tx_in_flight * BBRLossThresh)
        loss_bursts >= loss_burst_limit
            && lost_bytes > (LOSS_THRESH * bytes_inflight).to_integer() as u64
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.2
    //# Endpoints SHOULD use an initial congestion
    //# window of ten times the maximum datagram size (max_datagram_size),
    //# while limiting the window to the larger of 14,720 bytes or twice the
    //# maximum datagram size.
    #[inline]
    fn initial_window(max_datagram_size: u16) -> u32 {
        const INITIAL_WINDOW_LIMIT: u32 = 14720;
        min(
            10 * max_datagram_size as u32,
            max(INITIAL_WINDOW_LIMIT, 2 * max_datagram_size as u32),
        )
    }

    /// The minimal cwnd value BBR targets
    #[inline]
    fn minimum_window(&self) -> u32 {
        (MIN_PIPE_CWND_PACKETS * self.max_datagram_size) as u32
    }

    /// Updates the congestion window based on the latest model
    #[inline]
    fn set_cwnd(&mut self, newly_acked: usize) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.6
        //# BBRSetCwnd():
        //#   BBRUpdateMaxInflight()
        //#   BBRModulateCwndForRecovery()
        //#   if (!BBR.packet_conservation) {
        //#     if (BBR.filled_pipe)
        //#       cwnd = min(cwnd + rs.newly_acked, BBR.max_inflight)
        //#     else if (cwnd < BBR.max_inflight || C.delivered < InitialCwnd)
        //#       cwnd = cwnd + rs.newly_acked
        //#     cwnd = max(cwnd, BBRMinPipeCwnd)
        //#  }
        //#  BBRBoundCwndForProbeRTT()
        //#  BBRBoundCwndForModel()

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.6
        //= type=exception
        //= reason=https://github.com/aws/s2n-quic/issues/1511
        //#   BBRModulateCwndForRecovery()

        let max_inflight = self.max_inflight().try_into().unwrap_or(u32::MAX);
        let initial_cwnd = Self::initial_window(self.max_datagram_size);
        let mut cwnd = self.cwnd;

        // Enable fast path if the cwnd has reached max_inflight
        // Adapted from the Linux TCP BBRv2 implementation
        // See https://github.com/google/bbr/blob/1a45fd4faf30229a3d3116de7bfe9d2f933d3562/net/ipv4/tcp_bbr2.c#L923
        self.try_fast_path = false;

        if self.full_pipe_estimator.filled_pipe() {
            cwnd = cwnd.saturating_add(newly_acked as u32);
            if cwnd >= max_inflight {
                cwnd = max_inflight;
                self.try_fast_path = true;
            }
        } else if cwnd < max_inflight
            || self.bw_estimator.delivered_bytes() < 2 * initial_cwnd as u64
        {
            // cwnd has room to grow, or so little data has been delivered that max_inflight should not be used
            // The Linux TCP BBRv2 implementation and Chromium BBRv2 implementation both use 2 * initial_cwnd here
            // See https://github.com/google/bbr/blob/1ee29b79317a3028ed1fcd85cb46da009f45de00/net/ipv4/tcp_bbr2.c#L931
            // and https://source.chromium.org/chromium/chromium/src/+/main:net/third_party/quiche/src/quiche/quic/core/congestion_control/bbr2_sender.cc;l=404;bpv=1;bpt=1
            cwnd += newly_acked as u32;
        } else {
            self.try_fast_path = true;
        }

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.5
        //# BBRBoundCwndForProbeRTT():
        //#   if (BBR.state == ProbeRTT)
        //#     cwnd = min(cwnd, BBRProbeRTTCwnd())
        if self.state.is_probing_rtt() {
            cwnd = cwnd.min(self.probe_rtt_cwnd());
        }

        // Ensure the cwnd is at least the minimum window, and at most the bound defined by the model.
        // This applies regardless of whether packet conservation is in place, as the pseudocode
        // applies this clamping within BBRBoundCwndForModel(), which is called after all prior
        // cwnd adjustments have been made.
        self.cwnd = cwnd.clamp(self.minimum_window(), self.bound_cwnd_for_model());
    }

    /// Returns the maximum congestion window bound by recent congestion
    #[inline]
    fn bound_cwnd_for_model(&self) -> u32 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.7
        //# BBRBoundCwndForModel():
        //#   cap = Infinity
        //#   if (IsInAProbeBWState() and
        //#       BBR.state != ProbeBW_CRUISE)
        //#     cap = BBR.inflight_hi
        //#   else if (BBR.state == ProbeRTT or
        //#            BBR.state == ProbeBW_CRUISE)
        //#     cap = BBRInflightWithHeadroom()
        //#
        //#   /* apply inflight_lo (possibly infinite): */
        //#   cap = min(cap, BBR.inflight_lo)
        //#   cap = max(cap, BBRMinPipeCwnd)
        //#   cwnd = min(cwnd, cap)
        let inflight_hi = self
            .data_volume_model
            .inflight_hi()
            .try_into()
            .unwrap_or(u32::MAX);
        let inflight_lo = self
            .data_volume_model
            .inflight_lo()
            .try_into()
            .unwrap_or(u32::MAX);

        let cap = if self.state.is_probing_bw() && !self.state.is_probing_bw_cruise() {
            inflight_hi
        } else if self.state.is_probing_rtt() || self.state.is_probing_bw_cruise() {
            self.inflight_with_headroom()
        } else {
            u32::MAX
        };

        cap.min(inflight_lo).max(self.minimum_window())
    }

    /// Saves the last-known good congestion window (the latest cwnd unmodulated by loss recovery or ProbeRTT)
    #[inline]
    fn save_cwnd(&mut self) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.4
        //# BBRSaveCwnd()
        //#   if (!InLossRecovery() and BBR.state != ProbeRTT)
        //#     return cwnd
        //#   else
        //#     return max(BBR.prior_cwnd, cwnd)

        self.prior_cwnd = if !self.recovery_state.in_recovery() && !self.state.is_probing_rtt() {
            self.cwnd
        } else {
            self.prior_cwnd.max(self.cwnd)
        }
    }

    /// Restores the last-known good congestion window (the latest cwnd unmodulated by loss recovery or ProbeRTT)
    #[inline]
    fn restore_cwnd(&mut self) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.4
        //# BBRRestoreCwnd()
        //#   cwnd = max(cwnd, BBR.prior_cwnd)

        self.cwnd = self.cwnd.max(self.prior_cwnd);
    }

    /// Called when entering fast recovery
    #[inline]
    fn on_enter_fast_recovery(&mut self) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.4
        //# Upon entering Fast Recovery, set cwnd to the number of packets still in flight
        //# (allowing at least one for a fast retransmit):
        //#
        //# BBROnEnterFastRecovery():
        //#   BBR.prior_cwnd = BBRSaveCwnd()
        //#   cwnd = packets_in_flight + max(rs.newly_acked, 1)
        //#   BBR.packet_conservation = true

        debug_assert!(self.recovery_state.in_recovery());

        // packet_conservation is true while in the state `recovery::State::Conservation`. That
        // state is entered prior to this method being called, when packet loss is recorded.
        debug_assert!(self.recovery_state.packet_conservation());

        //self.save_cwnd();
        // BBROnEnterFastRecovery() tries to allow for at least one fast retransmit packet in the
        // the congestion window. The recovery manager will already allow for this fast retransmit
        // even if we are blocked by congestion control, as long as requires_fast_retransmission()
        // returns true.
        // self.cwnd = self.bytes_in_flight();
    }

    /// Called when exiting fast recovery
    #[inline]
    fn on_exit_fast_recovery(&mut self) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.4
        //# Upon exiting loss recovery (RTO recovery or Fast Recovery), either by repairing all
        //# losses or undoing recovery, BBR restores the best-known cwnd value we had upon entering
        //# loss recovery:
        //#
        //#   BBR.packet_conservation = false
        //#   BBRRestoreCwnd()

        debug_assert!(!self.recovery_state.in_recovery());

        // When fast recovery is exited, the state changes to `recovery::State::Recovered`, which
        // has packet_conservation as false
        debug_assert!(!self.recovery_state.packet_conservation());

        //self.restore_cwnd();

        // self.data_volume_model.reset_lower_bound();
        // self.data_rate_model.reset_lower_bound();

        // Since we are exiting a recovery period, we need to make sure the model is updated
        // and the congestion window is bound appropriately
        self.try_fast_path = false;
    }

    #[inline]
    fn handle_lost_packet<Pub: Publisher>(
        &mut self,
        lost_bytes: u32,
        packet_info: <BbrCongestionController as CongestionController>::PacketInfo,
        random_generator: &mut dyn random::Generator,
        now: Timestamp,
        publisher: &mut Pub,
    ) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.2
        //# if (!BBR.bw_probe_samples)
        //#   return /* not a packet sent while probing bandwidth */
        //# rs.tx_in_flight = packet.tx_in_flight /* inflight at transmit */
        //# rs.lost = C.lost - packet.lost /* data lost since transmit */
        //# rs.is_app_limited = packet.is_app_limited;
        //# if (IsInflightTooHigh(rs))
        //#   rs.tx_in_flight = BBRInflightHiFromLostPacket(rs, packet)
        //#   BBRHandleInflightTooHigh(rs)

        if !self.bw_probe_samples {
            // not a packet sent while probing bandwidth
            return;
        }

        let lost_since_transmit = (self.bw_estimator.lost_bytes() - packet_info.lost_bytes)
            .try_into()
            .unwrap_or(u32::MAX);

        if Self::is_loss_too_high(
            lost_since_transmit as u64,
            packet_info.bytes_in_flight,
            self.congestion_state.loss_bursts_in_round(),
            PROBE_BW_FULL_LOSS_COUNT,
        ) {
            let inflight_hi_from_lost_packet =
                Self::inflight_hi_from_lost_packet(lost_bytes, lost_since_transmit, packet_info);
            self.on_inflight_too_high(
                packet_info.is_app_limited,
                inflight_hi_from_lost_packet,
                random_generator,
                now,
                publisher,
            );
        }
    }

    /// Returns the prefix of packet where losses exceeded `LOSS_THRESH`
    #[inline]
    fn inflight_hi_from_lost_packet(
        size: u32,
        lost_since_transmit: u32,
        packet_info: <BbrCongestionController as CongestionController>::PacketInfo,
    ) -> u32 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.2
        //# BBRInflightHiFromLostPacket(rs, packet):
        //#   size = packet.size
        //#   /* What was in flight before this packet? */
        //#   inflight_prev = rs.tx_in_flight - size
        //#   /* What was lost before this packet? */
        //#   lost_prev = rs.lost - size
        //#   lost_prefix = (BBRLossThresh * inflight_prev - lost_prev) /
        //#                 (1 - BBRLossThresh)
        //#   /* At what inflight value did losses cross BBRLossThresh? */
        //#   inflight = inflight_prev + lost_prefix
        //#   return inflight

        // The RFC passes a newly construct Rate Sample to BBRInflightHiFromLostPacket as
        // a means for holding tx_in_flight and lost_since_transmit. Instead, we pass
        // the required information directly.

        // What was in flight before this packet?
        // Note: The TCP BBRv2 impl treats a negative inflight_prev as an error case
        // see https://github.com/aws/s2n-quic/issues/1456
        let inflight_prev = packet_info.bytes_in_flight.saturating_sub(size);
        // What was lost before this packet?
        let lost_prev = lost_since_transmit - size;
        // BBRLossThresh * inflight_prev - lost_prev
        let loss_budget = (LOSS_THRESH * inflight_prev)
            .to_integer()
            .saturating_sub(lost_prev);
        // Multiply by the inverse of 1 - LOSS_THRESH instead of dividing
        let lost_prefix = ((Ratio::one() - LOSS_THRESH).inv() * loss_budget).to_integer();
        // At what inflight value did losses cross BBRLossThresh?
        inflight_prev + lost_prefix
    }

    /// Handles when the connection resumes transmitting after an idle period
    #[inline]
    fn handle_restart_from_idle<Pub: Publisher>(&mut self, now: Timestamp, publisher: &mut Pub) {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.4.3
        //# BBRHandleRestartFromIdle():
        //#   if (packets_in_flight == 0 and C.app_limited)
        //#     BBR.idle_restart = true
        //#        BBR.extra_acked_interval_start = Now()
        //#     if (IsInAProbeBWState())
        //#       BBRSetPacingRateWithGain(1)

        if self.bytes_in_flight == 0 && self.bw_estimator.is_app_limited() {
            self.idle_restart = true;
            self.data_volume_model.set_extra_acked_interval_start(now);
            if self.state.is_probing_bw() {
                self.pacer.set_pacing_rate(
                    self.data_rate_model.bw(),
                    Ratio::one(),
                    self.full_pipe_estimator.filled_pipe(),
                    publisher,
                );
            }
        }

        // As an optimization, we can check if the ProbeRtt may be exited here, see #1412 for details.
        // Without this optimization, ProbeRtt will be exited on the next received Ack.

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.4.3
        //= type=TODO
        //= tracking-issue=1412
        //#   else if (BBR.state == ProbeRTT)
        //#     BBRCheckProbeRTTDone()

        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.4.2
        //= type=TODO
        //= tracking-issue=1412
        //# As an optimization, when restarting from idle BBR checks to see if the connection is in
        //# ProbeRTT and has met the exit conditions for ProbeRTT. If a connection goes idle during
        //# ProbeRTT then often it will have met those exit conditions by the time it restarts, so
        //# that the connection can restore the cwnd to its full value before it starts transmitting
        //# a new flight of data.
    }

    /// Determines if the BBR model does not need to be updated
    ///
    /// Based on `bbr2_fast_path` in the Linux TCP BBRv2.
    /// See https://github.com/google/bbr/blob/1a45fd4faf30229a3d3116de7bfe9d2f933d3562/net/ipv4/tcp_bbr2.c#L2208
    #[inline]
    fn model_update_required(&self) -> bool {
        let rate_sample = self.bw_estimator.rate_sample();

        // We can skip updating the model when app limited and there is no congestion,
        // and the bandwidth sample is less than the estimated maximum bandwidth
        !self.try_fast_path
            || !rate_sample.is_app_limited
            || rate_sample.delivery_rate() >= self.data_rate_model.max_bw()
            || self.congestion_state.loss_in_round()
            || self.congestion_state.ecn_in_round()
    }

    /// Determines if the BBR control parameters do not need to be updated
    #[inline]
    fn control_update_required(&self, model_updated: bool, prev_min_rtt: Option<Duration>) -> bool {
        // We can skip updating the control parameters if we had skipped updating the model
        // and the BBR state and min rtt did not change. `try_fast_path` is set to false
        // when the BBR state is changed.
        !self.try_fast_path || model_updated || prev_min_rtt != self.data_volume_model.min_rtt()
    }
}

#[non_exhaustive]
#[derive(Debug, Default)]
pub struct Endpoint {}

impl congestion_controller::Endpoint for Endpoint {
    type CongestionController = BbrCongestionController;

    fn new_congestion_controller(
        &mut self,
        path_info: congestion_controller::PathInfo,
    ) -> Self::CongestionController {
        BbrCongestionController::new(path_info.max_datagram_size)
    }
}

#[cfg(test)]
mod tests;
