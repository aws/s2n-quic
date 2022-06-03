// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::Counter,
    random,
    recovery::{
        congestion_controller::{self, CongestionController},
        cubic::{FastRetransmission::*, State::*},
        hybrid_slow_start::HybridSlowStart,
        pacing::Pacer,
        RttEstimator,
    },
    time::Timestamp,
};
use core::{
    cmp::{max, min},
    time::Duration,
};
#[cfg(not(feature = "std"))]
use num_traits::Float as _;

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.3
//#                 New Path or      +------------+
//#            persistent congestion |   Slow     |
//#        (O)---------------------->|   Start    |
//#                                  +------------+
//#                                        |
//#                                Loss or |
//#                        ECN-CE increase |
//#                                        v
//# +------------+     Loss or       +------------+
//# | Congestion |  ECN-CE increase  |  Recovery  |
//# | Avoidance  |------------------>|   Period   |
//# +------------+                   +------------+
//#           ^                            |
//#           |                            |
//#          +----------------------------+
//#              Acknowledgment of packet
//#                sent during recovery
// This implementation uses Hybrid Slow Start, which allows for
// Slow Start to exit directly to Congestion Avoidance.
#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    SlowStart,
    Recovery(Timestamp, FastRetransmission),
    CongestionAvoidance(CongestionAvoidanceTiming),
}

impl State {
    /// Returns State::CongestionAvoidance initialized with the given `start_time`
    fn congestion_avoidance(start_time: Timestamp) -> Self {
        Self::CongestionAvoidance(CongestionAvoidanceTiming {
            start_time,
            window_increase_time: start_time,
            app_limited_time: None,
        })
    }

    /// Called when app limited after sending has completed for a round and an ACK has been received.
    fn on_app_limited(&mut self, timestamp: Timestamp) {
        if let CongestionAvoidance(ref mut timing) = self {
            debug_assert!(
                timing
                    .app_limited_time
                    .map_or(true, |app_limited_time| timestamp >= app_limited_time),
                "timestamp must be monotonically increasing"
            );
            debug_assert!(
                timestamp >= timing.window_increase_time,
                "timestamp must not be before the window was last increased"
            );

            timing.app_limited_time = Some(timestamp);
        }
    }
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
//# If the congestion window is reduced immediately, a
//# single packet can be sent prior to reduction.  This speeds up loss
//# recovery if the data in the lost packet is retransmitted and is
//# similar to TCP as described in Section 5 of [RFC6675].
#[derive(Clone, Debug, PartialEq, Eq)]
enum FastRetransmission {
    Idle,
    RequiresTransmission,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CongestionAvoidanceTiming {
    // The time the congestion avoidance state was entered
    start_time: Timestamp,
    // The time the congestion window was last increased
    window_increase_time: Timestamp,
    // The last time the current congestion window was underutilized
    app_limited_time: Option<Timestamp>,
}

impl CongestionAvoidanceTiming {
    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.1
    //# t is the elapsed time from the beginning of the current congestion avoidance
    fn t(&self, timestamp: Timestamp) -> Duration {
        timestamp - self.start_time
    }

