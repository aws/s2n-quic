// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    assert_delta,
    counter::Counter,
    event, path,
    path::MINIMUM_MAX_DATAGRAM_SIZE,
    random,
    recovery::{
        bandwidth::{Bandwidth, PacketInfo, RateSample},
        bbr,
        bbr::{probe_bw::CyclePhase, probe_rtt, BbrCongestionController, State},
        congestion_controller::{PathPublisher, Publisher},
        CongestionController,
    },
    time::{Clock, NoopClock},
};
use num_rational::Ratio;
use num_traits::{Inv, One, ToPrimitive};
use std::time::Duration;

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.3
//= type=test
//# When not explicitly accelerating to probe for bandwidth (Drain, ProbeRTT,
//# ProbeBW_DOWN, ProbeBW_CRUISE), BBR responds to loss by slowing down to some extent.
#[test]
fn is_probing_for_bandwidth() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

    // States that are not explicitly accelerating to probe for bandwidth
    // ie Drain, ProbeRtt, ProbeBW_DOWN, ProbeBW_CRUISE
    assert!(!State::Drain.is_probing_for_bandwidth());
    assert!(!State::ProbeRtt(probe_rtt::State::default()).is_probing_for_bandwidth());

    enter_probe_bw_state(&mut bbr, CyclePhase::Down, &mut publisher);
    assert!(!bbr.state.is_probing_for_bandwidth());

    enter_probe_bw_state(&mut bbr, CyclePhase::Cruise, &mut publisher);
    assert!(!bbr.state.is_probing_for_bandwidth());

    // States that are explicitly accelerating to probe for bandwidth
    assert!(State::Startup.is_probing_for_bandwidth());

    enter_probe_bw_state(&mut bbr, CyclePhase::Up, &mut publisher);
    assert!(bbr.state.is_probing_for_bandwidth());

    enter_probe_bw_state(&mut bbr, CyclePhase::Refill, &mut publisher);
    assert!(bbr.state.is_probing_for_bandwidth());
}

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

    let inflight_prev = packet_info.bytes_in_flight - MINIMUM_MAX_DATAGRAM_SIZE as u32;
    assert_eq!(inflight_prev, 1800);

    // lost prefix = (BBRLossThresh * inflight_prev - lost_prev) / (1 - BBRLossThresh)
    // lost prefix = (1/50 * 1800 - (1210 - 1200)) / (1 - 1/50) = ~26
    assert_eq!(
        inflight_prev + 26,
        BbrCongestionController::inflight_hi_from_lost_packet(
            MINIMUM_MAX_DATAGRAM_SIZE as u32,
            1210,
            packet_info
        )
    );

    // lost prefix is zero since LOSS_THRESH * 1800 < 3000 - 1200
    assert_eq!(
        inflight_prev,
        BbrCongestionController::inflight_hi_from_lost_packet(
            MINIMUM_MAX_DATAGRAM_SIZE as u32,
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
            MINIMUM_MAX_DATAGRAM_SIZE as u32,
            MINIMUM_MAX_DATAGRAM_SIZE as u32,
            packet_info
        )
    );
}

