// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    counter::Counter,
    recovery::{
        congestion_controller::{self, CongestionController},
        cubic::{FastRetransmission::*, State::*},
        hybrid_slow_start::HybridSlowStart,
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

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3
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
    CongestionAvoidance(Timestamp),
}

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3.2
//# If the congestion window is reduced immediately, a
//# single packet can be sent prior to reduction.  This speeds up loss
//# recovery if the data in the lost packet is retransmitted and is
//# similar to TCP as described in Section 5 of [RFC6675].
#[derive(Clone, Debug, PartialEq, Eq)]
enum FastRetransmission {
    Idle,
    RequiresTransmission,
}

/// A congestion controller that implements "CUBIC for Fast Long-Distance Networks"
/// as specified in https://tools.ietf.org/html/rfc8312. The Hybrid Slow Start algorithm
/// is used for determining the slow start threshold.
#[derive(Clone, Debug)]
pub struct CubicCongestionController {
    cubic: Cubic,
    //= https://tools.ietf.org/rfc/rfc8312#4.8
    //# CUBIC MUST employ a slow-start algorithm, when the cwnd is no more
    //# than ssthresh.

    //= https://tools.ietf.org/rfc/rfc8312#4.8
    //# Among the slow-start algorithms, CUBIC MAY choose the
    //# standard TCP slow start [RFC5681] in general networks, or the limited
    //# slow start [RFC3742] or hybrid slow start [HR08] for fast and long-
    //# distance networks.
    slow_start: HybridSlowStart,
    max_datagram_size: u16,
    congestion_window: f32,
    state: State,
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#B.2
    //# The sum of the size in bytes of all sent packets
    //# that contain at least one ack-eliciting or PADDING frame, and have
    //# not been acknowledged or declared lost.  The size does not include
    //# IP or UDP overhead, but does include the QUIC header and AEAD
    //# overhead.  Packets only containing ACK frames do not count towards
    //# bytes_in_flight to ensure congestion control does not impede
    //# congestion feedback.
    bytes_in_flight: BytesInFlight,
    time_of_last_sent_packet: Option<Timestamp>,
    under_utilized: bool,
}

type BytesInFlight = Counter<u32>;

impl CongestionController for CubicCongestionController {
    fn congestion_window(&self) -> u32 {
        self.congestion_window as u32
    }

    fn is_congestion_limited(&self) -> bool {
        let available_congestion_window = self
            .congestion_window()
            .saturating_sub(*self.bytes_in_flight);
        available_congestion_window < self.max_datagram_size as u32
    }

    fn requires_fast_retransmission(&self) -> bool {
        matches!(self.state, Recovery(_, RequiresTransmission))
    }

    fn on_packet_sent(&mut self, time_sent: Timestamp, bytes_sent: usize) {
        self.bytes_in_flight
            .try_add(bytes_sent)
            .expect("bytes sent should not exceed u32::MAX");

        self.under_utilized = self.is_congestion_window_under_utilized();

        if self.under_utilized {
            if let CongestionAvoidance(ref mut avoidance_start_time) = self.state {
                //= https://tools.ietf.org/rfc/rfc8312#5.8
                //# CUBIC does not raise its congestion window size if the flow is
                //# currently limited by the application instead of the congestion
                //# window.  In case of long periods when cwnd has not been updated due
                //# to the application rate limit, such as idle periods, t in Eq. 1 MUST
                //# NOT include these periods; otherwise, W_cubic(t) might be very high
                //# after restarting from these periods.

                // Since we are application limited, we shift the start time of CongestionAvoidance
                // by the app limited duration, to avoid including that duration in W_cubic(t).
                let last_time_sent = self.time_of_last_sent_packet.unwrap_or(time_sent);
                // Use the later of the last time sent and the avoidance start time to not count
                // the app limited time spent prior to entering congestion avoidance.
                let app_limited_duration = time_sent - last_time_sent.max(*avoidance_start_time);

                *avoidance_start_time += app_limited_duration;
            }
        }

        if let Recovery(recovery_start_time, RequiresTransmission) = self.state {
            // A packet has been sent since we entered recovery (fast retransmission)
            // so flip the state back to idle.
            self.state = Recovery(recovery_start_time, Idle);
        }

        self.time_of_last_sent_packet = Some(time_sent);
    }

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