    /// Called when the congestion window is being increased.
    ///
    /// Adjusts the start time by the period from the last window increase to the app limited time
    /// to avoid counting the app limited time period in W_cubic.
    fn on_window_increase(&mut self, timestamp: Timestamp) {
        if let Some(app_limited_time) = self.app_limited_time.take() {
            //= https://www.rfc-editor.org/rfc/rfc8312#section-5.8
            //# CUBIC does not raise its congestion window size if the flow is
            //# currently limited by the application instead of the congestion
            //# window.  In case of long periods when cwnd has not been updated due
            //# to the application rate limit, such as idle periods, t in Eq. 1 MUST
            //# NOT include these periods; otherwise, W_cubic(t) might be very high
            //# after restarting from these periods.

            // Adjust the congestion avoidance start time by the app limited duration
            self.start_time += app_limited_time - self.window_increase_time;
        }

        self.window_increase_time = timestamp;
    }
}

/// A congestion controller that implements "CUBIC for Fast Long-Distance Networks"
/// as specified in <https://tools.ietf.org/html/rfc8312>. The Hybrid Slow Start algorithm
/// is used for determining the slow start threshold.
#[derive(Clone, Debug)]
pub struct CubicCongestionController {
    cubic: Cubic,
    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.8
    //# CUBIC MUST employ a slow-start algorithm, when the cwnd is no more
    //# than ssthresh.

    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.8
    //# Among the slow-start algorithms, CUBIC MAY choose the
    //# standard TCP slow start [RFC5681] in general networks, or the limited
    //# slow start [RFC3742] or hybrid slow start [HR08] for fast and long-
    //# distance networks.
    slow_start: HybridSlowStart,
    pacer: Pacer,
    max_datagram_size: u16,
    congestion_window: f32,
    state: State,
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
    time_of_last_sent_packet: Option<Timestamp>,
    under_utilized: bool,
}

type BytesInFlight = Counter<u32>;

impl CongestionController for CubicCongestionController {
    type PacketInfo = ();

    #[inline]
    fn congestion_window(&self) -> u32 {
        self.congestion_window as u32
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
        matches!(self.state, Recovery(_, RequiresTransmission))
    }

    #[inline]
    fn on_packet_sent(
        &mut self,
        time_sent: Timestamp,
        bytes_sent: usize,
        rtt_estimator: &RttEstimator,
    ) {
        if bytes_sent == 0 {
            // Packet was not congestion controlled
            return;
        }

        self.bytes_in_flight
            .try_add(bytes_sent)
            .expect("bytes sent should not exceed u32::MAX");

        self.under_utilized = self.is_congestion_window_under_utilized();

        if let Recovery(recovery_start_time, RequiresTransmission) = self.state {
            // A packet has been sent since we entered recovery (fast retransmission)
            // so flip the state back to idle.
            self.state = Recovery(recovery_start_time, Idle);
        }

        self.time_of_last_sent_packet = Some(time_sent);

        let slow_start = matches!(self.state, State::SlowStart);

        self.pacer.on_packet_sent(
            time_sent,
            bytes_sent,
            rtt_estimator,
            self.congestion_window(),
            self.max_datagram_size,
            slow_start,
        );
    }

    #[inline]
    fn on_rtt_update(&mut self, time_sent: Timestamp, rtt_estimator: &RttEstimator) {
        // Update the Slow Start algorithm each time the RTT
        // estimate is updated to find the slow start threshold.
        self.slow_start.on_rtt_update(
            self.congestion_window,
            time_sent,
            self.time_of_last_sent_packet
                .expect("At least one packet must be sent to update RTT"),
            rtt_estimator.latest_rtt(),
        );
    }

