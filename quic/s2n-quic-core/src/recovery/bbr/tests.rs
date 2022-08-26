// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    assert_delta,
    path::MINIMUM_MTU,
    random,
    recovery::{
        bandwidth::{Bandwidth, PacketInfo, RateSample},
        bbr,
        bbr::{probe_bw::CyclePhase, probe_rtt, BbrCongestionController, State, State::ProbeRtt},
        CongestionController,
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

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
//= type=test
//# BBRUpdateMaxInflight()
//#   BBRUpdateAggregationBudget()
//#   inflight = BBRBDPMultiple(BBR.cwnd_gain)
//#   inflight += BBR.extra_acked
//#   BBR.max_inflight = BBRQuantizationBudget(inflight)
#[test]
fn max_inflight() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);

    bbr.data_volume_model.set_extra_acked_for_test(1000, 0);
    // bdp = initial_window = 12000 since min_rtt is not populated
    // inflight = bdp + extra_acked = 13000
    // max_inflight = quantization_budget(13000) = 3 * MAX_SEND_QUANTUM = 192000
    assert_eq!(192000, bbr.max_inflight());
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.2
//= type=test
//# BBRInflight(gain)
//#   inflight = BBRBDPMultiple(gain)
//#   return BBRQuantizationBudget(inflight)
#[test]
fn inflight() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);
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
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);

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
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);
    bbr.send_quantum = 4000;

    // offload_budget = 3 * send_quantum = 12000

    // offload_budget > inflight > minimum_window, return offload_budget
    assert_eq!(12000, bbr.quantization_budget(6000));

    // offload_budget < inflight, return inflight
    assert_eq!(14000, bbr.quantization_budget(14000));

    bbr.send_quantum = 1000;
    // offload_budget = 3 * send_quantum = 3000

    // minimum_window = 4 * mtu = 4800
    // offload_budget < inflight < minimum_window
    assert_eq!(4800, bbr.quantization_budget(2000));

    enter_probe_bw_state(&mut bbr, CyclePhase::Up);
    assert!(bbr.state.is_probing_bw_up());

    // since probe bw up, add 2 packets to the budget
    assert_eq!(4800 + 2 * MINIMUM_MTU as u64, bbr.quantization_budget(2000));
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.2
//= type=test
//# BBRSetPacingRateWithGain(pacing_gain):
//#   rate = pacing_gain * bw * (100 - BBRPacingMarginPercent) / 100
//#   if (BBR.filled_pipe || rate > BBR.pacing_rate)
//#     BBR.pacing_rate = rate
#[test]
fn set_pacing_rate() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);
    let rate_sample = RateSample {
        delivered_bytes: 1000,
        interval: Duration::from_millis(1),
        ..Default::default()
    };
    bbr.data_rate_model.update_max_bw(rate_sample);
    bbr.data_rate_model.bound_bw_for_model();

    bbr.full_pipe_estimator.set_filled_pipe_for_test(true);
    bbr.set_pacing_rate(Ratio::new(5, 4));

    // pacing rate = pacing_gain * bw * (100 - BBRPacingMarginPercent) / 100
    //             = 1.25 * 1000bytes/ms * 99/100
    //             = 1237.5bytes/ms
    assert_eq!(
        Bandwidth::new(12375, Duration::from_millis(10)),
        bbr.pacing_rate
    );
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.3
//= type=test
//# if (BBR.pacing_rate < 1.2 Mbps)
//#   floor = 1 * SMSS
//# else
//#   floor = 2 * SMSS
//# BBR.send_quantum = min(BBR.pacing_rate * 1ms, 64KBytes)
//# BBR.send_quantum = max(BBR.send_quantum, floor)
#[test]
fn set_send_quantum() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);
    // pacing_rate < 1.2 Mbps, floor = MINIMUM_MTU
    bbr.pacing_rate = Bandwidth::new(1_100_000 / 8, Duration::from_secs(1));
    bbr.set_send_quantum();
    // pacing_Rate * 1ms = 137 bytes
    // send_quantum = min(137, 64_000) = 137
    // send_quantum = max(137, MINIMUM_MTU) = MINIMUM_MTU
    assert_eq!(MINIMUM_MTU as usize, bbr.send_quantum);

    // pacing_rate = 1.2 Mbps, floor = 2 * MINIMUM_MTU
    bbr.pacing_rate = Bandwidth::new(1_200_000 / 8, Duration::from_secs(1));
    bbr.set_send_quantum();
    // pacing_Rate * 1ms = 150 bytes
    // send_quantum = min(150, 64_000) = 150
    // send_quantum = max(150, 2 * MINIMUM_MTU) = 2 * MINIMUM_MTU
    assert_eq!(2 * MINIMUM_MTU as usize, bbr.send_quantum);

    // pacing_rate = 10.0 MBps, floor = 2 * MINIMUM_MTU
    bbr.pacing_rate = Bandwidth::new(10_000_000, Duration::from_secs(1));
    bbr.set_send_quantum();
    // pacing_Rate * 1ms = 10000 bytes
    // send_quantum = min(10000, 64_000) = 10000
    // send_quantum = max(10000, 2 * MINIMUM_MTU) = 10000
    assert_eq!(10000, bbr.send_quantum);

    // pacing_rate = 100.0 MBps, floor = 2 * MINIMUM_MTU
    bbr.pacing_rate = Bandwidth::new(100_000_000, Duration::from_secs(1));
    bbr.set_send_quantum();
    // pacing_Rate * 1ms = 100000 bytes
    // send_quantum = min(100000, 64_000) = 64_000
    // send_quantum = max(64_000, 2 * MINIMUM_MTU) = 64_000
    assert_eq!(64_000, bbr.send_quantum);
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.2
//= type=test
//# BBR.next_departure_time = max(Now(), BBR.next_departure_time)
//# packet.departure_time = BBR.next_departure_time
//# pacing_delay = packet.size / BBR.pacing_rate
//# BBR.next_departure_time = BBR.next_departure_time + pacing_delay
#[test]
fn set_next_departure_time() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);
    let now = NoopClock.get_time();

    bbr.pacing_rate = Bandwidth::new(100, Duration::from_millis(1));
    bbr.set_next_departure_time(1000, now);

    assert_eq!(
        Some(now + Duration::from_millis(10)),
        bbr.earliest_departure_time()
    );
}

