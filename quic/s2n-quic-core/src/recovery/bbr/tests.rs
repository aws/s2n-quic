// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    assert_delta,
    path::MINIMUM_MTU,
    random,
    recovery::{
        bandwidth::{Bandwidth, PacketInfo, RateSample},
        bbr,
        bbr::{probe_rtt, BbrCongestionController, State},
    },
    time::{Clock, NoopClock},
};
use num_rational::Ratio;
use num_traits::{Inv, One, ToPrimitive};
use std::time::Duration;

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

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.2.1
//= type=test
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
#[test]
fn new() {
    let bbr = BbrCongestionController::new(MINIMUM_MTU);

    assert_eq!(Bandwidth::ZERO, bbr.data_rate_model.max_bw());
    assert_eq!(None, bbr.data_volume_model.min_rtt());
    assert_eq!(0, bbr.prior_cwnd);
    assert!(!bbr.idle_restart);
    assert_eq!(0, bbr.data_volume_model.extra_acked());

    // BBRResetCongestionSignals()
    bbr::congestion::testing::assert_reset(bbr.congestion_state);

    // BBRResetLowerBounds()
    assert_eq!(u64::MAX, bbr.data_volume_model.inflight_lo());
    assert_eq!(Bandwidth::INFINITY, bbr.data_rate_model.bw_lo());

    // BBRInitRoundCounting()
    assert!(!bbr.round_counter.round_start());
    assert_eq!(0, bbr.round_counter.round_count());

    // BBRInitFullPipe()
    assert!(!bbr.full_pipe_estimator.filled_pipe());

    // BBRInitPacingRate():
    //     nominal_bandwidth = InitialCwnd / (SRTT ? SRTT : 1ms)
    //     BBR.pacing_rate =  BBRStartupPacingGain * nominal_bandwidth
    // nominal_bandwidth = 12_000 / 1ms = ~83nanos/byte
    // pacing_rate = 2.77 * 83nanos/byte = ~29nanos/byte

    assert_eq!(Bandwidth::new(1, Duration::from_nanos(29)), bbr.pacing_rate);

    // BBREnterStartup()
    assert!(bbr.state.is_startup());
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
//= type=test
//# BBRBDPMultiple(gain):
//#   if (BBR.min_rtt == Inf)
//#       return InitialCwnd /* no valid RTT samples yet */
//#     BBR.bdp = BBR.bw * BBR.min_rtt
//#     return gain * BBR.bdp
#[test]
fn bdp_multiple() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);
    let now = NoopClock.get_time();

    // No min_rtt yet, so bdp is the initial window
    assert_eq!(12_000, bbr.bdp_multiple(Bandwidth::ZERO, Ratio::one()));

    // Set an RTT so min_rtt is populated
    let rtt = Duration::from_millis(100);
    bbr.data_volume_model.update_min_rtt(rtt, now);
    assert_eq!(Some(rtt), bbr.data_volume_model.min_rtt());

    let bandwidth = Bandwidth::new(1000, Duration::from_millis(1));
    let gain = Ratio::new(2, 1);

    // bdp_multiple = bandwidth * min_rtt * gain = 1000bytes/ms * 100ms * 2 = 200000 bytes
    assert_eq!(200000, bbr.bdp_multiple(bandwidth, gain));

    // Infinite bandwidth should not panic
    assert_eq!(u64::MAX, bbr.bdp_multiple(Bandwidth::INFINITY, gain));

    // Zero bandwidth should not panic
    assert_eq!(0, bbr.bdp_multiple(Bandwidth::ZERO, gain));
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.5.3
//# BBRTargetInflight()
//#   return min(BBR.bdp, cwnd)
#[test]
fn target_inflight() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);
    let now = NoopClock.get_time();

    let rate_sample = RateSample {
        delivered_bytes: 1000,
        interval: Duration::from_millis(1),
        ..Default::default()
    };
    bbr.data_rate_model.update_max_bw(rate_sample);
    bbr.data_rate_model.bound_bw_for_model();

    // Set an RTT so min_rtt is populated
    let rtt = Duration::from_millis(100);
    bbr.data_volume_model.update_min_rtt(rtt, now);
    assert_eq!(Some(rtt), bbr.data_volume_model.min_rtt());

    // bdp = 100ms * 1000bytes/ms = 100000bytes
    // cwnd = 12000
    // bdp < cwnd, so cwnd is returned
    assert_eq!(12000, bbr.target_inflight());

    bbr.cwnd = 200000;

    // bdp > cwnd, so bdp is returned
    assert_eq!(100000, bbr.target_inflight());
}