    #[inline]
    fn on_ack<Rnd: random::Generator>(
        &mut self,
        newest_acked_time_sent: Timestamp,
        bytes_acknowledged: usize,
        _newest_acked_packet_info: Self::PacketInfo,
        rtt_estimator: &RttEstimator,
        _random_generator: &mut Rnd,
        ack_receive_time: Timestamp,
    ) {
        self.bytes_in_flight
            .try_sub(bytes_acknowledged)
            .expect("bytes_acknowledged should not exceed u32::MAX");

        if self.under_utilized {
            self.state.on_app_limited(ack_receive_time);

            //= https://www.rfc-editor.org/rfc/rfc9002#section-7.8
            //# When bytes in flight is smaller than the congestion window and
            //# sending is not pacing limited, the congestion window is
            //# underutilized.  This can happen due to insufficient application data
            //# or flow control limits.  When this occurs, the congestion window
            //# SHOULD NOT be increased in either slow start or congestion avoidance.
            return;
        }

        // Check if this ack causes the controller to exit recovery
        if let State::Recovery(recovery_start_time, _) = self.state {
            if newest_acked_time_sent > recovery_start_time {
                //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
                //# A recovery period ends and the sender enters congestion avoidance
                //# when a packet sent during the recovery period is acknowledged.
                self.state = State::congestion_avoidance(ack_receive_time)
            }
        };

        match self.state {
            SlowStart => {
                //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.1
                //# While a sender is in slow start, the congestion window increases by
                //# the number of bytes acknowledged when each acknowledgment is
                //# processed.  This results in exponential growth of the congestion
                //# window.
                self.congestion_window += self.slow_start.cwnd_increment(bytes_acknowledged);

                if self.congestion_window >= self.slow_start.threshold {
                    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.8
                    //# In the case when CUBIC runs the hybrid slow start [HR08], it may exit
                    //# the first slow start without incurring any packet loss and thus W_max
                    //# is undefined.  In this special case, CUBIC switches to congestion
                    //# avoidance and increases its congestion window size using Eq. 1, where
                    //# t is the elapsed time since the beginning of the current congestion
                    //# avoidance, K is set to 0, and W_max is set to the congestion window
                    //# size at the beginning of the current congestion avoidance.
                    self.state = State::congestion_avoidance(ack_receive_time);
                    self.cubic.on_slow_start_exit(self.congestion_window);
                }
            }
            Recovery(_, _) => {
                // Don't increase the congestion window while in recovery
            }
            CongestionAvoidance(ref mut timing) => {
                timing.on_window_increase(ack_receive_time);

                //= https://www.rfc-editor.org/rfc/rfc8312#section-4.1
                //# t is the elapsed time from the beginning of the current congestion avoidance
                let t = timing.t(ack_receive_time);

                //= https://www.rfc-editor.org/rfc/rfc8312#section-4.1
                //# RTT is the weighted average RTT
                // TODO: Linux Kernel Cubic implementation uses min RTT, possibly
                //      because it is more stable than smoothed_rtt. Other implementations
                //      have followed Linux's choice, so we will as well. The end result is a more
                //      conservative rate of increase of the congestion window. This requires
                //      investigation and testing to evaluate if smoothed_rtt would be a better input.
                let rtt = rtt_estimator.min_rtt();

                self.congestion_avoidance(t, rtt, bytes_acknowledged);
            }
        };

        debug_assert!(self.congestion_window >= self.cubic.minimum_window());
    }

    #[inline]
    fn on_packet_lost<Rnd: random::Generator>(
        &mut self,
        lost_bytes: u32,
        _packet_info: Self::PacketInfo,
        persistent_congestion: bool,
        _new_loss_burst: bool,
        _random_generator: &mut Rnd,
        timestamp: Timestamp,
    ) {
        debug_assert!(lost_bytes > 0);

        self.bytes_in_flight -= lost_bytes;
        self.on_congestion_event(timestamp);

        //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.2
        //# When persistent congestion is declared, the sender's congestion
        //# window MUST be reduced to the minimum congestion window
        //# (kMinimumWindow), similar to a TCP sender's response on an RTO
        //# [RFC5681].
        if persistent_congestion {
            self.congestion_window = self.cubic.minimum_window();
            self.state = State::SlowStart;
            self.cubic.reset();
        }
    }