#[test]
fn pacing_cwnd_gain() {
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

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

    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    let now = NoopClock.get_time();
    bbr.enter_drain(&mut publisher);
    bbr.enter_probe_bw(
        false,
        &mut random::testing::Generator::default(),
        now,
        &mut publisher,
    );
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
    let bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);

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
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
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
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
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

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
//= type=test
//# BBRUpdateMaxInflight()
//#   BBRUpdateAggregationBudget()
//#   inflight = BBRBDPMultiple(BBR.cwnd_gain)
//#   inflight += BBR.extra_acked
//#   BBR.max_inflight = BBRQuantizationBudget(inflight)
#[test]
fn max_inflight() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);

    bbr.data_volume_model.set_extra_acked_for_test(1000, 0);
    // bdp = initial_window = 12000 since min_rtt is not populated
    // inflight = bdp + extra_acked = 13000
    // max_inflight = quantization_budget(13000) = 3 * MAX_BURST_PACKETS * MINIMUM_MAX_DATAGRAM_SIZE = 36000
    assert_eq!(36000, bbr.max_inflight());
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
//= type=test
//# BBRInflight(gain)
//#   inflight = BBRBDPMultiple(gain)
//#   return BBRQuantizationBudget(inflight)
#[test]
fn inflight() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    let now = NoopClock.get_time();

    // Set an RTT so min_rtt is populated
    let rtt = Duration::from_millis(100);
    bbr.data_volume_model.update_min_rtt(rtt, now);
    assert_eq!(Some(rtt), bbr.data_volume_model.min_rtt());

    let bandwidth = Bandwidth::new(2000, Duration::from_millis(1));
    // bdp = 2000bytes/ms * 100ms = 200000bytes
    // max_inflight = quantization_budget(200000) = 200000
    assert_eq!(200000, bbr.inflight(bandwidth, Ratio::one()));

    // Infinite bandwidth should not panic
    assert_eq!(
        u32::MAX,
        bbr.inflight(Bandwidth::INFINITY, Ratio::new(2, 1))
    );
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.3.3.6
//= type=test
//# BBRInflightWithHeadroom()
//#   if (BBR.inflight_hi == Infinity)
//#     return Infinity
//#   headroom = max(1, BBRHeadroom * BBR.inflight_hi)
//#     return max(BBR.inflight_hi - headroom,
//#                BBRMinPipeCwnd)
#[test]
fn inflight_with_headroom() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);

    // inflight_hi has not been initialized so inflight is u32::MAX
    assert_eq!(u32::MAX, bbr.inflight_with_headroom());

    bbr.data_volume_model.update_upper_bound(10_000);

    // inflight_with_headroom = HEADROOM * inflight_hi = .85 * 10_0000 = 8500
    assert_eq!(8500, bbr.inflight_with_headroom());

    // Set a lower inflight_hi so that the minimum window is larger
    bbr.data_volume_model.update_upper_bound(1000);

    // inflight_with_headroom = HEADROOM * inflight_hi = .85 * 1000 = 850
    // inflight_with_headroom < minimum_window, return minimum_window
    assert_eq!(4800, bbr.inflight_with_headroom());
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
//= type=test
//# BBRQuantizationBudget(inflight)
//#   BBRUpdateOffloadBudget()
//#   inflight = max(inflight, BBR.offload_budget)
//#   inflight = max(inflight, BBRMinPipeCwnd)
//#   if (BBR.state == ProbeBW && BBR.cycle_idx == ProbeBW_UP)
//#     inflight += 2
//#   return inflight
#[test]
fn quantization_budget() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
    bbr.pacer.set_send_quantum_for_test(4000);

    let send_quantum = bbr.pacer.send_quantum();

    // offload_budget = 3 * send_quantum

    // offload_budget > inflight > minimum_window, return offload_budget
    assert_eq!(3 * send_quantum as u64, bbr.quantization_budget(6000));

    // offload_budget < inflight, return inflight
    assert_eq!(14000, bbr.quantization_budget(14000));

    bbr.pacer.set_send_quantum_for_test(1000);
    // offload_budget = 3 * send_quantum = 3000

    // minimum_window = 4 * mtu = 4800
    // offload_budget < inflight < minimum_window
    assert_eq!(4800, bbr.quantization_budget(2000));

    enter_probe_bw_state(&mut bbr, CyclePhase::Up, &mut publisher);
    assert!(bbr.state.is_probing_bw_up());

    // since probe bw up, add 2 packets to the budget
    assert_eq!(
        4800 + 2 * MINIMUM_MAX_DATAGRAM_SIZE as u64,
        bbr.quantization_budget(2000)
    );
}

