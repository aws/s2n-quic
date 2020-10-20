use crate::recovery::{
    congestion_controller::CongestionController, cubic::State::*,
    hybrid_slow_start::HybridSlowStart, loss_info::LossInfo,
};
use core::{
    cmp::{max, min},
    ops::{AddAssign, SubAssign},
    time::Duration,
};
use s2n_quic_core::{recovery::RTTEstimator, time::Timestamp};

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.3
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
    Recovery(Timestamp),
    CongestionAvoidance(Timestamp),
}

/// A congestion controller that implements "CUBIC for Fast Long-Distance Networks"
/// as specified in https://tools.ietf.org/html/rfc8312. The Hybrid Slow Start algorithm
/// is used for determining the slow start threshold.
#[derive(Clone)]
struct CubicCongestionController {
    cubic: Cubic,
    slow_start: HybridSlowStart,
    max_datagram_size: usize,
    congestion_window: usize,
    state: State,
    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#B.2
    //# The sum of the size in bytes of all sent packets
    //# that contain at least one ack-eliciting or PADDING frame, and have
    //# not been acknowledged or declared lost.  The size does not include
    //# IP or UDP overhead, but does include the QUIC header and AEAD
    //# overhead.  Packets only containing ACK frames do not count towards
    //# bytes_in_flight to ensure congestion control does not impede
    //# congestion feedback.
    bytes_in_flight: BytesInFlight,
    time_of_last_sent_packet: Option<Timestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct BytesInFlight(usize);

// This implementation of subtraction for BytesInFlight will
// panic if debug assertions are enabled, but will otherwise
// saturate to zero to avoid overflowing and wrapping.
impl SubAssign<usize> for BytesInFlight {
    fn sub_assign(&mut self, rhs: usize) {
        if cfg!(debug_assertions) {
            self.0 -= rhs;
        } else {
            self.0 = self.0.saturating_sub(rhs);
        }
    }
}

impl AddAssign<usize> for BytesInFlight {
    fn add_assign(&mut self, rhs: usize) {
        self.0 = self.0 + rhs;
    }
}

impl CongestionController for CubicCongestionController {
    fn congestion_window(&self) -> usize {
        self.congestion_window
    }

    fn on_packet_sent(&mut self, time_sent: Timestamp, bytes_sent: usize) {
        self.bytes_in_flight += bytes_sent;

        if self.is_under_utilized() {
            if let CongestionAvoidance(ref mut avoidance_start_time) = self.state {
                //= https://tools.ietf.org/rfc/rfc8312.txt#5.8
                //# CUBIC does not raise its congestion window size if the flow is
                //# currently limited by the application instead of the congestion
                //# window.  In case of long periods when cwnd has not been updated due
                //# to the application rate limit, such as idle periods, t in Eq. 1 MUST
                //# NOT include these periods; otherwise, W_cubic(t) might be very high
                //# after restarting from these periods.

                // Since we are application limited, we shift the start time of CongestionAvoidance
                // by the limited duration, to avoid including that duration in W_cubic(t).
                let last_time_sent = self.time_of_last_sent_packet.unwrap_or(time_sent);
                *avoidance_start_time += time_sent - last_time_sent;
            }
        }

        self.time_of_last_sent_packet = Some(time_sent);
    }

