// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::Counter,
    random,
    recovery::{
        bandwidth, bandwidth::Bandwidth, bbr::probe_bw::CyclePhase, CongestionController,
        RttEstimator,
    },
    time::Timestamp,
};
use core::{
    cmp::{max, min},
    convert::TryInto,
};
use num_rational::Ratio;
use num_traits::One;

mod congestion;
mod data_rate;
mod data_volume;
mod full_pipe;
mod probe_bw;
mod recovery;
mod round;
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

/// A congestion controller that implements "Bottleneck Bandwidth and Round-trip propagation time"
/// version 2 (BBRv2) as specified in <https://datatracker.ietf.org/doc/draft-cardwell-iccrg-bbr-congestion-control/>.
///
/// Based in part on the Chromium BBRv2 implementation, see <https://source.chromium.org/chromium/chromium/src/+/main:net/third_party/quiche/src/quic/core/congestion_control/bbr2_sender.cc>
/// and the Linux Kernel TCP BBRv2 implementation, see <https://github.com/google/bbr/blob/v2alpha/net/ipv4/tcp_bbr2.c>
#[derive(Debug, Clone)]
struct BbrCongestionController {
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
    recovery_state: recovery::State,
    congestion_state: congestion::State,
    probe_bw_state: probe_bw::State,
    data_rate_model: data_rate::Model,
    data_volume_model: data_volume::Model,
    max_datagram_size: u16,
}

type BytesInFlight = Counter<u32>;

impl CongestionController for BbrCongestionController {
    type PacketInfo = bandwidth::PacketInfo;

    fn congestion_window(&self) -> u32 {
        self.cwnd
    }

    fn bytes_in_flight(&self) -> u32 {
        *self.bytes_in_flight
    }

    fn is_congestion_limited(&self) -> bool {
        todo!()
    }

    fn requires_fast_retransmission(&self) -> bool {
        self.recovery_state.requires_fast_retransmission()
    }

    fn on_packet_sent(
        &mut self,
        time_sent: Timestamp,
        sent_bytes: usize,
        _rtt_estimator: &RttEstimator,
    ) -> Self::PacketInfo {
        let is_app_limited = false; // TODO: determine if app limited
        let packet_info =
            self.bw_estimator
                .on_packet_sent(*self.bytes_in_flight, is_app_limited, time_sent);

        if sent_bytes > 0 {
            self.recovery_state.on_packet_sent();

            self.bytes_in_flight
                .try_add(sent_bytes)
                .expect("sent_bytes should not exceed u32::MAX");
        }

        packet_info
    }

    fn on_rtt_update(&mut self, _time_sent: Timestamp, _rtt_estimator: &RttEstimator) {
        todo!()
    }

    fn on_ack<Rnd: random::Generator>(
        &mut self,
        newest_acked_time_sent: Timestamp,
        bytes_acknowledged: usize,
        newest_acked_packet_info: Self::PacketInfo,
        _rtt_estimator: &RttEstimator,
        random_generator: &mut Rnd,
        ack_receive_time: Timestamp,
    ) {
        self.bw_estimator.on_ack(
            bytes_acknowledged,
            newest_acked_time_sent,
            newest_acked_packet_info,
            ack_receive_time,
        );
        self.round_counter.on_ack(
            newest_acked_packet_info,
            self.bw_estimator.delivered_bytes(),
        );
        self.recovery_state
            .on_ack(self.round_counter.round_start(), newest_acked_time_sent);
        let is_probing_bw = false; // TODO: track probing state
        self.congestion_state.update(
            newest_acked_packet_info,
            self.bw_estimator.rate_sample(),
            self.bw_estimator.delivered_bytes(),
            &mut self.data_rate_model,
            &mut self.data_volume_model,
            is_probing_bw,
            self.cwnd,
        );

        if self.round_counter.round_start() {
            self.full_pipe_estimator.on_round_start(
                self.bw_estimator.rate_sample(),
                self.data_rate_model.max_bw(),
                self.recovery_state.in_recovery(),
            );
            self.probe_bw_state.on_round_start();
        }

        if self.full_pipe_estimator.filled_pipe() {
            self.adapt_upper_bounds(
                self.bw_estimator.rate_sample(),
                bytes_acknowledged,
                random_generator,
                ack_receive_time,
            );
            // TODO: if self.state == State::Probe_BW
            self.update_probe_bw_cycle_phase(random_generator, ack_receive_time);
        }
        self.congestion_state
            .advance(self.bw_estimator.rate_sample());
    }