    fn on_packet_ack(
        &mut self,
        largest_acked_time_sent: Timestamp,
        sent_bytes: usize,
        rtt_estimator: &RttEstimator,
        ack_receive_time: Timestamp,
    ) {
        self.bytes_in_flight
            .try_sub(sent_bytes)
            .expect("sent bytes should not exceed u32::MAX");

        if self.under_utilized {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.8
            //# When bytes in flight is smaller than the congestion window and
            //# sending is not pacing limited, the congestion window is under-
            //# utilized.  When this occurs, the congestion window SHOULD NOT be
            //# increased in either slow start or congestion avoidance.  This can
            //# happen due to insufficient application data or flow control limits.
            return;
        }

        // Check if this ack causes the controller to exit recovery
        if let State::Recovery(recovery_start_time, _) = self.state {
            if largest_acked_time_sent > recovery_start_time {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3.2
                //# A recovery period ends and the sender enters congestion avoidance
                //# when a packet sent during the recovery period is acknowledged.
                self.state = State::CongestionAvoidance(ack_receive_time)
            }
        };

        match self.state {
            SlowStart => {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3.1
                //# While a sender is in slow start, the congestion window increases by
                //# the number of bytes acknowledged when each acknowledgment is
                //# processed.  This results in exponential growth of the congestion
                //# window.
                self.congestion_window += sent_bytes as f32;

                if self.congestion_window >= self.slow_start.threshold {
                    //= https://tools.ietf.org/rfc/rfc8312.txt#4.8
                    //# In the case when CUBIC runs the hybrid slow start [HR08], it may exit
                    //# the first slow start without incurring any packet loss and thus W_max
                    //# is undefined.  In this special case, CUBIC switches to congestion
                    //# avoidance and increases its congestion window size using Eq. 1, where
                    //# t is the elapsed time since the beginning of the current congestion
                    //# avoidance, K is set to 0, and W_max is set to the congestion window
                    //# size at the beginning of the current congestion avoidance.
                    self.state = State::CongestionAvoidance(ack_receive_time);
                    self.cubic.on_slow_start_exit(self.congestion_window);
                }
            }
            Recovery(_, _) => {
                // Don't increase the congestion window while in recovery
            }
            CongestionAvoidance(avoidance_start_time) => {
                //= https://tools.ietf.org/rfc/rfc8312.txt#4.1
                //# t is the elapsed time from the beginning of the current congestion avoidance
                let t = ack_receive_time - avoidance_start_time;

                //= https://tools.ietf.org/rfc/rfc8312.txt#4.1
                //# RTT is the weighted average RTT
                // TODO: Linux Kernel Cubic implementation uses min RTT, possibly
                //      because it is more stable than smoothed_rtt. Other implementations
                //      have followed Linux's choice, so we will as well. The end result is a more
                //      conservative rate of increase of the congestion window. This requires
                //      investigation and testing to evaluate if smoothed_rtt would be a better input.
                let rtt = rtt_estimator.min_rtt();

                self.congestion_avoidance(t, rtt, sent_bytes);
            }
        };

        debug_assert!(self.congestion_window >= self.cubic.minimum_window());
    }

    fn on_packets_lost(
        &mut self,
        lost_bytes: u32,
        persistent_congestion: bool,
        timestamp: Timestamp,
    ) {
        debug_assert!(lost_bytes > 0);

        self.bytes_in_flight -= lost_bytes;
        self.on_congestion_event(timestamp);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.2
        //# When persistent congestion is declared, the sender's congestion
        //# window MUST be reduced to the minimum congestion window
        //# (kMinimumWindow), similar to a TCP sender's response on an RTO
        //# ([RFC5681]).
        if persistent_congestion {
            self.congestion_window = self.cubic.minimum_window();
            self.state = State::SlowStart;
            self.cubic.reset();
        }
    }