#[test]
fn is_inflight_too_high() {
    let rate_sample = RateSample {
        lost_bytes: 3,
        bytes_in_flight: 100,
        ..Default::default()
    };
    // loss rate higher than 2% threshold and loss bursts = limit
    assert!(BbrCongestionController::is_inflight_too_high(
        rate_sample,
        MINIMUM_MAX_DATAGRAM_SIZE,
        2,
        2
    ));

    // loss rate higher than 2% threshold but loss bursts < limit
    assert!(!BbrCongestionController::is_inflight_too_high(
        rate_sample,
        MINIMUM_MAX_DATAGRAM_SIZE,
        1,
        2
    ));

    let rate_sample = RateSample {
        lost_bytes: 2,
        bytes_in_flight: 100,
        ..Default::default()
    };
    // loss rate <= 2% threshold
    assert!(!BbrCongestionController::is_inflight_too_high(
        rate_sample,
        MINIMUM_MAX_DATAGRAM_SIZE,
        2,
        2
    ));

    let rate_sample = RateSample {
        delivered_bytes: 100 * MINIMUM_MAX_DATAGRAM_SIZE as u64,
        ecn_ce_count: 51,
        ..Default::default()
    };
    // ecn rate higher than 50% threshold
    assert!(BbrCongestionController::is_inflight_too_high(
        rate_sample,
        MINIMUM_MAX_DATAGRAM_SIZE,
        0,
        2
    ));

    let rate_sample = RateSample {
        delivered_bytes: 100 * MINIMUM_MAX_DATAGRAM_SIZE as u64,
        ecn_ce_count: 50,
        ..Default::default()
    };
    // ecn rate <= 50% threshold
    assert!(!BbrCongestionController::is_inflight_too_high(
        rate_sample,
        MINIMUM_MAX_DATAGRAM_SIZE,
        0,
        2,
    ));
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.7
//= type=test
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
#[test]
fn bound_cwnd_for_model() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
    enter_probe_bw_state(&mut bbr, CyclePhase::Down, &mut publisher);

    bbr.data_volume_model.update_upper_bound(10000);

    assert_eq!(10000, bbr.bound_cwnd_for_model());

    enter_probe_bw_state(&mut bbr, CyclePhase::Cruise, &mut publisher);

    // inflight_with_headroom = .85 * 10000 = 8500
    assert_eq!(8500, bbr.bound_cwnd_for_model());

    bbr.state = State::ProbeRtt(probe_rtt::State::default());
    // inflight_with_headroom = .85 * 10000 = 8500
    assert_eq!(8500, bbr.bound_cwnd_for_model());

    // now the limiting factor is inflight_lo
    bbr.state = State::Startup;
    // Set inflight_lo to 5000
    bbr.data_volume_model
        .update_lower_bound(1000, 5000, true, false, 0.0);
    assert_eq!(5000, bbr.data_volume_model.inflight_lo());

    assert_eq!(5000, bbr.bound_cwnd_for_model());

    // now the limiting factor is minimum window (4800)
    // Set inflight_lo to 4000
    bbr.data_volume_model
        .update_lower_bound(1000, 4000, true, false, 0.0);
    assert_eq!(4000, bbr.data_volume_model.inflight_lo());

    assert_eq!(4800, bbr.bound_cwnd_for_model());
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.6
//= type=test
//#     if (BBR.filled_pipe)
//#       cwnd = min(cwnd + rs.newly_acked, BBR.max_inflight)
#[test]
fn set_cwnd_filled_pipe() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    assert_eq!(36_000, bbr.max_inflight());

    bbr.full_pipe_estimator.set_filled_pipe_for_test(true);
    assert!(bbr.full_pipe_estimator.filled_pipe());

    bbr.cwnd = 12_000;
    bbr.set_cwnd(1000);
    assert_eq!(13_000, bbr.cwnd);
    assert!(!bbr.try_fast_path);

    bbr.cwnd = 40_000;
    bbr.set_cwnd(1000);
    assert_eq!(36_000, bbr.cwnd);
    assert!(bbr.try_fast_path);
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.6
//= type=test
//#     else if (cwnd < BBR.max_inflight || C.delivered < InitialCwnd)
//#       cwnd = cwnd + rs.newly_acked
#[test]
fn set_cwnd_not_filled_pipe() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
    let now = NoopClock.get_time();
    assert_eq!(36_000, bbr.max_inflight());

    assert!(!bbr.full_pipe_estimator.filled_pipe());

    bbr.cwnd = 12_000;
    bbr.set_cwnd(1000);
    assert_eq!(13_000, bbr.cwnd);

    // cwnd > BBR.max_inflight, but C.delivered < 2 * InitialCwnd
    bbr.cwnd = 40_000;
    bbr.set_cwnd(1000);
    assert_eq!(41_000, bbr.cwnd);

    // Set C.delivered > 2 * InitialCwnd
    let packet_info = PacketInfo {
        delivered_bytes: 0,
        delivered_time: now,
        lost_bytes: 0,
        ecn_ce_count: 0,
        first_sent_time: now,
        bytes_in_flight: 0,
        is_app_limited: false,
    };
    bbr.bw_estimator.on_ack(
        2 * BbrCongestionController::initial_window(MINIMUM_MAX_DATAGRAM_SIZE) as usize + 1,
        now,
        packet_info,
        now,
        &mut publisher,
    );
    bbr.cwnd = 12_000;
    bbr.set_cwnd(1000);
    assert_eq!(13_000, bbr.cwnd);
    assert!(!bbr.try_fast_path);

    // cwnd > BBR.max_inflight and C.delivered > InitialCwnd
    bbr.cwnd = 40_000;
    bbr.set_cwnd(1000);
    assert_eq!(40_000, bbr.cwnd); // No change
    assert!(bbr.try_fast_path);
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.5
//= type=test
//# BBRBoundCwndForProbeRTT():
//#   if (BBR.state == ProbeRTT)
//#     cwnd = min(cwnd, BBRProbeRTTCwnd())
#[test]
fn set_cwnd_probing_rtt() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    assert_eq!(36_000, bbr.max_inflight());
    assert_eq!(12_000, bbr.probe_rtt_cwnd());

    bbr.state = State::ProbeRtt(probe_rtt::State::default());

    // cwnd > probe_rtt_cwnd
    bbr.cwnd = 13_000;
    bbr.set_cwnd(1000);
    assert_eq!(12_000, bbr.cwnd);

    // cwnd < probe_rtt_cwnd. Since cwnd < max_flight, newly_acked is added
    bbr.cwnd = 10_000;
    bbr.set_cwnd(1000);
    assert_eq!(11_000, bbr.cwnd);
}

