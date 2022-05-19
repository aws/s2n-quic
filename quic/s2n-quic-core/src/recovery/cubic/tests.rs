// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::{
    packet::number::PacketNumberSpace,
    time::{Clock, NoopClock},
};
use core::time::Duration;

#[macro_export]
macro_rules! assert_delta {
    ($x:expr, $y:expr, $d:expr) => {
        assert!(
            ($x - $y).abs() < $d,
            "assertion failed: `({:?} - {:?}).abs() < {:?})`",
            $x,
            $y,
            $d
        );
    };
}

fn bytes_to_packets(bytes: f32, max_datagram_size: u16) -> f32 {
    bytes / max_datagram_size as f32
}

#[test]
//= https://www.rfc-editor.org/rfc/rfc8312#section-4.1
//= type=test
fn w_cubic() {
    let max_datagram_size = 1200;
    let mut cubic = Cubic::new(max_datagram_size);

    // 2_764_800 is used because it can be divided by 1200 and then have a cubic
    // root result in an integer value.
    cubic.multiplicative_decrease(2_764_800.0);
    assert_delta!(
        cubic.w_max,
        bytes_to_packets(2_764_800.0, max_datagram_size),
        0.001
    );

    let mut t = Duration::from_secs(0);

    // W_cubic(0)=W_max*beta_cubic
    assert_delta!(cubic.w_max * BETA_CUBIC, cubic.w_cubic(t), 0.001);

    // K = cubic_root(W_max*(1-beta_cubic)/C)
    // K = cubic_root(2304 * 0.75) = 12
    assert_eq!(cubic.k, Duration::from_secs(12));

    //= https://www.rfc-editor.org/rfc/rfc8312#section-5.1
    //= type=test
    //# Therefore, C SHOULD be set to 0.4.

    // W_cubic(t) = C*(t-K)^3 + W_max
    // W_cubic(t) = .4*(t-12)^3 + 2304
    // W_cubic(15) = .4*27 + 2304 = 2314.8
    t = Duration::from_secs(15);
    assert_delta!(cubic.w_cubic(t), 2314.8, 0.001);

    // W_cubic(10) = .4*-8 + 2304 = 2300.8
    t = Duration::from_secs(10);
    assert_delta!(cubic.w_cubic(t), 2300.8, 0.001);
}

#[test]
//= https://www.rfc-editor.org/rfc/rfc8312#section-4.6
//= type=test
fn w_est() {
    let max_datagram_size = 1200;
    let mut cubic = Cubic::new(max_datagram_size);
    cubic.w_max = 100.0;
    let t = Duration::from_secs(6);
    let rtt = Duration::from_millis(300);

    // W_est(t) = W_max*beta_cubic + [3*(1-beta_cubic)/(1+beta_cubic)] * (t/RTT)
    // W_est(6) = 100*.7 + [3*(1-.7)/(1+.7)] * (6/.3)
    // W_est(6) = 70 + 0.5294117647 * 20 = 80.588235294

    assert_delta!(cubic.w_est(t, rtt), 80.5882, 0.001);
}

#[allow(clippy::float_cmp)]
#[test]
//= https://www.rfc-editor.org/rfc/rfc8312#section-4.5
//= type=test
fn multiplicative_decrease() {
    let max_datagram_size = 1200.0;
    let mut cubic = Cubic::new(max_datagram_size as u16);
    cubic.w_max = bytes_to_packets(10000.0, max_datagram_size as u16);

    assert_eq!(
        cubic.multiplicative_decrease(100_000.0),
        (100_000.0 * BETA_CUBIC)
    );
    // Window max was not less than the last max, so not fast convergence
    assert_delta!(cubic.w_last_max, cubic.w_max, 0.001);
    assert_delta!(cubic.w_max, 100_000.0 / max_datagram_size, 0.001);

    assert_eq!(
        cubic.multiplicative_decrease(80000.0),
        (80000.0 * BETA_CUBIC)
    );
    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.6
    //= type=test
    //# To speed up this bandwidth release by
    //# existing flows, the following mechanism called "fast convergence"
    //# SHOULD be implemented.
    // Window max was less than the last max, so fast convergence applies
    assert_delta!(cubic.w_last_max, 80000.0 / max_datagram_size, 0.001);
    // W_max = W_max*(1.0+beta_cubic)/2.0 = W_max * .85
    assert_delta!(cubic.w_max, 80000.0 * 0.85 / max_datagram_size, 0.001);

    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.5
    //= type=test
    //# Parameter beta_cubic SHOULD be set to 0.7.
    assert_eq!(0.7, BETA_CUBIC);
}