#[test]
fn is_inflight_too_high() {
    let rate_sample = RateSample {
        lost_bytes: 3,
        bytes_in_flight: 100,
        ..Default::default()
    };
    // loss rate higher than 2% threshold
    assert!(BbrCongestionController::is_inflight_too_high(
        rate_sample,
        MINIMUM_MTU
    ));

    let rate_sample = RateSample {
        lost_bytes: 2,
        bytes_in_flight: 100,
        ..Default::default()
    };
    // loss rate <= 2% threshold
    assert!(!BbrCongestionController::is_inflight_too_high(
        rate_sample,
        MINIMUM_MTU
    ));

    let rate_sample = RateSample {
        delivered_bytes: 100 * MINIMUM_MTU as u64,
        ecn_ce_count: 51,
        ..Default::default()
    };
    // ecn rate higher than 50% threshold
    assert!(BbrCongestionController::is_inflight_too_high(
        rate_sample,
        MINIMUM_MTU
    ));

    let rate_sample = RateSample {
        delivered_bytes: 100 * MINIMUM_MTU as u64,
        ecn_ce_count: 50,
        ..Default::default()
    };
    // ecn rate <= 50% threshold
    assert!(!BbrCongestionController::is_inflight_too_high(
        rate_sample,
        MINIMUM_MTU
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
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);
    enter_probe_bw_state(&mut bbr, CyclePhase::Down);

    bbr.data_volume_model.update_upper_bound(10000);

    assert_eq!(10000, bbr.bound_cwnd_for_model());

    enter_probe_bw_state(&mut bbr, CyclePhase::Cruise);

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

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.4
//= type=test
//# BBRSaveCwnd()
//#   if (!InLossRecovery() and BBR.state != ProbeRTT)
//#     return cwnd
//#   else
//#     return max(BBR.prior_cwnd, cwnd)
#[test]
fn save_cwnd() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);

    // Not in recovery
    bbr.prior_cwnd = 1000;
    bbr.cwnd = 2000;
    bbr.save_cwnd();

    assert_eq!(2000, bbr.prior_cwnd);

    bbr.prior_cwnd = 2000;
    bbr.cwnd = 1000;
    bbr.save_cwnd();
    assert_eq!(1000, bbr.prior_cwnd);

    // Enter probe RTT
    bbr.state = ProbeRtt(probe_rtt::State::default());
    assert!(bbr.state.is_probing_rtt());

    bbr.prior_cwnd = 2000;
    bbr.cwnd = 1000;
    assert_eq!(2000, bbr.prior_cwnd);

    // Enter recovery
    bbr.state = State::Startup;
    let now = NoopClock.get_time();
    bbr.recovery_state.on_congestion_event(now);
    assert!(bbr.recovery_state.in_recovery());

    bbr.prior_cwnd = 2000;
    bbr.cwnd = 1000;
    assert_eq!(2000, bbr.prior_cwnd);
}

//= https://tools.ietf.org/id/draft-cardwell-iccrg-bbr-congestion-control-02#4.6.4.4
//= type=test
//# BBRRestoreCwnd()
//#   cwnd = max(cwnd, BBR.prior_cwnd)
#[test]
fn restore_cwnd() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);

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
//# BBRModulateCwndForRecovery():
//#   if (rs.newly_lost > 0)
//#     cwnd = max(cwnd - rs.newly_lost, 1)
#[test]
fn modulate_cwnd_for_recovery() {
    let mut bbr = BbrCongestionController::new(MINIMUM_MTU);

    bbr.cwnd = 100_000;

    bbr.modulate_cwnd_for_recovery(1000);
    assert_eq!(99_000, bbr.congestion_window());

    // Don't drop below the minimum window
    bbr.cwnd = bbr.minimum_window();
    bbr.modulate_cwnd_for_recovery(1000);
    assert_eq!(bbr.minimum_window(), bbr.congestion_window());
}

/// Helper method to move the given BBR congestion controller into the
/// ProbeBW state with the given CyclePhase
fn enter_probe_bw_state(bbr: &mut BbrCongestionController, cycle_phase: CyclePhase) {
    let now = NoopClock.get_time();

    match bbr.state {
        State::Startup => {
            bbr.enter_drain();
            bbr.enter_probe_bw(false, &mut random::testing::Generator::default(), now);
        }
        State::Drain | State::ProbeRtt(_) => {
            bbr.enter_probe_bw(false, &mut random::testing::Generator::default(), now);
        }
        State::ProbeBw(_) => {}
    }

    if let State::ProbeBw(ref mut probe_bw_state) = bbr.state {
        probe_bw_state.set_cycle_phase_for_test(cycle_phase);
    }
}