    fn on_congestion_event(&mut self, event_time: Timestamp) {
        // No reaction if already in a recovery period.
        if matches!(self.state, Recovery(_, _)) {
            return;
        }

        // Enter recovery period.

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3.1
        //# The sender MUST exit slow start and enter a recovery period when a
        //# packet is lost or when the ECN-CE count reported by its peer
        //# increases.

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3.2
        //# If the congestion window is reduced immediately, a
        //# single packet can be sent prior to reduction.  This speeds up loss
        //# recovery if the data in the lost packet is retransmitted and is
        //# similar to TCP as described in Section 5 of [RFC6675].
        self.state = Recovery(event_time, RequiresTransmission);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3.2
        //# Implementations MAY reduce the congestion window immediately upon
        //# entering a recovery period or use other mechanisms, such as
        //# Proportional Rate Reduction ([PRR]), to reduce the congestion window
        //# more gradually.

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.2
        //# The minimum congestion window is the smallest value the congestion
        //# window can decrease to as a response to loss, increase in the peer-
        //# reported ECN-CE count, or persistent congestion.
        self.congestion_window = self.cubic.multiplicative_decrease(self.congestion_window);

        // Update Hybrid Slow Start with the decreased congestion window.
        self.slow_start.on_congestion_event(self.congestion_window);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.2
    //# If the maximum datagram size changes during the connection, the
    //# initial congestion window SHOULD be recalculated with the new size.
    //# If the maximum datagram size is decreased in order to complete the
    //# handshake, the congestion window SHOULD be set to the new initial
    //# congestion window.

    //= https://tools.ietf.org/rfc/rfc8899.txt#3
    //# An update to the PLPMTU (or MPS) MUST NOT increase the congestion
    //# window measured in bytes [RFC4821].

    //= https://tools.ietf.org/rfc/rfc8899.txt#3
    //# A PL that maintains the congestion window in terms of a limit to
    //# the number of outstanding fixed-size packets SHOULD adapt this
    //# limit to compensate for the size of the actual packets.
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

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.4
    //# When packet protection keys are discarded (see Section 4.8 of
    //# [QUIC-TLS]), all packets that were sent with those keys can no longer
    //# be acknowledged because their acknowledgements cannot be processed
    //# anymore.  The sender MUST discard all recovery state associated with
    //# those packets and MUST remove them from the count of bytes in flight.
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
}

impl CubicCongestionController {
    pub fn new(max_datagram_size: u16) -> Self {
        Self {
            cubic: Cubic::new(max_datagram_size),
            slow_start: HybridSlowStart::new(max_datagram_size),
            max_datagram_size,
            congestion_window: CubicCongestionController::initial_window(max_datagram_size) as f32,
            state: SlowStart,
            bytes_in_flight: Counter::new(0),
            time_of_last_sent_packet: None,
            under_utilized: true,
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.2
    //# Endpoints SHOULD use an initial congestion window of 10 times the
    //# maximum datagram size (max_datagram_size), limited to the larger
    //# of 14720 bytes or twice the maximum datagram size.
    fn initial_window(max_datagram_size: u16) -> u32 {
        const INITIAL_WINDOW_LIMIT: u32 = 14720;
        min(
            10 * max_datagram_size as u32,
            max(INITIAL_WINDOW_LIMIT, 2 * max_datagram_size as u32),
        )
    }

    fn congestion_avoidance(&mut self, t: Duration, rtt: Duration, sent_bytes: usize) {
        let w_cubic = self.cubic.w_cubic(t);
        let w_est = self.cubic.w_est(t, rtt);
        // limit the window increase to half the acked bytes
        // as the Linux implementation of Cubic does.
        let max_cwnd = self.congestion_window + sent_bytes as f32 / 2.0;

        if w_cubic < w_est {
            // TCP-Friendly Region
            //= https://tools.ietf.org/rfc/rfc8312#4.2
            //# When receiving an ACK in congestion avoidance (cwnd could be greater than
            //# or less than W_max), CUBIC checks whether W_cubic(t) is less than
            //# W_est(t).  If so, CUBIC is in the TCP-friendly region and cwnd SHOULD
            //# be set to W_est(t) at each reception of an ACK.
            self.congestion_window = self.packets_to_bytes(w_est).min(max_cwnd);
        } else {
            //= https://tools.ietf.org/rfc/rfc8312#4.1
            //# Upon receiving an ACK during congestion avoidance, CUBIC computes the
            //# window increase rate during the next RTT period using Eq. 1.  It sets
            //# W_cubic(t+RTT) as the candidate target value of the congestion
            //# window

            // Concave Region
            //= https://tools.ietf.org/rfc/rfc8312#4.3
            //# When receiving an ACK in congestion avoidance, if CUBIC is not in the
            //# TCP-friendly region and cwnd is less than W_max, then CUBIC is in the
            //# concave region.  In this region, cwnd MUST be incremented by
            //# (W_cubic(t+RTT) - cwnd)/cwnd for each received ACK, where
            //# W_cubic(t+RTT) is calculated using Eq. 1.

            // Convex Region
            //# https://tools.ietf.org/rfc/rfc8312#4.4
            //# When receiving an ACK in congestion avoidance, if CUBIC is not in the
            //# TCP-friendly region and cwnd is larger than or equal to W_max, then
            //# CUBIC is in the convex region.

            //= https://tools.ietf.org/rfc/rfc8312#4.4
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

    fn packets_to_bytes(&self, cwnd: f32) -> f32 {
        cwnd * self.max_datagram_size as f32
    }

    /// Returns true if the congestion window is under utilized and should not grow larger
    /// without further evidence of the stability of the current window.
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
    //= https://tools.ietf.org/rfc/rfc8312#4.1
    //# W_max is the window size just before the window is
    //# reduced in the last congestion event.
    w_max: f32,
    //= https://tools.ietf.org/rfc/rfc8312#4.6
    //# a flow remembers the last value of W_max before it
    //# updates W_max for the current congestion event.
    //# Let us call the last value of W_max to be W_last_max.
    w_last_max: f32,
    // k is the time until we expect to reach w_max
    k: Duration,
    max_datagram_size: u16,
}

//= https://tools.ietf.org/rfc/rfc8312#5.1
//# Based on these observations and our experiments, we find C=0.4
//# gives a good balance between TCP-friendliness and aggressiveness
//# of window increase.  Therefore, C SHOULD be set to 0.4.
const C: f32 = 0.4;

//= https://tools.ietf.org/rfc/rfc8312#4.5
//# Parameter beta_cubic SHOULD be set to 0.7.
const BETA_CUBIC: f32 = 0.7;

impl Cubic {
    pub fn new(max_datagram_size: u16) -> Self {
        Cubic {
            w_max: 0.0,
            w_last_max: 0.0,
            k: Duration::default(),
            max_datagram_size,
        }
    }

    /// Reset to the original state
    pub fn reset(&mut self) {
        self.w_max = 0.0;
        self.w_last_max = 0.0;
        self.k = Duration::default();
    }

    //= https://tools.ietf.org/rfc/rfc8312#4.1
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
    fn w_cubic(&self, t: Duration) -> f32 {
        C * (t.as_secs_f32() - self.k.as_secs_f32()).powf(3.0) + self.w_max as f32
    }

    //= https://tools.ietf.org/rfc/rfc8312#4.2
    //# W_est(t) = W_max*beta_cubic +
    //               [3*(1-beta_cubic)/(1+beta_cubic)] * (t/RTT) (Eq. 4)
    fn w_est(&self, t: Duration, rtt: Duration) -> f32 {
        self.w_max.mul_add(
            BETA_CUBIC,
            (3.0 * (1.0 - BETA_CUBIC) / (1.0 + BETA_CUBIC)) * (t.as_secs_f32() / rtt.as_secs_f32()),
        )
    }

    //= https://tools.ietf.org/rfc/rfc8312#4.5
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
    fn multiplicative_decrease(&mut self, cwnd: f32) -> f32 {
        self.w_max = self.bytes_to_packets(cwnd);

        //= https://tools.ietf.org/rfc/rfc8312#4.6
        //# To speed up this bandwidth release by
        //# existing flows, the following mechanism called "fast convergence"
        //# SHOULD be implemented.

        //= https://tools.ietf.org/rfc/rfc8312#4.6
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

    //= https://tools.ietf.org/rfc/rfc8312#4.8
    //# In the case when CUBIC runs the hybrid slow start [HR08], it may exit
    //# the first slow start without incurring any packet loss and thus W_max
    //# is undefined.  In this special case, CUBIC switches to congestion
    //# avoidance and increases its congestion window size using Eq. 1, where
    //# t is the elapsed time since the beginning of the current congestion
    //# avoidance, K is set to 0, and W_max is set to the congestion window
    //# size at the beginning of the current congestion avoidance.
    fn on_slow_start_exit(&mut self, cwnd: f32) {
        self.w_max = self.bytes_to_packets(cwnd);

        // We are currently at the w_max, so set k to zero indicating zero
        // seconds to reach the max
        self.k = Duration::from_secs(0);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.2
    //# The minimum congestion window is the smallest value the congestion
    //# window can decrease to as a response to loss, increase in the peer-
    //# reported ECN-CE count, or persistent congestion.  The RECOMMENDED
    //# value is 2 * max_datagram_size.
    fn minimum_window(&self) -> f32 {
        2.0 * self.max_datagram_size as f32
    }

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
mod test {
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
    //= https://tools.ietf.org/rfc/rfc8312.txt#4.1
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

        //= https://tools.ietf.org/rfc/rfc8312#5.1
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
    //= https://tools.ietf.org/rfc/rfc8312.txt#4.6
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
    //= https://tools.ietf.org/rfc/rfc8312.txt#4.5
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
        //= https://tools.ietf.org/rfc/rfc8312#4.6
        //= type=test
        //# To speed up this bandwidth release by
        //# existing flows, the following mechanism called "fast convergence"
        //# SHOULD be implemented.
        // Window max was less than the last max, so fast convergence applies
        assert_delta!(cubic.w_last_max, 80000.0 / max_datagram_size, 0.001);
        // W_max = W_max*(1.0+beta_cubic)/2.0 = W_max * .85
        assert_delta!(cubic.w_max, 80000.0 * 0.85 / max_datagram_size, 0.001);

        //= https://tools.ietf.org/rfc/rfc8312#4.5
        //= type=test
        //# Parameter beta_cubic SHOULD be set to 0.7.
        assert_eq!(0.7, BETA_CUBIC);
    }

    #[test]
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.8
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

        cc.state = CongestionAvoidance(NoopClock.get_time());
        assert!(cc.is_congestion_window_under_utilized());

        // In Congestion Avoidance, the window is under utilized if there are more than
        // 3 * MTU bytes available in the congestion window (12000 - 3 * 1200 = 8400)
        cc.bytes_in_flight = BytesInFlight::new(8399);
        assert!(cc.is_congestion_window_under_utilized());

        cc.bytes_in_flight = BytesInFlight::new(8400);
        assert!(!cc.is_congestion_window_under_utilized());
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.2
    //= type=test
    //# Endpoints SHOULD use an initial congestion
    //# window of 10 times the maximum datagram size (max_datagram_size),
    //# limited to the larger of 14720 bytes or twice the maximum datagram
    //# size.
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

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.2
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
        cc.on_packet_sent(now + Duration::from_secs(10), 1);

        assert_eq!(cc.bytes_in_flight, 1);

        // Latest RTT is 100ms
        rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(100),
            now,
            true,
            PacketNumberSpace::ApplicationData,
        );

        //= https://tools.ietf.org/rfc/rfc8312#4.8
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
        cc.on_packet_sent(now + Duration::from_secs(20), 1);

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
        let now = NoopClock.get_time();

        cc.congestion_window = 100_000.0;
        cc.bytes_in_flight = BytesInFlight::new(92_500);
        cc.state = SlowStart;

        // t0: Send a packet in Slow Start
        cc.on_packet_sent(now, 1000);

        assert_eq!(cc.bytes_in_flight, 93_500);
        assert_eq!(cc.time_of_last_sent_packet, Some(now));

        // t10: Enter Congestion Avoidance
        cc.state = CongestionAvoidance(now + Duration::from_secs(10));

        // t15: Send a packet in Congestion Avoidance
        cc.on_packet_sent(now + Duration::from_secs(15), 1000);

        assert_eq!(cc.bytes_in_flight, 94_500);
        assert_eq!(
            cc.time_of_last_sent_packet,
            Some(now + Duration::from_secs(15))
        );
        // Application limited, but the last sent packet was sent before CongestionAvoidance,
        // so the CongestionAvoidance increases by the time from avoidance start to now
        assert_eq!(cc.state, CongestionAvoidance(now + Duration::from_secs(15)));

        cc.bytes_in_flight = BytesInFlight::new(93500);

        // t25: Send a packet in Congestion Avoidance
        cc.on_packet_sent(now + Duration::from_secs(25), 1000);

        // Application limited so the CongestionAvoidance start moves up by 10 seconds
        // (time_of_last_sent_packet - time_sent)
        assert_eq!(cc.state, CongestionAvoidance(now + Duration::from_secs(25)));
    }

    #[test]
    fn on_packet_sent_fast_retransmission() {
        let mut cc = CubicCongestionController::new(1000);
        let now = NoopClock.get_time();

        cc.congestion_window = 100_000.0;
        cc.bytes_in_flight = BytesInFlight::new(99900);
        cc.state = Recovery(now, RequiresTransmission);

        cc.on_packet_sent(now + Duration::from_secs(10), 100);

        assert_eq!(cc.state, Recovery(now, Idle));
    }

    //= https://tools.ietf.org/rfc/rfc8312#5.8
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

        cc.congestion_window = 6000.0;
        cc.bytes_in_flight = BytesInFlight::new(0);
        cc.state = SlowStart;

        // t0: Send a packet in Slow Start
        cc.on_packet_sent(now, 1000);

        assert_eq!(cc.bytes_in_flight, 1000);

        // t10: Enter Congestion Avoidance
        cc.cubic.w_max = 6.0;
        cc.state = CongestionAvoidance(now + Duration::from_secs(10));

        // t15: Send a packet in Congestion Avoidance while under utilized
        cc.on_packet_sent(now + Duration::from_secs(15), 1000);
        assert!(cc.is_congestion_window_under_utilized());

        // t15: Send a packet in Congestion Avoidance while not under utilized
        cc.on_packet_sent(now + Duration::from_secs(15), 1000);
        assert!(!cc.is_congestion_window_under_utilized());

        assert_eq!(cc.bytes_in_flight, 3000);

        // t16: Ack a packet in Congestion Avoidance
        cc.on_packet_ack(now, 1000, rtt_estimator, now + Duration::from_secs(16));

        assert_eq!(cc.bytes_in_flight, 2000);

        // Verify congestion avoidance start time was moved from t10 to t15 to account
        // for the 5 seconds of under utilized time
        assert_eq!(cc.state, CongestionAvoidance(now + Duration::from_secs(15)));
    }

    #[test]
    fn congestion_avoidance_after_fast_convergence() {
        let max_datagram_size = 1200;
        let mut cc = CubicCongestionController::new(max_datagram_size);
        let now = NoopClock.get_time();
        cc.bytes_in_flight = BytesInFlight::new(100);
        cc.congestion_window = 80_000.0;
        cc.cubic.w_last_max = bytes_to_packets(100_000.0, max_datagram_size);

        cc.on_packets_lost(100, false, now);
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
            cc.cubic.w_cubic(Duration::from_secs(0))
                < bytes_to_packets(prev_cwnd, max_datagram_size)
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
        cc.congestion_window = 100_000.0;
        cc.bytes_in_flight = BytesInFlight::new(100_000);
        cc.state = SlowStart;

        cc.on_packets_lost(100, false, now + Duration::from_secs(10));

        assert_eq!(cc.bytes_in_flight, 100_000u32 - 100);
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3.1
        //= type=test
        //# The sender MUST exit slow start and enter a recovery period when a
        //# packet is lost or when the ECN-CE count reported by its peer
        //# increases.
        assert_eq!(
            cc.state,
            Recovery(now + Duration::from_secs(10), RequiresTransmission)
        );
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3.2
        //= type=test
        //# Implementations MAY reduce the congestion window immediately upon
        //# entering a recovery period or use other mechanisms, such as
        //# Proportional Rate Reduction ([PRR]), to reduce the congestion window
        //# more gradually.
        assert_delta!(cc.congestion_window, 100_000.0 * BETA_CUBIC, 0.001);
        assert_delta!(cc.slow_start.threshold, 100_000.0 * BETA_CUBIC, 0.001);
    }

    #[test]
    fn on_packet_lost_below_minimum_window() {
        let mut cc = CubicCongestionController::new(1000);
        let now = NoopClock.get_time();
        cc.congestion_window = cc.cubic.minimum_window();
        cc.bytes_in_flight = BytesInFlight::new(cc.congestion_window());
        cc.state = CongestionAvoidance(now);

        cc.on_packets_lost(100, false, now + Duration::from_secs(10));

        assert_delta!(cc.congestion_window, cc.cubic.minimum_window(), 0.001);
    }

    #[test]
    fn on_packet_lost_already_in_recovery() {
        let mut cc = CubicCongestionController::new(1000);
        let now = NoopClock.get_time();
        cc.congestion_window = 10000.0;
        cc.bytes_in_flight = BytesInFlight::new(1000);
        cc.state = Recovery(now, Idle);

        // break up on_packet_loss into two call to confirm double call
        // behavior is valid (50 + 50 = 100 lost bytes)
        cc.on_packets_lost(50, false, now);
        cc.on_packets_lost(50, false, now);

        // No change to the congestion window
        assert_delta!(cc.congestion_window, 10000.0, 0.001);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.2
    //= type=test
    //# When persistent congestion is declared, the sender's congestion
    //# window MUST be reduced to the minimum congestion window
    //# (kMinimumWindow), similar to a TCP sender's response on an RTO
    //# ([RFC5681]).
    #[test]
    fn on_packet_lost_persistent_congestion() {
        let mut cc = CubicCongestionController::new(1000);
        let now = NoopClock.get_time();
        cc.congestion_window = 10000.0;
        cc.bytes_in_flight = BytesInFlight::new(1000);
        cc.state = Recovery(now, Idle);

        cc.on_packets_lost(100, true, now);

        assert_eq!(cc.state, SlowStart);
        assert_delta!(cc.congestion_window, cc.cubic.minimum_window(), 0.001);
        assert_delta!(cc.cubic.w_max, 0.0, 0.001);
        assert_delta!(cc.cubic.w_last_max, 0.0, 0.001);
        assert_eq!(cc.cubic.k, Duration::from_millis(0));
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.2
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

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.2
    //= type=test
    //# If the maximum datagram size changes during the connection, the
    //# initial congestion window SHOULD be recalculated with the new size.

    //= https://tools.ietf.org/rfc/rfc8899.txt#3
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

        //= https://tools.ietf.org/rfc/rfc8899.txt#3
        //= type=test
        //# An update to the PLPMTU (or MPS) MUST NOT increase the congestion
        //# window measured in bytes [RFC4821].
        assert_delta!(cc.congestion_window / mtu as f32, cwnd_in_bytes, 0.001);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.4
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

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.8
    //= type=test
    //# When bytes in flight is smaller than the congestion window and
    //# sending is not pacing limited, the congestion window is under-
    //# utilized. When this occurs, the congestion window SHOULD NOT be
    //# increased in either slow start or congestion avoidance.
    #[test]
    fn on_packet_ack_limited() {
        let mut cc = CubicCongestionController::new(5000);
        let now = NoopClock.get_time();
        cc.congestion_window = 100_000.0;
        cc.bytes_in_flight = BytesInFlight::new(10000);
        cc.under_utilized = true;
        cc.state = SlowStart;

        cc.on_packet_ack(now, 1, &RttEstimator::new(Duration::from_secs(0)), now);

        assert_delta!(cc.congestion_window, 100_000.0, 0.001);

        cc.state = CongestionAvoidance(now);

        cc.on_packet_ack(now, 1, &RttEstimator::new(Duration::from_secs(0)), now);

        assert_delta!(cc.congestion_window, 100_000.0, 0.001);
    }

    #[test]
    fn on_packet_ack_utilized_then_under_utilized() {
        let mut cc = CubicCongestionController::new(5000);
        let now = NoopClock.get_time();
        let mut rtt_estimator = RttEstimator::new(Duration::from_secs(0));
        rtt_estimator.update_rtt(
            Duration::from_secs(0),
            Duration::from_millis(200),
            now,
            true,
            PacketNumberSpace::ApplicationData,
        );
        cc.congestion_window = 100_000.0;
        cc.state = SlowStart;

        cc.on_packet_sent(now, 60_000);
        cc.on_packet_ack(now, 50_000, &rtt_estimator, now);
        let cwnd = cc.congestion_window();

        assert!(!cc.under_utilized);
        assert!(cwnd > 100_000);

        // Now the window is under utilized, but we still grow the window until more packets are sent
        assert!(cc.is_congestion_window_under_utilized());
        cc.on_packet_ack(now, 1200, &rtt_estimator, now + Duration::from_millis(100));
        assert!(cc.congestion_window() > cwnd);

        let cwnd = cc.congestion_window();

        // Now the application has had a chance to send more data, but it didn't send enough to
        // utilize the congestion window, so the window does not grow.
        cc.on_packet_sent(now, 1200);
        assert!(cc.under_utilized);
        cc.on_packet_ack(now, 1200, &rtt_estimator, now + Duration::from_millis(201));
        assert_eq!(cc.congestion_window(), cwnd);
    }

    #[test]
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3.2
    //= type=test
    fn on_packet_ack_recovery_to_congestion_avoidance() {
        let mut cc = CubicCongestionController::new(5000);
        let now = NoopClock.get_time();

        cc.cubic.w_max = bytes_to_packets(25000.0, 5000);
        cc.state = Recovery(now, Idle);
        cc.bytes_in_flight = BytesInFlight::new(25000);
        cc.under_utilized = false;

        cc.on_packet_ack(
            now + Duration::from_millis(1),
            1,
            &RttEstimator::new(Duration::from_secs(0)),
            now + Duration::from_millis(2),
        );

        assert_eq!(
            cc.state,
            CongestionAvoidance(now + Duration::from_millis(2))
        );
    }

    #[test]
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.3.2
    //= type=test
    fn on_packet_ack_slow_start_to_congestion_avoidance() {
        let mut cc = CubicCongestionController::new(5000);
        let now = NoopClock.get_time();

        cc.state = SlowStart;
        cc.congestion_window = 10000.0;
        cc.bytes_in_flight = BytesInFlight::new(10000);
        cc.slow_start.threshold = 10050.0;
        cc.under_utilized = false;

        cc.on_packet_ack(
            now,
            100,
            &RttEstimator::new(Duration::from_secs(0)),
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
            CongestionAvoidance(now + Duration::from_millis(2))
        );
    }

    #[test]
    fn on_packet_ack_recovery() {
        let mut cc = CubicCongestionController::new(5000);
        let now = NoopClock.get_time();

        cc.state = Recovery(now, Idle);
        cc.congestion_window = 10000.0;
        cc.bytes_in_flight = BytesInFlight::new(10000);

        cc.on_packet_ack(
            now,
            100,
            &RttEstimator::new(Duration::from_secs(0)),
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

        cc.state = CongestionAvoidance(now + Duration::from_millis(3300));
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

        cc.on_packet_ack(now, 1000, &rtt_estimator, now + Duration::from_millis(4750));

        let t = Duration::from_millis(4750) - Duration::from_millis(3300);
        let rtt = rtt_estimator.min_rtt();

        cc2.congestion_avoidance(t, rtt, 1000);

        assert_delta!(cc.congestion_window, cc2.congestion_window, 0.001);
    }

    //= https://tools.ietf.org/rfc/rfc8312#4.2
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

    //= https://tools.ietf.org/rfc/rfc8312#4.3
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

    //= https://tools.ietf.org/rfc/rfc8312#4.4
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
}