    #[inline]
    fn on_congestion_event(&mut self, event_time: Timestamp) {
        // No reaction if already in a recovery period.
        if matches!(self.state, Recovery(_, _)) {
            return;
        }

        // Enter recovery period.

        //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.1
        //# The sender MUST exit slow start and enter a recovery period when a
        //# packet is lost or when the ECN-CE count reported by its peer
        //# increases.

        //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
        //# If the congestion window is reduced immediately, a
        //# single packet can be sent prior to reduction.  This speeds up loss
        //# recovery if the data in the lost packet is retransmitted and is
        //# similar to TCP as described in Section 5 of [RFC6675].
        self.state = Recovery(event_time, RequiresTransmission);

        //= https://www.rfc-editor.org/rfc/rfc9002#section-7.3.2
        //# Implementations MAY reduce the congestion window immediately upon
        //# entering a recovery period or use other mechanisms, such as
        //# Proportional Rate Reduction [PRR], to reduce the congestion window
        //# more gradually.

        //= https://www.rfc-editor.org/rfc/rfc9002#section-7.2
        //# The minimum congestion window is the smallest value the congestion
        //# window can attain in response to loss, an increase in the peer-
        //# reported ECN-CE count, or persistent congestion.
        self.congestion_window = self.cubic.multiplicative_decrease(self.congestion_window);

        // Update Hybrid Slow Start with the decreased congestion window.
        self.slow_start.on_congestion_event(self.congestion_window);
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.2
    //# If the maximum datagram size changes during the connection, the
    //# initial congestion window SHOULD be recalculated with the new size.
    //# If the maximum datagram size is decreased in order to complete the
    //# handshake, the congestion window SHOULD be set to the new initial
    //# congestion window.

    //= https://www.rfc-editor.org/rfc/rfc8899#section-3
    //# An update to the PLPMTU (or MPS) MUST NOT increase the congestion
    //# window measured in bytes [RFC4821].

    //= https://www.rfc-editor.org/rfc/rfc8899#section-3
    //# A PL that maintains the congestion window in terms of a limit to
    //# the number of outstanding fixed-size packets SHOULD adapt this
    //# limit to compensate for the size of the actual packets.
    #[inline]
    fn on_mtu_update(&mut self, max_datagram_size: u16) {
        let old_max_datagram_size = self.max_datagram_size;
        self.max_datagram_size = max_datagram_size;
        self.cubic.max_datagram_size = max_datagram_size;

        if max_datagram_size < old_max_datagram_size {
            self.congestion_window =
                CubicCongestionController::initial_window(max_datagram_size) as f32;
        } else {
            self.congestion_window =
                (self.congestion_window / old_max_datagram_size as f32) * max_datagram_size as f32;
        }
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.4
    //# When Initial and Handshake packet protection keys are discarded (see
    //# Section 4.9 of [QUIC-TLS]), all packets that were sent with those
    //# keys can no longer be acknowledged because their acknowledgments
    //# cannot be processed.  The sender MUST discard all recovery state
    //# associated with those packets and MUST remove them from the count of
    //# bytes in flight.
    #[inline]
    fn on_packet_discarded(&mut self, bytes_sent: usize) {
        self.bytes_in_flight
            .try_sub(bytes_sent)
            .expect("bytes sent should not exceed u32::MAX");

        if let Recovery(recovery_start_time, RequiresTransmission) = self.state {
            // If any of the discarded packets were lost, they will no longer be retransmitted
            // so flip the Recovery status back to Idle so it is not waiting for a
            // retransmission that may never come.
            self.state = Recovery(recovery_start_time, Idle);
        }
    }

    #[inline]
    fn earliest_departure_time(&self) -> Option<Timestamp> {
        self.pacer.earliest_departure_time()
    }
}

impl CubicCongestionController {
    pub fn new(max_datagram_size: u16) -> Self {
        Self {
            cubic: Cubic::new(max_datagram_size),
            slow_start: HybridSlowStart::new(max_datagram_size),
            pacer: Pacer::default(),
            max_datagram_size,
            congestion_window: CubicCongestionController::initial_window(max_datagram_size) as f32,
            state: SlowStart,
            bytes_in_flight: Counter::new(0),
            time_of_last_sent_packet: None,
            under_utilized: true,
        }
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

    #[inline]
    fn congestion_avoidance(&mut self, t: Duration, rtt: Duration, sent_bytes: usize) {
        let w_cubic = self.cubic.w_cubic(t);
        let w_est = self.cubic.w_est(t, rtt);
        // limit the window increase to half the acked bytes
        // as the Linux implementation of Cubic does.
        let max_cwnd = self.congestion_window + sent_bytes as f32 / 2.0;

        if w_cubic < w_est {
            // TCP-Friendly Region
            //= https://www.rfc-editor.org/rfc/rfc8312#section-4.2
            //# When receiving an ACK in congestion avoidance (cwnd could be greater than
            //# or less than W_max), CUBIC checks whether W_cubic(t) is less than
            //# W_est(t).  If so, CUBIC is in the TCP-friendly region and cwnd SHOULD
            //# be set to W_est(t) at each reception of an ACK.
            self.congestion_window = self.packets_to_bytes(w_est).min(max_cwnd);
        } else {
            //= https://www.rfc-editor.org/rfc/rfc8312#section-4.1
            //# Upon receiving an ACK during congestion avoidance, CUBIC computes the
            //# window increase rate during the next RTT period using Eq. 1.  It sets
            //# W_cubic(t+RTT) as the candidate target value of the congestion
            //# window

            // Concave Region
            //= https://www.rfc-editor.org/rfc/rfc8312#section-4.3
            //# When receiving an ACK in congestion avoidance, if CUBIC is not in the
            //# TCP-friendly region and cwnd is less than W_max, then CUBIC is in the
            //# concave region.  In this region, cwnd MUST be incremented by
            //# (W_cubic(t+RTT) - cwnd)/cwnd for each received ACK, where
            //# W_cubic(t+RTT) is calculated using Eq. 1.

            // Convex Region
            //# https://www.rfc-editor.org/rfc/rfc8312#section-4.4
            //# When receiving an ACK in congestion avoidance, if CUBIC is not in the
            //# TCP-friendly region and cwnd is larger than or equal to W_max, then
            //# CUBIC is in the convex region.

            //= https://www.rfc-editor.org/rfc/rfc8312#section-4.4
            //# In this region, cwnd MUST be incremented by
            //# (W_cubic(t+RTT) - cwnd)/cwnd for each received ACK, where
            //# W_cubic(t+RTT) is calculated using Eq. 1.

            // The congestion window is adjusted in the same way in the convex and concave regions.
            // A target congestion window is calculated for where the congestion window should be
            // by the end of one RTT. That target is used for calculating the required rate of increase
            // based on where the congestion window currently is. Assuming a full congestion window's
            // worth of packets will be sent and acknowledged within that RTT, we increase the
            // congestion window by this increment for each acknowledgement. As long as all packets
            // are sent and acknowledged by the end of the RTT, the congestion window will reach the
            // target size. Otherwise it will be smaller, reflecting that the network latency is
            // higher than needed to achieve the target window, and thus a smaller congestion window
            // is appropriate.
            let target_congestion_window = self.packets_to_bytes(self.cubic.w_cubic(t + rtt));

            // Decreases in the RTT estimate can cause the congestion window to get ahead of the
            // target. In the case where the congestion window has already exceeded the target,
            // we return without any further adjustment to the window.
            if self.congestion_window >= target_congestion_window {
                return;
            }

            let window_increase_rate =
                (target_congestion_window - self.congestion_window) / self.congestion_window;
            let window_increment = self.packets_to_bytes(window_increase_rate);

            self.congestion_window = (self.congestion_window + window_increment).min(max_cwnd);
        }
    }

    #[inline]
    fn packets_to_bytes(&self, cwnd: f32) -> f32 {
        cwnd * self.max_datagram_size as f32
    }

    /// Returns true if the congestion window is under utilized and should not grow larger
    /// without further evidence of the stability of the current window.
    #[inline]
    fn is_congestion_window_under_utilized(&self) -> bool {
        // This value is based on kMaxBurstBytes from Chromium
        // https://source.chromium.org/chromium/chromium/src/+/master:net/third_party/quiche/src/quic/core/congestion_control/tcp_cubic_sender_bytes.cc;l=23;drc=f803516d2656ed829e54b2e819731763ca6cf4d9
        const MAX_BURST_MULTIPLIER: u32 = 3;

        if self.is_congestion_limited() {
            return false;
        }

        // In slow start, allow the congestion window to increase as long as half of it is
        // being used. This allows for the window to increase rapidly.
        if matches!(self.state, SlowStart) && self.bytes_in_flight >= self.congestion_window() / 2 {
            return false;
        }

        // Otherwise allow the window to increase while MAX_BURST_MULTIPLIER packets are available
        // in the window.
        let available_congestion_window = self
            .congestion_window()
            .saturating_sub(*self.bytes_in_flight);
        available_congestion_window > self.max_datagram_size as u32 * MAX_BURST_MULTIPLIER
    }
}

/// Core functions of "CUBIC for Fast Long-Distance Networks" as specified in
/// https://tools.ietf.org/html/rfc8312. The unit of all window sizes is in
/// packets of size max_datagram_size to maintain alignment with the specification.
/// Thus, window sizes should be converted to bytes before applying to the
/// congestion window in the congestion controller.
#[derive(Clone, Debug)]
struct Cubic {
    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.1
    //# W_max is the window size just before the window is
    //# reduced in the last congestion event.
    w_max: f32,
    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.6
    //# a flow remembers the last value of W_max before it
    //# updates W_max for the current congestion event.
    //# Let us call the last value of W_max to be W_last_max.
    w_last_max: f32,
    // k is the time until we expect to reach w_max
    k: Duration,
    max_datagram_size: u16,
}

//= https://www.rfc-editor.org/rfc/rfc8312#section-5.1
//# Based on these observations and our experiments, we find C=0.4
//# gives a good balance between TCP-friendliness and aggressiveness
//# of window increase.  Therefore, C SHOULD be set to 0.4.
const C: f32 = 0.4;

//= https://www.rfc-editor.org/rfc/rfc8312#section-4.5
//# Parameter beta_cubic SHOULD be set to 0.7.
const BETA_CUBIC: f32 = 0.7;

impl Cubic {
    pub fn new(max_datagram_size: u16) -> Self {
        Cubic {
            w_max: 0.0,
            w_last_max: 0.0,
            k: Duration::ZERO,
            max_datagram_size,
        }
    }

    /// Reset to the original state
    #[inline]
    pub fn reset(&mut self) {
        self.w_max = 0.0;
        self.w_last_max = 0.0;
        self.k = Duration::ZERO;
    }

    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.1
    //# CUBIC uses the following window increase function:
    //#
    //#    W_cubic(t) = C*(t-K)^3 + W_max (Eq. 1)
    //#
    //# where C is a constant fixed to determine the aggressiveness of window
    //# increase in high BDP networks, t is the elapsed time from the
    //# beginning of the current congestion avoidance, and K is the time
    //# period that the above function takes to increase the current window
    //# size to W_max if there are no further congestion events and is
    //# calculated using the following equation:
    //#
    //#    K = cubic_root(W_max*(1-beta_cubic)/C) (Eq. 2)
    //#
    //# where beta_cubic is the CUBIC multiplication decrease factor
    #[inline]
    fn w_cubic(&self, t: Duration) -> f32 {
        C * (t.as_secs_f32() - self.k.as_secs_f32()).powi(3) + self.w_max as f32
    }

    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.2
    //# W_est(t) = W_max*beta_cubic +
    //               [3*(1-beta_cubic)/(1+beta_cubic)] * (t/RTT) (Eq. 4)
    #[inline]
    fn w_est(&self, t: Duration, rtt: Duration) -> f32 {
        self.w_max.mul_add(
            BETA_CUBIC,
            (3.0 * (1.0 - BETA_CUBIC) / (1.0 + BETA_CUBIC)) * (t.as_secs_f32() / rtt.as_secs_f32()),
        )
    }

    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.5
    //# When a packet loss is detected by duplicate ACKs or a network
    //# congestion is detected by ECN-Echo ACKs, CUBIC updates its W_max,
    //# cwnd, and ssthresh as follows.  Parameter beta_cubic SHOULD be set to
    //# 0.7.
    //#
    //#    W_max = cwnd;                 // save window size before reduction
    //#    ssthresh = cwnd * beta_cubic; // new slow-start threshold
    //#    ssthresh = max(ssthresh, 2);  // threshold is at least 2 MSS
    //#    cwnd = cwnd * beta_cubic;     // window reduction
    // This does not change the units of the congestion window
    #[inline]
    fn multiplicative_decrease(&mut self, cwnd: f32) -> f32 {
        self.w_max = self.bytes_to_packets(cwnd);

        //= https://www.rfc-editor.org/rfc/rfc8312#section-4.6
        //# To speed up this bandwidth release by
        //# existing flows, the following mechanism called "fast convergence"
        //# SHOULD be implemented.

        //= https://www.rfc-editor.org/rfc/rfc8312#section-4.6
        //# With fast convergence, when a congestion event occurs, before the
        //# window reduction of the congestion window, a flow remembers the last
        //# value of W_max before it updates W_max for the current congestion
        //# event.  Let us call the last value of W_max to be W_last_max.
        //#
        //#    if (W_max < W_last_max){ // should we make room for others
        //#       W_last_max = W_max;             // remember the last W_max
        //#       W_max = W_max*(1.0+beta_cubic)/2.0; // further reduce W_max
        //#    } else {
        //#       W_last_max = W_max              // remember the last W_max
        //#    }
        //#
        //# At a congestion event, if the current value of W_max is less than
        //# W_last_max, this indicates that the saturation point experienced by
        //# this flow is getting reduced because of the change in available
        //# bandwidth.  Then we allow this flow to release more bandwidth by
        //# reducing W_max further.  This action effectively lengthens the time
        //# for this flow to increase its congestion window because the reduced
        //# W_max forces the flow to have the plateau earlier.  This allows more
        //# time for the new flow to catch up to its congestion window size.
        let w_max = self.w_max;
        if w_max < self.w_last_max {
            self.w_max = (w_max * (1.0 + BETA_CUBIC) / 2.0)
                .max(self.bytes_to_packets(self.minimum_window()));
        }
        self.w_last_max = w_max;

        let cwnd_start = (cwnd * BETA_CUBIC).max(self.minimum_window());

        //= https://tools.ietf.org/id/draft-eggert-tcpm-rfc8312bis-01#4.2
        //# _K_ is the time period that the above
        //# function takes to increase the congestion window size at the
        //# beginning of the current congestion avoidance stage to _W_(max)_ if
        //# there are no further congestion events and is calculated using the
        //# following equation:
        //#
        //#                                ________________
        //#                               /W    - cwnd
        //#                           3  /  max       start
        //#                       K = | /  ----------------
        //#                           |/           C
        //#
        //#                                Figure 2
        //#
        //# where _cwnd_(start)_ is the congestion window at the beginning of the
        //# current congestion avoidance stage.
        self.k =
            Duration::from_secs_f32(((self.w_max - self.bytes_to_packets(cwnd_start)) / C).cbrt());

        cwnd_start
    }

    //= https://www.rfc-editor.org/rfc/rfc8312#section-4.8
    //# In the case when CUBIC runs the hybrid slow start [HR08], it may exit
    //# the first slow start without incurring any packet loss and thus W_max
    //# is undefined.  In this special case, CUBIC switches to congestion
    //# avoidance and increases its congestion window size using Eq. 1, where
    //# t is the elapsed time since the beginning of the current congestion
    //# avoidance, K is set to 0, and W_max is set to the congestion window
    //# size at the beginning of the current congestion avoidance.
    #[inline]
    fn on_slow_start_exit(&mut self, cwnd: f32) {
        self.w_max = self.bytes_to_packets(cwnd);

        // We are currently at the w_max, so set k to zero indicating zero
        // seconds to reach the max
        self.k = Duration::from_secs(0);
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.2
    //# The minimum congestion window is the smallest value the congestion
    //# window can attain in response to loss, an increase in the peer-
    //# reported ECN-CE count, or persistent congestion.  The RECOMMENDED
    //# value is 2 * max_datagram_size.
    #[inline]
    fn minimum_window(&self) -> f32 {
        2.0 * self.max_datagram_size as f32
    }

    #[inline]
    fn bytes_to_packets(&self, bytes: f32) -> f32 {
        bytes / self.max_datagram_size as f32
    }
}

#[derive(Debug, Default)]
pub struct Endpoint {}

impl congestion_controller::Endpoint for Endpoint {
    type CongestionController = CubicCongestionController;

    fn new_congestion_controller(
        &mut self,
        path_info: congestion_controller::PathInfo,
    ) -> Self::CongestionController {
        CubicCongestionController::new(path_info.max_datagram_size)
    }
}

#[cfg(test)]
mod tests;