#[test]
fn set_cwnd_clamp() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    assert_eq!(36_000, bbr.max_inflight());

    // cwnd < min
    bbr.cwnd = bbr.minimum_window() - 1000;
    bbr.set_cwnd(500);
    assert_eq!(bbr.minimum_window(), bbr.cwnd);

    // cwnd > bound_cwnd_for_model
    bbr.data_volume_model.set_inflight_lo_for_test(30_000);
    assert_eq!(30_000, bbr.bound_cwnd_for_model());

    bbr.cwnd = 40_000;
    bbr.set_cwnd(1000);
    assert_eq!(30_000, bbr.cwnd);
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.4
//= type=test
//# BBRSaveCwnd()
//#   if (!InLossRecovery() and BBR.state != ProbeRTT)
//#     return cwnd
//#   else
//#     return max(BBR.prior_cwnd, cwnd)
#[test]
fn save_cwnd() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    bbr.state = State::ProbeRtt(probe_rtt::State::default());

    bbr.prior_cwnd = 2000;
    bbr.cwnd = 1000;
    bbr.save_cwnd();
    assert_eq!(2000, bbr.prior_cwnd);

    bbr.prior_cwnd = 4000;
    bbr.cwnd = 5000;
    bbr.save_cwnd();
    assert_eq!(5000, bbr.prior_cwnd);
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.4
//= type=test
//# BBRRestoreCwnd()
//#   cwnd = max(cwnd, BBR.prior_cwnd)
#[test]
fn restore_cwnd() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    bbr.state = State::ProbeRtt(probe_rtt::State::default());

    bbr.prior_cwnd = 1000;
    bbr.cwnd = 2000;

    bbr.restore_cwnd();

    assert_eq!(2000, bbr.cwnd);

    bbr.prior_cwnd = 2000;
    bbr.cwnd = 1000;

    bbr.restore_cwnd();

    assert_eq!(2000, bbr.cwnd);
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.4
//= type=test
//# Upon entering Fast Recovery, set cwnd to the number of packets still in flight
//# (allowing at least one for a fast retransmit):
//#
//# BBROnEnterFastRecovery():
//#   BBR.prior_cwnd = BBRSaveCwnd()
//#   cwnd = packets_in_flight + max(rs.newly_acked, 1)
//#   BBR.packet_conservation = true

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.5.6.2
//= type=test
//# if (!BBR.bw_probe_samples)
//#   return /* not a packet sent while probing bandwidth */
//# rs.tx_in_flight = packet.tx_in_flight /* inflight at transmit */
//# rs.lost = C.lost - packet.lost /* data lost since transmit */
//# rs.is_app_limited = packet.is_app_limited;
//# if (IsInflightTooHigh(rs))
//#   rs.tx_in_flight = BBRInflightHiFromLostPacket(rs, packet)
//#   BBRHandleInflightTooHigh(rs)
#[test]
fn handle_lost_packet() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    let now = NoopClock.get_time();
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
    bbr.bw_probe_samples = true;

    bbr.bw_estimator.on_loss(1000);

    let lost_packet = PacketInfo {
        delivered_bytes: 0,
        delivered_time: now,
        lost_bytes: 0,
        ecn_ce_count: 0,
        first_sent_time: now,
        bytes_in_flight: 10000,
        is_app_limited: false,
    };

    enter_probe_bw_state(&mut bbr, CyclePhase::Up, &mut publisher);
    // Two lost bursts to trigger inflight being too high
    bbr.congestion_state.on_packet_lost(500, true);
    bbr.congestion_state.on_packet_lost(500, true);
    bbr.handle_lost_packet(
        1000,
        lost_packet,
        &mut random::testing::Generator::default(),
        now,
        &mut publisher,
    );

    let inflight_hi_from_lost_packet =
        BbrCongestionController::inflight_hi_from_lost_packet(1000, 1000, lost_packet) as u64;

    // Only react once per bw probe
    assert!(!bbr.bw_probe_samples);

    // inflight_hi_from_lost_packet > BETA * target_inflight, so that becomes inflight_hi
    assert!(inflight_hi_from_lost_packet > (bbr::BETA * bbr.target_inflight() as u64).to_integer());
    assert_eq!(
        inflight_hi_from_lost_packet,
        bbr.data_volume_model.inflight_hi()
    );

    if let State::ProbeBw(probe_bw_state) = bbr.state {
        assert_eq!(CyclePhase::Down, probe_bw_state.cycle_phase());
    } else {
        panic!("Must be in ProbeBw Down state");
    }

    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    bbr.bw_estimator.on_loss(1000);

    // This time set cwnd and max_bw higher so that BETA * target_inflight is higher than inflight_hi_from_lost_packet
    bbr.bw_probe_samples = true;
    bbr.cwnd = 100_000;
    let rate_sample = RateSample {
        delivered_bytes: 100_000,
        interval: Duration::from_millis(1),
        ..Default::default()
    };
    bbr.data_volume_model
        .update_min_rtt(Duration::from_millis(10), now);
    bbr.data_rate_model.update_max_bw(rate_sample);
    bbr.data_rate_model.bound_bw_for_model();
    // Two lost bursts to trigger inflight being too high
    bbr.congestion_state.on_packet_lost(500, true);
    bbr.congestion_state.on_packet_lost(500, true);

    bbr.handle_lost_packet(
        1000,
        lost_packet,
        &mut random::testing::Generator::default(),
        now,
        &mut publisher,
    );

    let inflight_hi_from_lost_packet =
        BbrCongestionController::inflight_hi_from_lost_packet(1000, 1000, lost_packet) as u64;

    // Only react once per bw probe
    assert!(!bbr.bw_probe_samples);

    // inflight_hi_from_lost_packet < BETA * target_inflight, so that becomes inflight_hi
    assert!(inflight_hi_from_lost_packet < (bbr::BETA * bbr.target_inflight() as u64).to_integer());
    assert_eq!(
        (bbr::BETA * bbr.target_inflight() as u64).to_integer(),
        bbr.data_volume_model.inflight_hi()
    );
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.4.3
//= type=test
//# BBRHandleRestartFromIdle():
//#   if (packets_in_flight == 0 and C.app_limited)
//#     BBR.idle_restart = true
//#        BBR.extra_acked_interval_start = Now()
//#     if (IsInAProbeBWState())
//#       BBRSetPacingRateWithGain(1)
#[test]
fn handle_restart_from_idle() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());
    let now = NoopClock.get_time();
    let pacing_rate = bbr.pacer.pacing_rate();

    bbr.handle_restart_from_idle(now, &mut publisher);

    // Not app limited
    assert!(!bbr.idle_restart);

    // App limited, but bytes in flight > 0
    bbr.bytes_in_flight = Counter::new(1);
    bbr.bw_estimator.on_app_limited(100);
    bbr.handle_restart_from_idle(now, &mut publisher);

    assert!(!bbr.idle_restart);

    bbr.bytes_in_flight = Counter::default();

    bbr.handle_restart_from_idle(now, &mut publisher);
    assert!(bbr.idle_restart);
    assert_eq!(
        Some(now),
        bbr.data_volume_model.extra_acked_interval_start()
    );
    assert_eq!(pacing_rate, bbr.pacer.pacing_rate());

    enter_probe_bw_state(&mut bbr, CyclePhase::Down, &mut publisher);
    let rate_sample = RateSample {
        delivered_bytes: 100_000,
        interval: Duration::from_millis(1),
        ..Default::default()
    };
    bbr.data_rate_model.update_max_bw(rate_sample);
    bbr.data_rate_model.bound_bw_for_model();
    assert!(bbr.data_rate_model.bw() > bbr.pacer.pacing_rate());

    let now = now + Duration::from_secs(5);
    bbr.handle_restart_from_idle(now, &mut publisher);
    assert!(bbr.idle_restart);
    assert_eq!(
        Some(now),
        bbr.data_volume_model.extra_acked_interval_start()
    );
    assert!(bbr.pacer.pacing_rate() > pacing_rate);
}