    fn on_rtt_update(&mut self, time_sent: Timestamp, rtt_estimator: &RTTEstimator) {
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
        rtt_estimator: &RTTEstimator,
        ack_receive_time: Timestamp,
    ) {
        // Check if the congestion window is under utilized before updating bytes in flight
        let under_utilized = self.is_under_utilized();
        self.bytes_in_flight -= sent_bytes;

        if under_utilized {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.8
            //# When bytes in flight is smaller than the congestion window and
            //# sending is not pacing limited, the congestion window is under-
            //# utilized.  When this occurs, the congestion window SHOULD NOT be
            //# increased in either slow start or congestion avoidance.  This can
            //# happen due to insufficient application data or flow control limits.
            return;
        }

        // Check if this ack causes the controller to exit recovery
        if let State::Recovery(recovery_start_time) = self.state {
            if largest_acked_time_sent > recovery_start_time {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.3.2
                //# A recovery period ends and the sender enters congestion avoidance
                //# when a packet sent during the recovery period is acknowledged.
                self.state = State::CongestionAvoidance(ack_receive_time)
            }
        };

        match self.state {
            SlowStart => {
                // Slow start
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.3.1
                //# While a sender is in slow start, the congestion window increases by
                //# the number of bytes acknowledged when each acknowledgment is
                //# processed.  This results in exponential growth of the congestion
                //# window.
                self.congestion_window += sent_bytes;

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
                    self.cubic.update_w_max(self.congestion_window);
                }
            }
            Recovery(_) => {
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
    }

    fn on_packets_lost(
        &mut self,
        loss_info: LossInfo,
        persistent_congestion_threshold: Duration,
        timestamp: Timestamp,
    ) {
        self.bytes_in_flight -= loss_info.bytes_in_flight;
        self.on_congestion_event(timestamp);

        // Reset the congestion window if the loss of these
        // packets indicates persistent congestion.
        if loss_info.persistent_congestion_period > persistent_congestion_threshold {
            self.congestion_window = self.minimum_window();
            self.state = State::SlowStart;
        }
    }

    fn on_congestion_event(&mut self, event_time: Timestamp) {
        // No reaction if already in a recovery period.
        if matches!(self.state, Recovery(_)) {
            return;
        }

        // Enter recovery period.
        self.state = Recovery(event_time);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.3.2
        //# On entering a recovery period, a sender MUST set the slow start threshold
        //# to half the value of the congestion window when loss is detected. The
        //# congestion window MUST be set to the reduced value of the slow start
        //# threshold before exiting the recovery period.
        // Since this is CUBIC and not NewReno, the slow start threshold is
        // set according to CUBIC.

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.3.2
        //# Implementations MAY reduce the congestion window immediately
        //# upon entering a recovery period
        self.congestion_window = self.cubic.multiplicative_decrease(self.congestion_window);
        // Update Hybrid Slow Start with the decreased congestion window.
        self.slow_start.on_congestion_event(self.congestion_window);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.3.2
        //# If the congestion window is reduced immediately, a
        //# single packet can be sent prior to reduction.  This speeds up loss
        //# recovery if the data in the lost packet is retransmitted and is
        //# similar to TCP as described in Section 5 of [RFC6675].
        // TODO: https://github.com/awslabs/s2n-quic/issues/138
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.2
    //# If the maximum datagram size changes during the connection, the
    //# initial congestion window SHOULD be recalculated with the new size.
    //# If the maximum datagram size is decreased in order to complete the
    //# handshake, the congestion window SHOULD be set to the new initial
    //# congestion window.
    fn on_mtu_update(&mut self, max_datagram_size: usize) {
        let old_max_datagram_size = self.max_datagram_size;
        self.max_datagram_size = max_datagram_size;
        self.cubic.max_datagram_size = max_datagram_size;

        if max_datagram_size < old_max_datagram_size {
            self.congestion_window = CubicCongestionController::initial_window(max_datagram_size);
        } else {
            self.congestion_window =
                (self.congestion_window / old_max_datagram_size) * max_datagram_size;
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#6.4
    //# When packet protection keys are discarded (see Section 4.8 of
    //# [QUIC-TLS]), all packets that were sent with those keys can no longer
    //# be acknowledged because their acknowledgements cannot be processed
    //# anymore.  The sender MUST discard all recovery state associated with
    //# those packets and MUST remove them from the count of bytes in flight.
    fn on_packet_discarded(&mut self, bytes_sent: usize) {
        self.bytes_in_flight -= bytes_sent;
    }
}

impl CubicCongestionController {
    // TODO: Remove when used
    #[allow(dead_code)]
    pub fn new(max_datagram_size: usize) -> Self {
        Self {
            cubic: Cubic::new(max_datagram_size),
            slow_start: HybridSlowStart::new(max_datagram_size),
            max_datagram_size,
            congestion_window: CubicCongestionController::initial_window(max_datagram_size),
            state: SlowStart,
            bytes_in_flight: BytesInFlight(0),
            time_of_last_sent_packet: None,
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.2
    //# Endpoints SHOULD use an initial congestion window of 10 times the
    //# maximum datagram size (max_datagram_size), limited to the larger
    //# of 14720 bytes or twice the maximum datagram size.
    fn initial_window(max_datagram_size: usize) -> usize {
        const INITIAL_WINDOW_LIMIT: usize = 14720;
        min(
            10 * max_datagram_size,
            max(INITIAL_WINDOW_LIMIT, 2 * max_datagram_size),
        )
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.2
    //# The minimum congestion window is the smallest value the congestion
    //# window can decrease to as a response to loss, ECN-CE, or persistent
    //# congestion.  The RECOMMENDED value is 2 * max_datagram_size.
    fn minimum_window(&self) -> usize {
        2 * self.max_datagram_size
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.8
    //# When bytes in flight is smaller than the congestion window and
    //# sending is not pacing limited, the congestion window is under-
    //# utilized.
    fn is_under_utilized(&self) -> bool {
        self.bytes_in_flight.0 < self.congestion_window
    }

    fn congestion_avoidance(&mut self, t: Duration, rtt: Duration, sent_bytes: usize) {
        let w_cubic = self.cubic.w_cubic(t);
        let w_est = self.cubic.w_est(t, rtt);

        if w_cubic < w_est {
            // TCP-Friendly Region
            //= https://tools.ietf.org/rfc/rfc8312.txt#4.2
            //# When receiving an ACK in congestion avoidance (cwnd could be greater than
            //# or less than W_max), CUBIC checks whether W_cubic(t) is less than
            //# W_est(t).  If so, CUBIC is in the TCP-friendly region and cwnd SHOULD
            //# be set to W_est(t) at each reception of an ACK.
            self.congestion_window = self.packets_to_bytes(self.cubic.w_est(t, rtt));
        } else {
            //= https://tools.ietf.org/rfc/rfc8312.txt#4.1
            //# Upon receiving an ACK during congestion avoidance, CUBIC computes the
            //# window increase rate during the next RTT period using Eq. 1.  It sets
            //# W_cubic(t+RTT) as the candidate target value of the congestion
            //# window

            // Concave Region
            //= https://tools.ietf.org/rfc/rfc8312.txt#4.3
            //# When receiving an ACK in congestion avoidance, if CUBIC is not in the
            //# TCP-friendly region and cwnd is less than W_max, then CUBIC is in the
            //# concave region.  In this region, cwnd MUST be incremented by
            //# (W_cubic(t+RTT) - cwnd)/cwnd for each received ACK

            // Convex Region
            //# https://tools.ietf.org/rfc/rfc8312.txt#4.4
            //# When receiving an ACK in congestion avoidance, if CUBIC is not in the
            //# TCP-friendly region and cwnd is larger than or equal to W_max, then
            //# CUBIC is in the convex region.

            //= https://tools.ietf.org/rfc/rfc8312.txt#4.4
            //# In this region, cwnd MUST be incremented by
            //# (W_cubic(t+RTT) - cwnd)/cwnd for each received ACK

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
            let window_increase_rate = (target_congestion_window - self.congestion_window) as f32
                / self.congestion_window as f32;
            // Convert the increase rate to bytes and limit to half the acked bytes as the Linux
            // implementation of Cubic does.
            let window_increment = min(self.packets_to_bytes(window_increase_rate), sent_bytes / 2);

            self.congestion_window += window_increment;
        }
    }

    fn packets_to_bytes(&self, cwnd: f32) -> usize {
        (cwnd * self.max_datagram_size as f32) as usize
    }
}

/// Core functions of "CUBIC for Fast Long-Distance Networks" as specified in
/// https://tools.ietf.org/html/rfc8312. The unit of all window sizes is in
/// packets of size max_datagram_size to maintain alignment with the specification.
/// Thus, window sizes should be converted to bytes before applying to the
/// congestion window in the congestion controller.
#[derive(Clone, Debug)]
struct Cubic {
    //= https://tools.ietf.org/rfc/rfc8312.txt#4.1
    //# W_max is the window size just before the window is
    //# reduced in the last congestion event.
    w_max: f32,
    //= https://tools.ietf.org/rfc/rfc8312.txt#4.6
    //# a flow remembers the last value of W_max before it
    //# updates W_max for the current congestion event.
    //# Let us call the last value of W_max to be W_last_max.
    w_last_max: f32,
    // k is the time until we expect to reach w_max
    k: Duration,
    max_datagram_size: usize,
}

//= https://tools.ietf.org/rfc/rfc8312.txt#5.1
//# Based on these observations and our experiments, we find C=0.4
//# gives a good balance between TCP-friendliness and aggressiveness
//# of window increase.  Therefore, C SHOULD be set to 0.4.
const C: f32 = 0.4;

//= https://tools.ietf.org/rfc/rfc8312.txt#4.5
//# Parameter beta_cubic SHOULD be set to 0.7.
const BETA_CUBIC: f32 = 0.7;

impl Cubic {
    pub fn new(max_datagram_size: usize) -> Self {
        Cubic {
            w_max: 0.0,
            w_last_max: 0.0,
            k: Duration::default(),
            max_datagram_size,
        }
    }

    //= https://tools.ietf.org/rfc/rfc8312.txt#4.1
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

    //= https://tools.ietf.org/rfc/rfc8312.txt#4.2
    //# W_est(t) = W_max*beta_cubic +
    //               [3*(1-beta_cubic)/(1+beta_cubic)] * (t/RTT) (Eq. 4)
    fn w_est(&self, t: Duration, rtt: Duration) -> f32 {
        self.w_max.mul_add(
            BETA_CUBIC,
            (3.0 * (1.0 - BETA_CUBIC) / (1.0 + BETA_CUBIC)) * (t.as_secs_f32() / rtt.as_secs_f32()),
        )
    }

    //= https://tools.ietf.org/rfc/rfc8312.txt#4.5
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
    fn multiplicative_decrease(&mut self, cwnd: usize) -> usize {
        self.update_w_max(cwnd);
        (cwnd as f32 * BETA_CUBIC) as usize
    }

    //= https://tools.ietf.org/rfc/rfc8312.txt#4.6
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
    fn update_w_max(&mut self, cwnd: usize) {
        self.w_max = cwnd as f32 / self.max_datagram_size as f32;

        if self.w_max < self.w_last_max {
            self.w_last_max = self.w_max;
            self.w_max = self.w_max * (1.0 + BETA_CUBIC) / 2.0;
        } else {
            self.w_last_max = self.w_max;
        }

        // Update k since it only varies on w_max
        self.k = Duration::from_secs_f32((self.w_max * (1.0 - BETA_CUBIC) / C).cbrt());
    }
}

#[cfg(test)]
mod test {
    use crate::recovery::{
        cubic::{
            BytesInFlight, Cubic, CubicCongestionController,
            State::{CongestionAvoidance, Recovery, SlowStart},
            BETA_CUBIC,
        },
        CongestionController,
    };
    use s2n_quic_core::{
        packet::number::PacketNumberSpace,
        recovery::{loss_info::LossInfo, RTTEstimator},
        time::Duration,
    };

    macro_rules! assert_delta {
        ($x:expr, $y:expr, $d:expr) => {
            if !($x - $y < $d || $y - $x < $d) {
                panic!();
            }
        };
    }

    #[test]
    #[compliance::tests("https://tools.ietf.org/rfc/rfc8312.txt#4.1")]
    fn w_cubic() {
        let max_datagram_size = 1200.0;
        let mut cubic = Cubic::new(max_datagram_size as usize);

        cubic.update_w_max(2_764_800);
        assert_delta!(cubic.w_max, 2_764_800.0 / max_datagram_size, 0.001);

        let mut t = Duration::from_secs(0);

        // W_cubic(0)=W_max*beta_cubic
        assert_delta!(cubic.w_max * BETA_CUBIC, cubic.w_cubic(t), 0.001);

        // K = cubic_root(W_max*(1-beta_cubic)/C)
        // K = cubic_root(2304 * 0.75) = 12
        assert_eq!(cubic.k, Duration::from_secs(12));

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
    #[compliance::tests("https://tools.ietf.org/rfc/rfc8312.txt#4.6")]
    fn w_est() {
        let max_datagram_size = 1200.0;
        let mut cubic = Cubic::new(max_datagram_size as usize);
        cubic.w_max = 100.0;
        let t = Duration::from_secs(6);
        let rtt = Duration::from_millis(300);

        // W_est(t) = W_max*beta_cubic + [3*(1-beta_cubic)/(1+beta_cubic)] * (t/RTT)
        // W_est(6) = 100*.7 + [3*(1-.7)/(1+.7)] * (6/.3)
        // W_est(6) = 70 + 0.5294117647 * 20 = 80.588235294

        assert_delta!(cubic.w_est(t, rtt), 80.5882, 0.001);
    }

    #[test]
    #[compliance::tests("https://tools.ietf.org/rfc/rfc8312.txt#4.5")]
    fn multiplicative_decrease() {
        let max_datagram_size = 1200.0;
        let mut cubic = Cubic::new(max_datagram_size as usize);
        cubic.update_w_max(10000);

        assert_eq!(
            cubic.multiplicative_decrease(100_000),
            (100_000.0 * BETA_CUBIC) as usize
        );
        // Window max was not less than the last max, so not fast convergence
        assert_delta!(cubic.w_last_max, cubic.w_max, 0.001);
        assert_delta!(cubic.w_max, 100_000.0 / max_datagram_size, 0.001);

        assert_eq!(
            cubic.multiplicative_decrease(80000),
            (80000.0 * BETA_CUBIC) as usize
        );
        // Window max was less than the last max, so fast convergence applies
        assert_delta!(cubic.w_last_max, 80000.0 / max_datagram_size, 0.001);
        // W_max = W_max*(1.0+beta_cubic)/2.0 = W_max * .85
        assert_delta!(cubic.w_max, 80000.0 * 0.85 / max_datagram_size, 0.001);
    }

    #[test]
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.8")]
    fn is_under_utilized() {
        let mut cc = CubicCongestionController::new(1000);
        cc.congestion_window = 1000;
        cc.bytes_in_flight = BytesInFlight(100);

        assert!(cc.is_under_utilized());

        cc.bytes_in_flight = BytesInFlight(1000);

        assert!(!cc.is_under_utilized());
    }

    #[test]
    fn on_packet_sent() {
        let mut cc = CubicCongestionController::new(1000);
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(0));
        let now = s2n_quic_platform::time::now();

        cc.congestion_window = 100_000;

        // Last sent packet time updated to t10
        cc.on_packet_sent(now + Duration::from_secs(10), 1);

        assert_eq!(cc.bytes_in_flight.0, 1);

        // Latest RTT is 100ms
        rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(100),
            now,
            PacketNumberSpace::ApplicationData,
        );

        // Round one of Hystart
        cc.on_rtt_update(now, &rtt_estimator);

        // Latest RTT is 200ms
        rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(200),
            now,
            PacketNumberSpace::ApplicationData,
        );

        // Last sent packet time updated to t20
        cc.on_packet_sent(now + Duration::from_secs(20), 1);

        assert_eq!(cc.bytes_in_flight.0, 2);

        // Round two of Hystart
        for _i in 1..=8 {
            cc.on_rtt_update(now + Duration::from_secs(10), &rtt_estimator);
        }

        assert_eq!(cc.slow_start.threshold, 100_000);
    }

    #[test]
    fn on_packet_sent_application_limited() {
        let mut cc = CubicCongestionController::new(1000);
        let now = s2n_quic_platform::time::now();

        cc.congestion_window = 100_000;
        cc.bytes_in_flight = BytesInFlight(99900);
        cc.state = CongestionAvoidance(now);

        cc.on_packet_sent(now + Duration::from_secs(10), 100);

        assert_eq!(cc.bytes_in_flight.0, 100_000);
        assert_eq!(
            cc.time_of_last_sent_packet,
            Some(now + Duration::from_secs(10))
        );
        // Not application limited so the CongestionAvoidance start stays the same
        assert_eq!(cc.state, CongestionAvoidance(now));

        cc.bytes_in_flight = BytesInFlight(99800);

        cc.on_packet_sent(now + Duration::from_secs(25), 100);

        // Application limited so the CongestionAvoidance start moves up by 15 seconds
        // (time_of_last_sent_packet - time_sent)
        assert_eq!(cc.state, CongestionAvoidance(now + Duration::from_secs(15)));
    }

    #[test]
    fn on_packet_lost() {
        let mut cc = CubicCongestionController::new(1000);
        let now = s2n_quic_platform::time::now();
        cc.congestion_window = 100_000;
        cc.bytes_in_flight = BytesInFlight(100_000);
        cc.state = CongestionAvoidance(now);

        let mut loss_info = LossInfo::default();
        loss_info.bytes_in_flight = 100;

        cc.on_packets_lost(
            loss_info,
            Duration::from_secs(5),
            now + Duration::from_secs(10),
        );

        assert_eq!(cc.bytes_in_flight.0, 100_000 - 100);
        assert_eq!(cc.state, Recovery(now + Duration::from_secs(10)));
        assert_eq!(cc.congestion_window(), (100_000.0 * BETA_CUBIC) as usize);
        assert_eq!(cc.slow_start.threshold, (100_000.0 * BETA_CUBIC) as usize);
    }

    #[test]
    fn on_packet_lost_already_in_recovery() {
        let mut cc = CubicCongestionController::new(1000);
        let now = s2n_quic_platform::time::now();
        cc.congestion_window = 10000;
        cc.state = Recovery(now);

        cc.on_packets_lost(LossInfo::default(), Duration::from_secs(5), now);

        // No change to the congestion window
        assert_eq!(cc.congestion_window(), 10000);
    }

    #[test]
    fn on_packet_lost_persistent_congestion() {
        let mut cc = CubicCongestionController::new(1000);
        let now = s2n_quic_platform::time::now();
        cc.congestion_window = 10000;
        cc.state = Recovery(now);

        let mut loss_info = LossInfo::default();
        loss_info.persistent_congestion_period = Duration::from_secs(10);

        cc.on_packets_lost(loss_info, Duration::from_secs(5), now);

        assert_eq!(cc.state, SlowStart);
        assert_eq!(cc.congestion_window(), cc.minimum_window());
    }

    #[test]
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.2")]
    fn on_mtu_update_decrease() {
        let mut cc = CubicCongestionController::new(10000);

        cc.on_mtu_update(5000);
        assert_eq!(cc.max_datagram_size, 5000);
        assert_eq!(cc.cubic.max_datagram_size, 5000);

        assert_eq!(
            cc.congestion_window(),
            CubicCongestionController::initial_window(5000)
        );
    }

    #[test]
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.2")]
    fn on_mtu_update_increase() {
        let mut cc = CubicCongestionController::new(5000);
        cc.congestion_window = 100_000;

        cc.on_mtu_update(10000);
        assert_eq!(cc.max_datagram_size, 10000);
        assert_eq!(cc.cubic.max_datagram_size, 10000);

        assert_eq!(cc.congestion_window(), 200_000);
    }

    #[test]
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#6.4")]
    fn on_packet_discarded() {
        let mut cc = CubicCongestionController::new(5000);
        cc.bytes_in_flight = BytesInFlight(10000);

        cc.on_packet_discarded(1000);

        assert_eq!(cc.bytes_in_flight.0, 10000 - 1000);
    }

    #[test]
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.8")]
    fn on_packet_ack_limited() {
        let mut cc = CubicCongestionController::new(5000);
        let now = s2n_quic_platform::time::now();
        cc.congestion_window = 100_000;
        cc.bytes_in_flight = BytesInFlight(10000);

        cc.on_packet_ack(now, 1, &RTTEstimator::new(Duration::from_secs(0)), now);

        assert_eq!(cc.congestion_window(), 100_000);
    }

    #[test]
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.3.2")]
    fn on_packet_ack_recovery_to_congestion_avoidance() {
        let mut cc = CubicCongestionController::new(5000);
        let now = s2n_quic_platform::time::now();

        cc.cubic.update_w_max(25000);
        cc.state = Recovery(now);
        cc.bytes_in_flight = BytesInFlight(25000);

        cc.on_packet_ack(
            now + Duration::from_millis(1),
            1,
            &RTTEstimator::new(Duration::from_secs(0)),
            now + Duration::from_millis(2),
        );

        assert_eq!(
            cc.state,
            CongestionAvoidance(now + Duration::from_millis(2))
        );
    }

    #[test]
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.3.2")]
    fn on_packet_ack_slow_start_to_congestion_avoidance() {
        let mut cc = CubicCongestionController::new(5000);
        let now = s2n_quic_platform::time::now();

        cc.state = SlowStart;
        cc.congestion_window = 10000;
        cc.bytes_in_flight = BytesInFlight(10000);
        cc.slow_start.threshold = 10050;

        cc.on_packet_ack(
            now,
            100,
            &RTTEstimator::new(Duration::from_secs(0)),
            now + Duration::from_millis(2),
        );

        assert_eq!(cc.congestion_window(), 10100);
        assert_eq!(cc.packets_to_bytes(cc.cubic.w_max), cc.congestion_window);
        assert_eq!(
            cc.state,
            CongestionAvoidance(now + Duration::from_millis(2))
        );
    }

    #[test]
    fn on_packet_ack_recovery() {
        let mut cc = CubicCongestionController::new(5000);
        let now = s2n_quic_platform::time::now();

        cc.state = Recovery(now);
        cc.congestion_window = 10000;
        cc.bytes_in_flight = BytesInFlight(10000);

        cc.on_packet_ack(
            now,
            100,
            &RTTEstimator::new(Duration::from_secs(0)),
            now + Duration::from_millis(2),
        );

        // Congestion window stays the same in recovery
        assert_eq!(cc.congestion_window(), 10000);
        assert_eq!(cc.state, Recovery(now));
    }

    #[test]
    fn on_packet_ack_congestion_avoidance() {
        let mut cc = CubicCongestionController::new(5000);
        let mut cc2 = CubicCongestionController::new(5000);
        let now = s2n_quic_platform::time::now();

        cc.state = CongestionAvoidance(now + Duration::from_millis(3300));
        cc.congestion_window = 10000;
        cc.bytes_in_flight = BytesInFlight(10000);
        cc.cubic.update_w_max(10000);

        cc2.congestion_window = 10000;
        cc2.bytes_in_flight = BytesInFlight(10000);
        cc2.cubic.update_w_max(10000);

        let mut rtt_estimator = RTTEstimator::new(Duration::from_secs(0));
        rtt_estimator.update_rtt(
            Duration::from_secs(0),
            Duration::from_millis(275),
            now,
            PacketNumberSpace::ApplicationData,
        );

        cc.on_packet_ack(now, 1000, &rtt_estimator, now + Duration::from_millis(4750));

        let t = Duration::from_millis(4750) - Duration::from_millis(3300);
        let rtt = rtt_estimator.min_rtt();

        cc2.congestion_avoidance(t, rtt, 1000);

        assert_eq!(cc.congestion_window(), cc2.congestion_window());
    }

    #[test]
    #[compliance::tests("https://tools.ietf.org/rfc/rfc8312.txt#4.2")]
    fn on_packet_ack_congestion_avoidance_tcp_friendly_region() {
        let mut cc = CubicCongestionController::new(5000);

        cc.congestion_window = 10000;
        cc.cubic.update_w_max(30 * 5000);

        let t = Duration::from_millis(4400);
        let rtt = Duration::from_millis(200);

        cc.congestion_avoidance(t, rtt, 1000);

        assert!(cc.cubic.w_cubic(t) < cc.cubic.w_est(t, rtt));
        assert_eq!(
            cc.congestion_window(),
            (cc.cubic.w_est(t, rtt) * 5000.0) as usize
        );
    }

    #[test]
    #[compliance::tests("https://tools.ietf.org/rfc/rfc8312.txt#4.3")]
    fn on_packet_ack_congestion_avoidance_concave_region() {
        let max_datagram_size = 1200.0;
        let mut cc = CubicCongestionController::new(max_datagram_size as usize);

        cc.congestion_window = 2_400_000;
        cc.cubic.update_w_max(2_764_800);

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

        assert_eq!(cc.congestion_window(), 2_400_180);
    }

    #[test]
    #[compliance::tests("https://tools.ietf.org/rfc/rfc8312.txt#4.4")]
    fn on_packet_ack_congestion_avoidance_convex_region() {
        let max_datagram_size = 1200.0;
        let mut cc = CubicCongestionController::new(max_datagram_size as usize);

        cc.congestion_window = 3_600_000;
        cc.cubic.update_w_max(2_764_800);

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
        let max_datagram_size = 1200.0;
        let mut cc = CubicCongestionController::new(max_datagram_size as usize);

        cc.congestion_window = 3_600_000;
        cc.cubic.update_w_max(2_764_800);

        let t = Duration::from_millis(125_800);
        let rtt = Duration::from_millis(200);

        cc.congestion_avoidance(t, rtt, 1000);

        assert!(cc.cubic.w_cubic(t) > cc.cubic.w_est(t, rtt));
        assert_eq!(cc.congestion_window(), 3_600_000 + 1000 / 2);
    }
}
