// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::Counter,
    number::Fraction,
    recovery::{bandwidth, bandwidth::Bandwidth, CongestionController, RttEstimator},
    time::Timestamp,
};

mod full_pipe;
mod recovery;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.8
//# The maximum tolerated per-round-trip packet loss rate when probing for bandwidth (the default is 2%).
const LOSS_THRESH: Fraction = Fraction::new(1, 50);

/// A congestion controller that implements "Bottleneck Bandwidth and Round-trip propagation time"
/// version 2 (BBRv2) as specified in <https://datatracker.ietf.org/doc/draft-cardwell-iccrg-bbr-congestion-control/>.
///
/// Based in part on the Chromium BBRv2 implementation, see <https://source.chromium.org/chromium/chromium/src/+/main:net/third_party/quiche/src/quic/core/congestion_control/bbr2_sender.cc>
/// and the Linux Kernel TCP BBRv2 implementation, see <https://github.com/google/bbr/blob/v2alpha/net/ipv4/tcp_bbr2.c>
#[derive(Debug, Clone)]
struct BbrCongestionController {
    bw_estimator: bandwidth::Estimator,
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.9.1
    //# The windowed maximum recent bandwidth sample - obtained using the BBR delivery rate sampling
    //# algorithm [draft-cheng-iccrg-delivery-rate-estimation] - measured during the current or
    //# previous bandwidth probing cycle (or during Startup, if the flow is still in that state).
    max_bw: Bandwidth,
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
    recovery_state: recovery::State,
}

type BytesInFlight = Counter<u32>;

impl CongestionController for BbrCongestionController {
    type PacketInfo = bandwidth::PacketInfo;

    fn congestion_window(&self) -> u32 {
        todo!()
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

    fn on_ack(
        &mut self,
        newest_acked_time_sent: Timestamp,
        bytes_acknowledged: usize,
        newest_acked_packet_info: Self::PacketInfo,
        _rtt_estimator: &RttEstimator,
        ack_receive_time: Timestamp,
    ) {
        self.bw_estimator.on_ack(
            bytes_acknowledged,
            newest_acked_time_sent,
            newest_acked_packet_info,
            ack_receive_time,
        );
        let round_start = false; // TODO: track rounds
        self.recovery_state
            .on_ack(round_start, newest_acked_time_sent);

        if round_start {
            self.full_pipe_estimator.on_round_start(
                self.bw_estimator.rate_sample(),
                self.max_bw,
                self.recovery_state.in_recovery(),
            )
        }
    }

    fn on_packet_lost(
        &mut self,
        lost_bytes: u32,
        _packet_info: Self::PacketInfo,
        _persistent_congestion: bool,
        new_loss_burst: bool,
        timestamp: Timestamp,
    ) {
        self.bw_estimator.on_loss(lost_bytes as usize);
        self.recovery_state.on_congestion_event(timestamp);
        self.full_pipe_estimator.on_packet_lost(new_loss_burst);
    }

    fn on_congestion_event(&mut self, event_time: Timestamp) {
        self.recovery_state.on_congestion_event(event_time);
    }

    fn on_mtu_update(&mut self, _max_data_size: u16) {
        todo!()
    }

    fn on_packet_discarded(&mut self, _bytes_sent: usize) {
        todo!()
    }

    fn earliest_departure_time(&self) -> Option<Timestamp> {
        todo!()
    }
}