#[test]
fn model_update_required() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    let rate_sample = RateSample {
        delivered_bytes: 100_000,
        interval: Duration::from_millis(1),
        ..Default::default()
    };
    bbr.data_rate_model.update_max_bw(rate_sample);
    bbr.data_rate_model.bound_bw_for_model();
    let rate_sample = RateSample {
        delivered_bytes: 10_000,
        interval: Duration::from_millis(1),
        is_app_limited: true,
        ..Default::default()
    };
    bbr.bw_estimator.set_rate_sample_for_test(rate_sample);

    assert!(bbr.bw_estimator.rate_sample().is_app_limited);
    assert!(bbr.bw_estimator.rate_sample().delivery_rate() < bbr.data_rate_model.max_bw());
    assert!(!bbr.congestion_state.loss_in_round());
    assert!(!bbr.congestion_state.ecn_in_round());

    // try_fast_path
    bbr.try_fast_path = false;
    assert!(bbr.model_update_required());
    bbr.try_fast_path = true;
    assert!(!bbr.model_update_required());

    // app limited
    let rate_sample = RateSample {
        delivered_bytes: 10_000,
        interval: Duration::from_millis(1),
        is_app_limited: false,
        ..Default::default()
    };
    bbr.bw_estimator.set_rate_sample_for_test(rate_sample);
    assert!(bbr.model_update_required());
    let rate_sample = RateSample {
        delivered_bytes: 10_000,
        interval: Duration::from_millis(1),
        is_app_limited: true,
        ..Default::default()
    };
    bbr.bw_estimator.set_rate_sample_for_test(rate_sample);
    assert!(!bbr.model_update_required());

    // delivery_rate >= max_bw
    let rate_sample = RateSample {
        delivered_bytes: 500_000,
        interval: Duration::from_millis(1),
        is_app_limited: true,
        ..Default::default()
    };
    bbr.bw_estimator.set_rate_sample_for_test(rate_sample);
    assert!(bbr.bw_estimator.rate_sample().delivery_rate() >= bbr.data_rate_model.max_bw());
    assert!(bbr.model_update_required());
    let rate_sample = RateSample {
        delivered_bytes: 10_000,
        interval: Duration::from_millis(1),
        is_app_limited: true,
        ..Default::default()
    };
    bbr.bw_estimator.set_rate_sample_for_test(rate_sample);
    assert!(bbr.bw_estimator.rate_sample().delivery_rate() < bbr.data_rate_model.max_bw());
    assert!(!bbr.model_update_required());

    // loss in round
    bbr.congestion_state.on_packet_lost(100, true);
    assert!(bbr.model_update_required());
    bbr.congestion_state.reset();
    assert!(!bbr.model_update_required());

    // ecn in round
    bbr.congestion_state.on_explicit_congestion();
    assert!(bbr.model_update_required());
    bbr.congestion_state.reset();
    assert!(!bbr.model_update_required());
}