    fn on_packet_lost<Rnd: random::Generator>(
        &mut self,
        lost_bytes: u32,
        _packet_info: Self::PacketInfo,
        _persistent_congestion: bool,
        new_loss_burst: bool,
        _random_generator: &mut Rnd,
        timestamp: Timestamp,
    ) {
        self.bw_estimator.on_loss(lost_bytes as usize);
        self.recovery_state.on_congestion_event(timestamp);
        self.full_pipe_estimator.on_packet_lost(new_loss_burst);
    }

    fn on_congestion_event(&mut self, event_time: Timestamp) {
        self.recovery_state.on_congestion_event(event_time);
    }

    fn on_mtu_update(&mut self, max_datagram_size: u16) {
        self.max_datagram_size = max_datagram_size;
    }

    fn on_packet_discarded(&mut self, _bytes_sent: usize) {
        todo!()
    }

    fn earliest_departure_time(&self) -> Option<Timestamp> {
        todo!()
    }
}

impl BbrCongestionController {
    /// The bandwidth-delay product
    ///
    /// Based on the current estimate of maximum sending bandwidth and minimum RTT
    fn bdp(&self) -> u64 {
        self.bdp_multiple(self.data_rate_model.bw(), Ratio::one())
    }

    /// Calculates a bandwidth-delay product using the supplied `Bandwidth` and `gain`
    fn bdp_multiple(&self, bw: Bandwidth, gain: Ratio<u64>) -> u64 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
        //# BBRBDPMultiple()

        if let Some(min_rtt) = self.data_volume_model.min_rtt() {
            (gain * (bw * min_rtt)).to_integer()
        } else {
            Self::initial_window(self.max_datagram_size).into()
        }
    }

    /// How much data do we want in flight
    ///
    /// Based on the estimated BDP, unless congestion reduced the cwnd
    fn target_inflight(&self) -> u32 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.5.3
        //# BBRTargetInflight()

        self.bdp().min(self.cwnd as u64) as u32
    }

    /// The estimate of the volume of in-flight data required to fully utilize the bottleneck
    /// bandwidth available to the flow
    ///
    /// Based on the BDP estimate (BBR.bdp), the aggregation estimate (BBR.extra_acked), the
    /// offload budget (BBR.offload_budget), and BBRMinPipeCwnd.
    #[allow(dead_code)] // TODO: Remove when used
    fn max_inflight(&self) -> u64 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
        //# BBRUpdateMaxInflight()

        let cwnd_gain = Ratio::one(); // TODO: get cwnd_gain from State
        let bdp = self.bdp_multiple(self.data_rate_model.bw(), cwnd_gain);
        let inflight = bdp + self.data_volume_model.extra_acked();
        self.quantization_budget(inflight)
    }

    /// Inflight based on min RTT and the estimated bottleneck bandwidth
    fn inflight(&self, bw: Bandwidth, gain: Ratio<u64>) -> u32 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
        //# BBRInflight(gain)

        let inflight = self.bdp_multiple(bw, gain);
        self.quantization_budget(inflight)
            .try_into()
            .unwrap_or(u32::MAX)
    }

    /// The volume of data that tries to leave free headroom in the bottleneck buffer or link for
    /// other flows, for fairness convergence and lower RTTs and loss
    fn inflight_with_headroom(&self) -> u32 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
        //# BBRInflightWithHeadroom()

        if self.data_volume_model.inflight_hi() == u64::MAX {
            return u32::MAX;
        }

        let headroom = max(
            1,
            (HEADROOM * self.data_volume_model.inflight_hi()).to_integer(),
        );
        max(
            self.data_volume_model.inflight_hi() - headroom,
            self.minimum_window() as u64,
        )
        .try_into()
        .unwrap_or(u32::MAX) // TODO: change type
    }

    /// Calculates the quantization budget
    fn quantization_budget(&self, inflight: u64) -> u64 {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
        //# BBRQuantizationBudget(inflight)
        //#   BBRUpdateOffloadBudget()
        //#   inflight = max(inflight, BBR.offload_budget)
        //#   inflight = max(inflight, BBRMinPipeCwnd)
        //#   if (BBR.state == ProbeBW && BBR.cycle_idx == ProbeBW_UP)
        //#     inflight += 2
        //#   return inflight

        let offload_budget = 1; // TODO: Track offload budget

        let mut inflight = inflight
            .max(offload_budget)
            .max(self.minimum_window() as u64);

        if self.probe_bw_state.cycle_phase() == CyclePhase::Up {
            // TODO: Check BBR.state == ProbeBW
            inflight += 2 * self.max_datagram_size as u64;
        }

        inflight
    }

    /// True if the amount of `lost_bytes` exceeds the BBR loss threshold
    fn is_inflight_too_high(lost_bytes: u64, bytes_inflight: u32) -> bool {
        //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.2
        //# IsInflightTooHigh()

        lost_bytes > (LOSS_THRESH * bytes_inflight).to_integer() as u64
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
}