#[test]
//= https://www.rfc-editor.org/rfc/rfc9002#section-7.8
//= type=test
fn is_congestion_limited() {
    let max_datagram_size = 1000;
    let mut cc = CubicCongestionController::new(max_datagram_size);
    cc.congestion_window = 1000.0;
    cc.bytes_in_flight = BytesInFlight::new(100);

    assert!(cc.is_congestion_limited());

    cc.congestion_window = 1100.0;

    assert!(!cc.is_congestion_limited());

    cc.bytes_in_flight = BytesInFlight::new(2000);

    assert!(cc.is_congestion_limited());
}

#[test]
fn is_congestion_window_under_utilized() {
    let max_datagram_size = 1200;
    let mut cc = CubicCongestionController::new(max_datagram_size);
    cc.congestion_window = 12000.0;

    // In Slow Start, the window is under utilized if it is less than half full
    cc.bytes_in_flight = BytesInFlight::new(5999);
    cc.state = SlowStart;
    assert!(cc.is_congestion_window_under_utilized());

    cc.bytes_in_flight = BytesInFlight::new(6000);
    assert!(!cc.is_congestion_window_under_utilized());

    cc.state = State::congestion_avoidance(NoopClock.get_time());
    assert!(cc.is_congestion_window_under_utilized());

    // In Congestion Avoidance, the window is under utilized if there are more than
    // 3 * MTU bytes available in the congestion window (12000 - 3 * 1200 = 8400)
    cc.bytes_in_flight = BytesInFlight::new(8399);
    assert!(cc.is_congestion_window_under_utilized());

    cc.bytes_in_flight = BytesInFlight::new(8400);
    assert!(!cc.is_congestion_window_under_utilized());
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.2
//= type=test
//# Endpoints SHOULD use an initial congestion
//# window of ten times the maximum datagram size (max_datagram_size),
//# while limiting the window to the larger of 14,720 bytes or twice the
//# maximum datagram size.
#[test]
fn initial_window() {
    let mut max_datagram_size = 1200;
    assert_eq!(
        (max_datagram_size * 10) as u32,
        CubicCongestionController::initial_window(max_datagram_size)
    );

    max_datagram_size = 2000;
    assert_eq!(
        14720,
        CubicCongestionController::initial_window(max_datagram_size)
    );

    max_datagram_size = 8000;
    assert_eq!(
        (max_datagram_size * 2) as u32,
        CubicCongestionController::initial_window(max_datagram_size)
    );
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.2
//= type=test
//# The RECOMMENDED
//# value is 2 * max_datagram_size.
#[test]
fn minimum_window_equals_two_times_max_datagram_size() {
    let max_datagram_size = 1200;
    let cc = CubicCongestionController::new(max_datagram_size);

    assert_delta!(
        (2 * max_datagram_size) as f32,
        cc.cubic.minimum_window(),
        0.001
    );
}

#[test]
fn on_packet_sent() {
    let mut cc = CubicCongestionController::new(1000);
    let mut rtt_estimator = RttEstimator::new(Duration::from_millis(0));
    let now = NoopClock.get_time();

    cc.congestion_window = 100_000.0;

    // Last sent packet time updated to t10
    cc.on_packet_sent(now + Duration::from_secs(10), 1, &rtt_estimator);

    assert_eq!(cc.bytes_in_flight, 1);

    // Latest RTT is 100ms
    rtt_estimator.update_rtt(
        Duration::from_millis(0),
        Duration::from_millis(100),
        now,
        true,
        PacketNumberSpace::ApplicationData,
    );

    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.8
    //= type=test
    //# CUBIC MUST employ a slow-start algorithm, when the cwnd is no more
    //# than ssthresh.  Among the slow-start algorithms, CUBIC MAY choose the
    //# standard TCP slow start [RFC5681] in general networks, or the limited
    //# slow start [RFC3742] or hybrid slow start [HR08] for fast and long-
    //# distance networks.

    // Round one of hybrid slow start
    cc.on_rtt_update(now, &rtt_estimator);

    // Latest RTT is 200ms
    rtt_estimator.update_rtt(
        Duration::from_millis(0),
        Duration::from_millis(200),
        now,
        true,
        PacketNumberSpace::ApplicationData,
    );

    // Last sent packet time updated to t20
    cc.on_packet_sent(now + Duration::from_secs(20), 1, &rtt_estimator);

    assert_eq!(cc.bytes_in_flight, 2);

    // Round two of hybrid slow start
    for _i in 1..=8 {
        cc.on_rtt_update(now + Duration::from_secs(10), &rtt_estimator);
    }

    assert_delta!(cc.slow_start.threshold, 100_000.0, 0.001);
}

#[test]
fn on_packet_sent_application_limited() {
    let mut cc = CubicCongestionController::new(1000);
    let rtt_estimator = RttEstimator::new(Duration::from_millis(0));
    let now = NoopClock.get_time();

    cc.congestion_window = 100_000.0;
    cc.bytes_in_flight = BytesInFlight::new(92_500);
    cc.state = SlowStart;

    // t0: Send a packet in Slow Start
    cc.on_packet_sent(now, 1000, &rtt_estimator);

    assert_eq!(cc.bytes_in_flight, 93_500);
    assert_eq!(cc.time_of_last_sent_packet, Some(now));

    // t10: Enter Congestion Avoidance
    cc.state = State::congestion_avoidance(now + Duration::from_secs(10));

    assert!(!cc.under_utilized);

    // t15: Send a packet in Congestion Avoidance
    cc.on_packet_sent(now + Duration::from_secs(15), 1000, &rtt_estimator);

    assert_eq!(cc.bytes_in_flight, 94_500);
    assert_eq!(
        cc.time_of_last_sent_packet,
        Some(now + Duration::from_secs(15))
    );
    assert!(cc.under_utilized);

    // t20: Send packets to fully utilize the congestion window
    while cc.bytes_in_flight < cc.congestion_window() {
        cc.on_packet_sent(now + Duration::from_secs(20), 1000, &rtt_estimator);
    }

    assert!(!cc.under_utilized);
}

#[test]
fn on_packet_sent_fast_retransmission() {
    let mut cc = CubicCongestionController::new(1000);
    let rtt_estimator = RttEstimator::new(Duration::from_millis(0));
    let now = NoopClock.get_time();

    cc.congestion_window = 100_000.0;
    cc.bytes_in_flight = BytesInFlight::new(99900);
    cc.state = Recovery(now, RequiresTransmission);

    cc.on_packet_sent(now + Duration::from_secs(10), 100, &rtt_estimator);

    assert_eq!(cc.state, Recovery(now, Idle));
}

//= https://www.rfc-editor.org/rfc/rfc8312#section-5.8
//= type=test
//# In case of long periods when cwnd has not been updated due
//# to the application rate limit, such as idle periods, t in Eq. 1 MUST
//# NOT include these periods; otherwise, W_cubic(t) might be very high
//# after restarting from these periods.
#[test]
fn congestion_avoidance_after_idle_period() {
    let mut cc = CubicCongestionController::new(1000);
    let now = NoopClock.get_time();
    let rtt_estimator = &RttEstimator::new(Duration::from_secs(0));
    let random = &mut random::testing::Generator::default();

    cc.congestion_window = 6000.0;
    cc.bytes_in_flight = BytesInFlight::new(0);
    cc.state = SlowStart;

    // t0: Send a packet in Slow Start
    cc.on_packet_sent(now, 1000, rtt_estimator);

    assert_eq!(cc.bytes_in_flight, 1000);

    // t10: Enter Congestion Avoidance
    cc.cubic.w_max = 6.0;
    cc.state = State::congestion_avoidance(now + Duration::from_secs(10));

    // t15: Send a packet in Congestion Avoidance while under utilized
    cc.on_packet_sent(now + Duration::from_secs(15), 1000, rtt_estimator);
    assert!(cc.is_congestion_window_under_utilized());

    assert_eq!(cc.bytes_in_flight, 2000);

    // t16: Ack a packet in Congestion Avoidance
    cc.on_ack(
        now,
        1000,
        (),
        rtt_estimator,
        random,
        now + Duration::from_secs(16),
    );
    // Verify the app limited time is set
    assert_eq!(
        cc.state,
        CongestionAvoidance(CongestionAvoidanceTiming {
            start_time: now + Duration::from_secs(10),
            window_increase_time: now + Duration::from_secs(10),
            app_limited_time: Some(now + Duration::from_secs(16)),
        })
    );

    assert_eq!(cc.bytes_in_flight, 1000);

    // t20: Send packets to fully utilize the congestion window
    while cc.bytes_in_flight < cc.congestion_window() {
        cc.on_packet_sent(now + Duration::from_secs(20), 1000, rtt_estimator);
    }

    assert!(!cc.is_congestion_window_under_utilized());

    // t25: Ack a packet in Congestion Avoidance
    cc.on_ack(
        now,
        1000,
        (),
        rtt_estimator,
        random,
        now + Duration::from_secs(25),
    );

    // Verify congestion avoidance start time was moved from t10 to t16 to account
    // for the 6 seconds of under utilized time and the app_limited_time was reset
    assert_eq!(
        cc.state,
        CongestionAvoidance(CongestionAvoidanceTiming {
            start_time: now + Duration::from_secs(16),
            window_increase_time: now + Duration::from_secs(25),
            app_limited_time: None,
        })
    );
    // Verify t does not include the app limited time
    if let CongestionAvoidance(timing) = cc.state {
        assert_eq!(
            Duration::from_secs(9),
            timing.t(now + Duration::from_secs(25))
        );
    }
}

#[test]
fn congestion_avoidance_after_fast_convergence() {
    let max_datagram_size = 1200;
    let mut cc = CubicCongestionController::new(max_datagram_size);
    let now = NoopClock.get_time();
    let random = &mut random::testing::Generator::default();
    cc.bytes_in_flight = BytesInFlight::new(100);
    cc.congestion_window = 80_000.0;
    cc.cubic.w_last_max = bytes_to_packets(100_000.0, max_datagram_size);

    cc.on_packet_lost(100, (), false, false, random, now);
    assert_delta!(cc.congestion_window, 80_000.0 * BETA_CUBIC, 0.001);

    // Window max was less than the last max, so fast convergence applies
    assert_delta!(
        cc.cubic.w_last_max,
        80000.0 / max_datagram_size as f32,
        0.001
    );
    // W_max = W_max*(1.0+beta_cubic)/2.0 = W_max * .85
    assert_delta!(
        cc.cubic.w_max,
        80000.0 * 0.85 / max_datagram_size as f32,
        0.001
    );

    let prev_cwnd = cc.congestion_window;

    // Enter congestion avoidance
    cc.congestion_avoidance(Duration::from_millis(10), Duration::from_millis(100), 100);

    // Verify congestion window has increased
    assert!(cc.congestion_window > prev_cwnd);
}

#[test]
fn congestion_avoidance_after_rtt_improvement() {
    let max_datagram_size = 1200;
    let mut cc = CubicCongestionController::new(max_datagram_size);
    cc.bytes_in_flight = BytesInFlight::new(100);
    cc.congestion_window = 80_000.0;
    cc.cubic.w_max = cc.congestion_window / 1200.0;

    // Enter congestion avoidance with a long rtt
    cc.congestion_avoidance(Duration::from_millis(10), Duration::from_millis(750), 100);

    // At this point the target is less than the congestion window
    let prev_cwnd = cc.congestion_window;
    assert!(
        cc.cubic.w_cubic(Duration::from_secs(0)) < bytes_to_packets(prev_cwnd, max_datagram_size)
    );

    // Receive another ack, now with a short rtt
    cc.congestion_avoidance(Duration::from_millis(20), Duration::from_millis(10), 100);

    // Verify congestion window did not change
    assert_delta!(cc.congestion_window, prev_cwnd, 0.001);
}

#[test]
fn congestion_avoidance_with_small_min_rtt() {
    let max_datagram_size = 1200;
    let mut cc = CubicCongestionController::new(max_datagram_size);
    cc.bytes_in_flight = BytesInFlight::new(100);
    cc.congestion_window = 80_000.0;
    cc.cubic.w_max = cc.congestion_window / 1200.0;

    cc.congestion_avoidance(Duration::from_millis(100), Duration::from_millis(1), 100);

    // Verify the window grew by half the sent bytes
    assert_delta!(cc.congestion_window, 80_050.0, 0.001);
}

#[test]
fn on_packet_lost() {
    let mut cc = CubicCongestionController::new(1000);
    let now = NoopClock.get_time();
    let random = &mut random::testing::Generator::default();
    cc.congestion_window = 100_000.0;
    cc.bytes_in_flight = BytesInFlight::new(100_000);
    cc.state = SlowStart;

    cc.on_packet_lost(100, (), false, false, random, now + Duration::from_secs(10));

    assert_eq!(cc.bytes_in_flight, 100_000u32 - 100);
    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.1
    //= type=test
    //# The sender MUST exit slow start and enter a recovery period when a
    //# packet is lost or when the ECN-CE count reported by its peer
    //# increases.
    assert_eq!(
        cc.state,
        Recovery(now + Duration::from_secs(10), RequiresTransmission)
    );
    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
    //= type=test
    //# Implementations MAY reduce the congestion window immediately upon
    //# entering a recovery period or use other mechanisms, such as
    //# Proportional Rate Reduction [PRR], to reduce the congestion window
    //# more gradually.
    assert_delta!(cc.congestion_window, 100_000.0 * BETA_CUBIC, 0.001);
    assert_delta!(cc.slow_start.threshold, 100_000.0 * BETA_CUBIC, 0.001);
}

#[test]
fn on_packet_lost_below_minimum_window() {
    let mut cc = CubicCongestionController::new(1000);
    let now = NoopClock.get_time();
    let random = &mut random::testing::Generator::default();
    cc.congestion_window = cc.cubic.minimum_window();
    cc.bytes_in_flight = BytesInFlight::new(cc.congestion_window());
    cc.state = State::congestion_avoidance(now);

    cc.on_packet_lost(100, (), false, false, random, now + Duration::from_secs(10));

    assert_delta!(cc.congestion_window, cc.cubic.minimum_window(), 0.001);
}

#[test]
fn on_packet_lost_already_in_recovery() {
    let mut cc = CubicCongestionController::new(1000);
    let now = NoopClock.get_time();
    let random = &mut random::testing::Generator::default();
    cc.congestion_window = 10000.0;
    cc.bytes_in_flight = BytesInFlight::new(1000);
    cc.state = Recovery(now, Idle);

    // break up on_packet_loss into two call to confirm double call
    // behavior is valid (50 + 50 = 100 lost bytes)
    cc.on_packet_lost(50, (), false, false, random, now);
    cc.on_packet_lost(50, (), false, false, random, now);

    // No change to the congestion window
    assert_delta!(cc.congestion_window, 10000.0, 0.001);
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.2
//= type=test
//# When persistent congestion is declared, the sender's congestion
//# window MUST be reduced to the minimum congestion window
//# (kMinimumWindow), similar to a TCP sender's response on an RTO
//# [RFC5681].
#[test]
fn on_packet_lost_persistent_congestion() {
    let mut cc = CubicCongestionController::new(1000);
    let now = NoopClock.get_time();
    let random = &mut random::testing::Generator::default();
    cc.congestion_window = 10000.0;
    cc.bytes_in_flight = BytesInFlight::new(1000);
    cc.state = Recovery(now, Idle);

    cc.on_packet_lost(100, (), true, false, random, now);

    assert_eq!(cc.state, SlowStart);
    assert_delta!(cc.congestion_window, cc.cubic.minimum_window(), 0.001);
    assert_delta!(cc.cubic.w_max, 0.0, 0.001);
    assert_delta!(cc.cubic.w_last_max, 0.0, 0.001);
    assert_eq!(cc.cubic.k, Duration::from_millis(0));
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.2
//= type=test
//# If the maximum datagram size is decreased in order to complete the
//# handshake, the congestion window SHOULD be set to the new initial
//# congestion window.
#[test]
fn on_mtu_update_decrease() {
    let mut cc = CubicCongestionController::new(10000);

    cc.on_mtu_update(5000);
    assert_eq!(cc.max_datagram_size, 5000);
    assert_eq!(cc.cubic.max_datagram_size, 5000);

    assert_delta!(
        cc.congestion_window,
        CubicCongestionController::initial_window(5000) as f32,
        0.001
    );
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.2
//= type=test
//# If the maximum datagram size changes during the connection, the
//# initial congestion window SHOULD be recalculated with the new size.

//= https://www.rfc-editor.org/rfc/rfc8899#section-3
//= type=test
//# A PL that maintains the congestion window in terms of a limit to
//# the number of outstanding fixed-size packets SHOULD adapt this
//# limit to compensate for the size of the actual packets.
#[test]
fn on_mtu_update_increase() {
    let mut mtu = 5000;
    let cwnd_in_packets = 100_000f32;
    let cwnd_in_bytes = cwnd_in_packets / mtu as f32;
    let mut cc = CubicCongestionController::new(mtu);
    cc.congestion_window = cwnd_in_packets;

    mtu = 10000;
    cc.on_mtu_update(mtu);
    assert_eq!(cc.max_datagram_size, mtu);
    assert_eq!(cc.cubic.max_datagram_size, mtu);

    assert_delta!(cc.congestion_window, 200_000.0, 0.001);

    //= https://www.rfc-editor.org/rfc/rfc8899#section-3
    //= type=test
    //# An update to the PLPMTU (or MPS) MUST NOT increase the congestion
    //# window measured in bytes [RFC4821].
    assert_delta!(cc.congestion_window / mtu as f32, cwnd_in_bytes, 0.001);
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-6.4
//= type=test
//# The sender MUST discard all recovery state associated with
//# those packets and MUST remove them from the count of bytes in flight.
#[test]
fn on_packet_discarded() {
    let mut cc = CubicCongestionController::new(5000);
    cc.bytes_in_flight = BytesInFlight::new(10000);

    cc.on_packet_discarded(1000);

    assert_eq!(cc.bytes_in_flight, 10000 - 1000);

    let now = NoopClock.get_time();
    cc.state = Recovery(now, FastRetransmission::RequiresTransmission);

    cc.on_packet_discarded(1000);

    assert_eq!(Recovery(now, FastRetransmission::Idle), cc.state);
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.8
//= type=test
//# When bytes in flight is smaller than the congestion window and
//# sending is not pacing limited, the congestion window is
//# underutilized.  This can happen due to insufficient application data
//# or flow control limits.  When this occurs, the congestion window
//# SHOULD NOT be increased in either slow start or congestion avoidance.
#[test]
fn on_packet_ack_limited() {
    let mut cc = CubicCongestionController::new(5000);
    let now = NoopClock.get_time();
    let random = &mut random::testing::Generator::default();
    cc.congestion_window = 100_000.0;
    cc.bytes_in_flight = BytesInFlight::new(10000);
    cc.under_utilized = true;
    cc.state = SlowStart;

    cc.on_ack(
        now,
        1,
        (),
        &RttEstimator::new(Duration::from_secs(0)),
        random,
        now,
    );

    assert_delta!(cc.congestion_window, 100_000.0, 0.001);

    cc.state = State::congestion_avoidance(now);

    cc.on_ack(
        now,
        1,
        (),
        &RttEstimator::new(Duration::from_secs(0)),
        random,
        now,
    );

    assert_delta!(cc.congestion_window, 100_000.0, 0.001);
}

#[test]
#[should_panic]
fn on_packet_ack_timestamp_regression() {
    let mut cc = CubicCongestionController::new(5000);
    let now = NoopClock.get_time() + Duration::from_secs(1);
    let rtt_estimator = RttEstimator::new(Duration::from_secs(0));
    let random = &mut random::testing::Generator::default();
    cc.congestion_window = 100_000.0;
    cc.bytes_in_flight = BytesInFlight::new(10000);
    cc.under_utilized = true;
    cc.state = State::congestion_avoidance(now);

    cc.on_ack(now, 1, (), &rtt_estimator, random, now);

    assert_eq!(
        State::CongestionAvoidance(CongestionAvoidanceTiming {
            start_time: now,
            window_increase_time: now,
            app_limited_time: Some(now),
        }),
        cc.state
    );

    cc.on_ack(
        now,
        1,
        (),
        &rtt_estimator,
        random,
        now - Duration::from_secs(1),
    );
}

#[test]
fn on_packet_ack_utilized_then_under_utilized() {
    let mut cc = CubicCongestionController::new(5000);
    let now = NoopClock.get_time();
    let mut rtt_estimator = RttEstimator::new(Duration::from_secs(0));
    let random = &mut random::testing::Generator::default();
    rtt_estimator.update_rtt(
        Duration::from_secs(0),
        Duration::from_millis(200),
        now,
        true,
        PacketNumberSpace::ApplicationData,
    );
    cc.congestion_window = 100_000.0;
    cc.state = SlowStart;

    cc.on_packet_sent(now, 60_000, &rtt_estimator);
    cc.on_ack(now, 50_000, (), &rtt_estimator, random, now);
    let cwnd = cc.congestion_window();

    assert!(!cc.under_utilized);
    assert!(cwnd > 100_000);

    // Now the window is under utilized, but we still grow the window until more packets are sent
    assert!(cc.is_congestion_window_under_utilized());
    cc.on_ack(
        now,
        1200,
        (),
        &rtt_estimator,
        random,
        now + Duration::from_millis(100),
    );
    assert!(cc.congestion_window() > cwnd);

    let cwnd = cc.congestion_window();

    // Now the application has had a chance to send more data, but it didn't send enough to
    // utilize the congestion window, so the window does not grow.
    cc.on_packet_sent(now, 1200, &rtt_estimator);
    assert!(cc.under_utilized);
    cc.on_ack(
        now,
        1200,
        (),
        &rtt_estimator,
        random,
        now + Duration::from_millis(201),
    );
    assert_eq!(cc.congestion_window(), cwnd);
}

#[test]
//= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
//= type=test
fn on_packet_ack_recovery_to_congestion_avoidance() {
    let mut cc = CubicCongestionController::new(5000);
    let now = NoopClock.get_time();
    let random = &mut random::testing::Generator::default();

    cc.cubic.w_max = bytes_to_packets(25000.0, 5000);
    cc.state = Recovery(now, Idle);
    cc.bytes_in_flight = BytesInFlight::new(25000);
    cc.under_utilized = false;

    cc.on_ack(
        now + Duration::from_millis(1),
        1,
        (),
        &RttEstimator::new(Duration::from_secs(0)),
        random,
        now + Duration::from_millis(2),
    );

    assert_eq!(
        cc.state,
        State::congestion_avoidance(now + Duration::from_millis(2))
    );
}

#[test]
//= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
//= type=test
fn on_packet_ack_slow_start_to_congestion_avoidance() {
    let mut cc = CubicCongestionController::new(5000);
    let now = NoopClock.get_time();
    let random = &mut random::testing::Generator::default();

    cc.state = SlowStart;
    cc.congestion_window = 10000.0;
    cc.bytes_in_flight = BytesInFlight::new(10000);
    cc.slow_start.threshold = 10050.0;
    cc.under_utilized = false;

    cc.on_ack(
        now,
        100,
        (),
        &RttEstimator::new(Duration::from_secs(0)),
        random,
        now + Duration::from_millis(2),
    );

    assert_delta!(cc.congestion_window, 10100.0, 0.001);
    assert_delta!(
        cc.packets_to_bytes(cc.cubic.w_max),
        cc.congestion_window,
        0.001
    );
    assert_eq!(cc.cubic.k, Duration::from_secs(0));
    assert_eq!(
        cc.state,
        State::congestion_avoidance(now + Duration::from_millis(2))
    );
}

#[test]
fn on_packet_ack_recovery() {
    let mut cc = CubicCongestionController::new(5000);
    let now = NoopClock.get_time();
    let random = &mut random::testing::Generator::default();

    cc.state = Recovery(now, Idle);
    cc.congestion_window = 10000.0;
    cc.bytes_in_flight = BytesInFlight::new(10000);

    cc.on_ack(
        now,
        100,
        (),
        &RttEstimator::new(Duration::from_secs(0)),
        random,
        now + Duration::from_millis(2),
    );

    // Congestion window stays the same in recovery
    assert_delta!(cc.congestion_window, 10000.0, 0.001);
    assert_eq!(cc.state, Recovery(now, Idle));
}

#[test]
fn on_packet_ack_congestion_avoidance() {
    let max_datagram_size = 5000;
    let mut cc = CubicCongestionController::new(max_datagram_size);
    let mut cc2 = CubicCongestionController::new(max_datagram_size);
    let now = NoopClock.get_time();
    let random = &mut random::testing::Generator::default();

    cc.state = State::congestion_avoidance(now + Duration::from_millis(3300));
    cc.congestion_window = 10000.0;
    cc.bytes_in_flight = BytesInFlight::new(10000);
    cc.cubic.w_max = bytes_to_packets(10000.0, max_datagram_size);
    cc.under_utilized = false;

    cc2.congestion_window = 10000.0;
    cc2.bytes_in_flight = BytesInFlight::new(10000);
    cc2.cubic.w_max = bytes_to_packets(10000.0, max_datagram_size);

    let mut rtt_estimator = RttEstimator::new(Duration::from_secs(0));
    rtt_estimator.update_rtt(
        Duration::from_secs(0),
        Duration::from_millis(275),
        now,
        true,
        PacketNumberSpace::ApplicationData,
    );

    cc.on_ack(
        now,
        1000,
        (),
        &rtt_estimator,
        random,
        now + Duration::from_millis(4750),
    );

    let t = Duration::from_millis(4750) - Duration::from_millis(3300);
    let rtt = rtt_estimator.min_rtt();

    cc2.congestion_avoidance(t, rtt, 1000);

    assert_delta!(cc.congestion_window, cc2.congestion_window, 0.001);
}

//= https://www.rfc-editor.org/rfc/rfc8312#section-4.2
//= type=test
//# If so, CUBIC is in the TCP-friendly region and cwnd SHOULD
//# be set to W_est(t) at each reception of an ACK.
#[test]
fn on_packet_ack_congestion_avoidance_tcp_friendly_region() {
    let mut cc = CubicCongestionController::new(5000);

    cc.congestion_window = 10000.0;
    cc.cubic.w_max = 2.5;
    cc.cubic.k = Duration::from_secs_f32(2.823);

    let t = Duration::from_millis(300);
    let rtt = Duration::from_millis(250);

    cc.congestion_avoidance(t, rtt, 5000);

    assert!(cc.cubic.w_cubic(t) < cc.cubic.w_est(t, rtt));
    assert_delta!(cc.congestion_window, cc.cubic.w_est(t, rtt) * 5000.0, 0.001);
}

//= https://www.rfc-editor.org/rfc/rfc8312#section-4.3
//= type=test
//# In this region, cwnd MUST be incremented by
//# (W_cubic(t+RTT) - cwnd)/cwnd for each received ACK, where
//# W_cubic(t+RTT) is calculated using Eq. 1.
#[test]
fn on_packet_ack_congestion_avoidance_concave_region() {
    let max_datagram_size = 1200;
    let mut cc = CubicCongestionController::new(max_datagram_size as u16);

    cc.congestion_window = 2_400_000.0;
    cc.cubic.w_max = 2304.0;
    cc.cubic.k = Duration::from_secs(12);

    let t = Duration::from_millis(9800);
    let rtt = Duration::from_millis(200);

    cc.congestion_avoidance(t, rtt, 1000);

    assert!(cc.cubic.w_cubic(t) > cc.cubic.w_est(t, rtt));

    // W_cubic(t+RTT) = C*(t-K)^3 + W_max
    // W_cubic(10) = .4*(-2)^3 + 2304
    // W_cubic(10) = 2300.8

    // cwnd = (W_cubic(t+RTT) - cwnd)/cwnd + cwnd
    // cwnd = ((2300.8 - 2000)/2000 + 2000) * max_datagram_size
    // cwnd = 2400180.48

    assert_delta!(cc.congestion_window, 2_400_180.5, 0.001);
}

//= https://www.rfc-editor.org/rfc/rfc8312#section-4.4
//= type=test
//# In this region, cwnd MUST be incremented by
//# (W_cubic(t+RTT) - cwnd)/cwnd for each received ACK, where
//# W_cubic(t+RTT) is calculated using Eq. 1.
#[test]
fn on_packet_ack_congestion_avoidance_convex_region() {
    let max_datagram_size = 1200;
    let mut cc = CubicCongestionController::new(max_datagram_size);

    cc.congestion_window = 3_600_000.0;
    cc.cubic.w_max = 2304.0;
    cc.cubic.k = Duration::from_secs(12);

    let t = Duration::from_millis(25800);
    let rtt = Duration::from_millis(200);

    cc.congestion_avoidance(t, rtt, 1000);

    assert!(cc.cubic.w_cubic(t) > cc.cubic.w_est(t, rtt));

    // W_cubic(t+RTT) = C*(t-K)^3 + W_max
    // W_cubic(26) = .4*(14)^3 + 2304
    // W_cubic(26) = 3401.6

    // cwnd = (W_cubic(t+RTT) - cwnd)/cwnd + cwnd
    // cwnd = ((3401.6 - 3000)/3000 + 3000) * max_datagram_size
    // cwnd = 3600160.64

    assert_eq!(cc.congestion_window(), 3_600_160);
}

#[test]
fn on_packet_ack_congestion_avoidance_too_large_increase() {
    let max_datagram_size = 1200;
    let mut cc = CubicCongestionController::new(max_datagram_size);

    cc.congestion_window = 3_600_000.0;
    cc.cubic.w_max = bytes_to_packets(2_764_800.0, max_datagram_size);

    let t = Duration::from_millis(125_800);
    let rtt = Duration::from_millis(200);

    cc.congestion_avoidance(t, rtt, 1000);

    assert!(cc.cubic.w_cubic(t) > cc.cubic.w_est(t, rtt));
    assert_delta!(cc.congestion_window, 3_600_000.0 + 1000.0 / 2.0, 0.001);
}