#[test]
fn control_update_required() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MAX_DATAGRAM_SIZE);
    let now = NoopClock.get_time();

    bbr.try_fast_path = true;
    let mut model_updated = false;
    assert!(bbr.data_volume_model.min_rtt().is_none());

    assert!(!bbr.control_update_required(model_updated, None));

    // try_fast_path
    bbr.try_fast_path = false;
    assert!(bbr.control_update_required(model_updated, None));
    bbr.try_fast_path = true;
    assert!(!bbr.control_update_required(model_updated, None));

    // model_updated
    model_updated = true;
    assert!(bbr.control_update_required(model_updated, None));
    model_updated = false;
    assert!(!bbr.control_update_required(model_updated, None));

    // prev_min_rtt != self.data_volume_model.min_rtt()
    assert!(bbr.control_update_required(model_updated, Some(Duration::from_millis(100))));
    bbr.data_volume_model
        .update_min_rtt(Duration::from_millis(100), now);
    assert!(bbr.control_update_required(model_updated, None));
    assert!(bbr.control_update_required(model_updated, Some(Duration::from_millis(200))));
    assert!(!bbr.control_update_required(model_updated, Some(Duration::from_millis(100))));
}

#[test]
fn on_mtu_update() {
    let mut mtu = 5000;
    let mut bbr = BbrCongestionController::new(mtu);
    let mut publisher = event::testing::Publisher::snapshot();
    let mut publisher = PathPublisher::new(&mut publisher, path::Id::test_id());

    bbr.cwnd = 100_000;

    mtu = 10000;
    bbr.on_mtu_update(mtu, &mut publisher);

    assert_eq!(bbr.max_datagram_size, mtu);
    assert_eq!(bbr.cwnd, 200_000);
}

/// Helper method to move the given BBR congestion controller into the
/// ProbeBW state with the given CyclePhase
fn enter_probe_bw_state<Pub: Publisher>(
    bbr: &mut BbrCongestionController,
    cycle_phase: CyclePhase,
    publisher: &mut Pub,
) {
    let now = NoopClock.get_time();

    match bbr.state {
        State::Startup => {
            bbr.enter_drain(publisher);
            bbr.enter_probe_bw(
                false,
                &mut random::testing::Generator::default(),
                now,
                publisher,
            );
        }
        State::Drain | State::ProbeRtt(_) => {
            bbr.enter_probe_bw(
                false,
                &mut random::testing::Generator::default(),
                now,
                publisher,
            );
        }
        State::ProbeBw(_) => {}
    }

    if let State::ProbeBw(ref mut probe_bw_state) = bbr.state {
        probe_bw_state.set_cycle_phase_for_test(cycle_phase);
    }
}
