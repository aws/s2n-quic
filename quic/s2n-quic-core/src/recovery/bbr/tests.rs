// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    assert_delta,
    path::MINIMUM_MTU,
    random,
    recovery::{
        bandwidth::PacketInfo,
        bbr::{probe_rtt, BbrCongestionController, State},
    },
    time::{Clock, NoopClock},
};
use num_traits::{Inv, ToPrimitive};

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.2
//= type=test
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
#[test]
fn inflight_hi_from_lost_packet() {
    let now = NoopClock.get_time();
    let packet_info = PacketInfo {
        delivered_bytes: 0,
        delivered_time: now,
        lost_bytes: 0,
        ecn_ce_count: 0,
        first_sent_time: now,
        bytes_in_flight: 3000,
        is_app_limited: false,
    };

    let inflight_prev = packet_info.bytes_in_flight - MINIMUM_MTU as u32;
    assert_eq!(inflight_prev, 1800);

    // lost prefix = (BBRLossThresh * inflight_prev - lost_prev) / (1 - BBRLossThresh)
    // lost prefix = (1/50 * 1800 - (1210 - 1200)) / (1 - 1/50) = ~26
    assert_eq!(
        inflight_prev + 26,
        BbrCongestionController::inflight_hi_from_lost_packet(
            MINIMUM_MTU as u32,
            1210,
            packet_info
        )
    );

    // lost prefix is zero since LOSS_THRESH * 1800 < 3000 - 1200
    assert_eq!(
        inflight_prev,
        BbrCongestionController::inflight_hi_from_lost_packet(
            MINIMUM_MTU as u32,
            3000,
            packet_info
        )
    );

    // Test losing the first sent packet when nothing is inflight yet
    let packet_info = PacketInfo {
        delivered_bytes: 0,
        delivered_time: now,
        lost_bytes: 0,
        ecn_ce_count: 0,
        first_sent_time: now,
        bytes_in_flight: 0,
        is_app_limited: false,
    };

    // inflight_prev = 0 since bytes_in_flight when the lost packet was sent was 0
    let inflight_prev = 0;

    // lost prefix is zero since inflight_prev = 0
    assert_eq!(
        inflight_prev,
        BbrCongestionController::inflight_hi_from_lost_packet(
            MINIMUM_MTU as u32,
            MINIMUM_MTU as u32,
            packet_info
        )
    );
}

#[test]
fn pacing_cwnd_gain() {
    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.6
    //= type=test
    //# A constant specifying the minimum gain value for calculating the pacing rate that will
    //# allow the sending rate to double each round (4*ln(2) ~= 2.77)
    assert_delta!(State::Startup.pacing_gain().to_f32().unwrap(), 2.77, 0.001);

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.6
    //= type=test
    //# A constant specifying the minimum gain value for calculating the
    //# cwnd that will allow the sending rate to double each round (2.0)
    assert_delta!(State::Startup.cwnd_gain().to_f32().unwrap(), 2.0, 0.001);

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.2
    //= type=test
    //# In Drain, BBR aims to quickly drain any queue created in Startup by switching to a
    //# pacing_gain well below 1.0, until any estimated queue has been drained. It uses a
    //# pacing_gain that is the inverse of the value used during Startup, chosen to try to
    //# drain the queue in one round

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.2
    //= type=test
    //# BBREnterDrain():
    //#     BBR.state = Drain
    //#     BBR.pacing_gain = 1/BBRStartupCwndGain  /* pace slowly */
    //#     BBR.cwnd_gain = BBRStartupCwndGain      /* maintain cwnd */
    assert_eq!(State::Drain.pacing_gain(), State::Startup.cwnd_gain().inv());
    assert_eq!(State::Drain.cwnd_gain(), State::Startup.cwnd_gain());

    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);
    let now = NoopClock.get_time();
    bbr.enter_drain();
    bbr.enter_probe_bw(false, &mut random::testing::Generator::default(), now);
    assert!(bbr.state.is_probing_bw());

    // ProbeBw cwnd gain from https://www.ietf.org/archive/id/draft-cardwell-iccrg-bbr-congestion-control-02.html#section-4.6.1
    assert_delta!(bbr.state.cwnd_gain().to_f32().unwrap(), 2.0, 0.001);

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.1
    //= type=test
    //# In the ProbeBW_DOWN phase of the cycle, a BBR flow pursues the deceleration tactic,
    //# to try to send slower than the network is delivering data, to reduce the amount of data
    //# in flight, with all of the standard motivations for the deceleration tactic (discussed
    //# in "State Machine Tactics", above). It does this by switching to a BBR.pacing_gain of
    //# 0.9, sending at 90% of BBR.bw.
    assert_delta!(bbr.state.pacing_gain().to_f32().unwrap(), 0.9, 0.001);

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.4.4
    //= type=test
    //# BBREnterProbeRTT():
    //#     BBR.state = ProbeRTT
    //#     BBR.pacing_gain = 1
    assert_delta!(
        State::ProbeRtt(probe_rtt::State::default())
            .pacing_gain()
            .to_f32()
            .unwrap(),
        1.0,
        0.001
    );

    //= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#2.14.2
    //= type=test
    //# A constant specifying the gain value for calculating the cwnd during ProbeRTT: 0.5
    assert_delta!(
        State::ProbeRtt(probe_rtt::State::default())
            .cwnd_gain()
            .to_f32()
            .unwrap(),
        0.5,
        0.001
    );
}
