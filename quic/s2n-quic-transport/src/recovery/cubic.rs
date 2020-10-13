use crate::recovery::{
    congestion_controller::CongestionController, hybrid_slow_start::HybridSlowStart,
    loss_info::LossInfo,
};
use core::{
    cmp::{max, min},
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
#[derive(Clone, PartialEq, Eq)]
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
}

impl CongestionController for CubicCongestionController {
    fn congestion_window(&self) -> usize {
        self.congestion_window
    }

    fn on_packet_sent(&mut self, time_sent: Timestamp, sent_bytes: usize) {
        self.slow_start.on_packet_sent(time_sent, sent_bytes);
    }

    fn on_rtt_update(&mut self, rtt_estimator: &RTTEstimator) {
        // Update the Slow Start algorithm each time the RTT estimate is updated to find
        // the slow start threshold. If the threshold has already been found, this is a no-op.
        self.slow_start
            .on_rtt_update(self.congestion_window, rtt_estimator.latest_rtt());
    }

    fn on_packet_ack(
        &mut self,
        time_sent: Timestamp,
        sent_bytes: usize,
        is_limited: bool,
        rtt_estimator: &RTTEstimator,
        ack_receive_time: Timestamp,
    ) {
        if is_limited {
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
            if time_sent > recovery_start_time {
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.3.2
                //# A recovery period ends and the sender enters congestion avoidance
                //# when a packet sent during the recovery period is acknowledged.
                self.state = State::CongestionAvoidance(ack_receive_time)
            }
        };

        match self.state {
            State::SlowStart => {
                // Slow start
                //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.3.1
                //# While a sender is in slow start, the congestion window increases by
                //# the number of bytes acknowledged when each acknowledgment is
                //# processed.  This results in exponential growth of the congestion
                //# window.
                self.congestion_window += sent_bytes;

                if self.congestion_window >= self.slow_start.threshold() {
                    //= https://tools.ietf.org/rfc/rfc8312.txt#4.8
                    //# In the case when CUBIC runs the hybrid slow start [HR08], it may exit
                    //# the first slow start without incurring any packet loss and thus W_max
                    //# is undefined.  In this special case, CUBIC switches to congestion
                    //# avoidance and increases its congestion window size using Eq. 1, where
                    //# t is the elapsed time since the beginning of the current congestion
                    //# avoidance, K is set to 0, and W_max is set to the congestion window
                    //# size at the beginning of the current congestion avoidance.
                    self.state = State::CongestionAvoidance(ack_receive_time);
                    self.cubic.w_max = self.congestion_window;
                }
            }
            State::Recovery(_) => {
                // Don't increase the congestion window while in recovery
            }
            State::CongestionAvoidance(avoidance_start_time) => {
                //= https://tools.ietf.org/rfc/rfc8312.txt#4.1
                //# t is the elapsed time from the beginning of the current congestion avoidance
                let t = ack_receive_time - avoidance_start_time;

                //= https://tools.ietf.org/rfc/rfc8312.txt#4.1
                //# RTT is the weighted average RTT
                let rtt = rtt_estimator.smoothed_rtt();

                self.congestion_avoidance(t, rtt);
            }
        };
    }

    fn on_packets_lost(
        &mut self,
        loss_info: LossInfo,
        persistent_congestion_duration: Duration,
        timestamp: Timestamp,
    ) {
        self.on_congestion_event(timestamp);

        // Reset the congestion window if the loss of these
        // packets indicates persistent congestion.
        if loss_info.persistent_congestion_period > persistent_congestion_duration {
            self.congestion_window = self.minimum_window();
            self.state = State::SlowStart;
        }
    }

    fn on_congestion_event(&mut self, event_time: Timestamp) {
        // No reaction if already in a recovery period.
        if let State::Recovery(_) = self.state {
            return;
        }

        // Enter recovery period.
        self.state = State::Recovery(event_time);

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

        if max_datagram_size < old_max_datagram_size {
            self.congestion_window = self.initial_window();
        }
    }
}

impl CubicCongestionController {
    // TODO: Remove when used
    #[allow(dead_code)]
    pub fn new(max_datagram_size: usize) -> Self {
        Self {
            cubic: Cubic::default(),
            slow_start: HybridSlowStart::new(max_datagram_size),
            max_datagram_size,
            congestion_window: 0,
            state: State::SlowStart,
        }
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.2
    //# The minimum congestion window is the smallest value the congestion
    //# window can decrease to as a response to loss, ECN-CE, or persistent
    //# congestion.  The RECOMMENDED value is 2 * max_datagram_size.
    fn minimum_window(&self) -> usize {
        2 * self.max_datagram_size
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-31.txt#7.2
    //# Endpoints SHOULD use an initial congestion window of 10 times the
    //# maximum datagram size (max_datagram_size), limited to the larger
    //# of 14720 bytes or twice the maximum datagram size.
    fn initial_window(&self) -> usize {
        const INITIAL_WINDOW_LIMIT: usize = 14720;
        min(
            10 * self.max_datagram_size,
            max(INITIAL_WINDOW_LIMIT, self.minimum_window()),
        )
    }

    fn congestion_avoidance(&mut self, t: Duration, rtt: Duration) {
        let w_cubic = self.cubic.w_cubic(t);
        let w_est = self.cubic.w_est(t, rtt);

        if w_cubic < w_est {
            // TCP-Friendly Region
            //= https://tools.ietf.org/rfc/rfc8312.txt#4.2
            //# When receiving an ACK in congestion avoidance (cwnd could be greater than
            //# or less than W_max), CUBIC checks whether W_cubic(t) is less than
            //# W_est(t).  If so, CUBIC is in the TCP-friendly region and cwnd SHOULD
            //# be set to W_est(t) at each reception of an ACK.
            self.congestion_window = self.cubic.w_est(t, rtt);
        } else {
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

            // The congestion window is adjusted in the same way in
            // the convex and concave regions
            self.congestion_window +=
                (self.cubic.w_cubic(t + rtt) - self.congestion_window) / self.congestion_window;
        }
    }
}

/// Core functions of "CUBIC for Fast Long-Distance Networks" as specified in
/// https://tools.ietf.org/html/rfc8312
#[derive(Clone, Debug, Default)]
struct Cubic {
    //= https://tools.ietf.org/rfc/rfc8312.txt#4.1
    //# W_max is the window size just before the window is
    //# reduced in the last congestion event.
    w_max: usize,
    //= https://tools.ietf.org/rfc/rfc8312.txt#4.6
    //# a flow remembers the last value of W_max before it
    //# updates W_max for the current congestion event.
    //# Let us call the last value of W_max to be W_last_max.
    w_last_max: usize,
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
    fn w_cubic(&self, t: Duration) -> usize {
        let k = Duration::from_secs_f32((self.w_max as f32 * (1.0 - BETA_CUBIC) / C).cbrt());
        (t - k).as_secs().pow(3) as usize + self.w_max
    }

    //= https://tools.ietf.org/rfc/rfc8312.txt#4.2
    //# W_est(t) = W_max*beta_cubic +
    //               [3*(1-beta_cubic)/(1+beta_cubic)] * (t/RTT) (Eq. 4)
    fn w_est(&self, t: Duration, rtt: Duration) -> usize {
        (self.w_max as f32 * BETA_CUBIC
            + (3.0 * (1.0 - BETA_CUBIC) / (1.0 + BETA_CUBIC))
                * (t.as_secs_f32() / rtt.as_secs_f32())) as usize
    }

    //# https://tools.ietf.org/rfc/rfc8312.txt#4.5
    //# When a packet loss is detected by duplicate ACKs or a network
    //# congestion is detected by ECN-Echo ACKs, CUBIC updates its W_max,
    //# cwnd, and ssthresh as follows.  Parameter beta_cubic SHOULD be set to
    //# 0.7.
    //#
    //#    W_max = cwnd;                 // save window size before reduction
    //#    ssthresh = cwnd * beta_cubic; // new slow-start threshold
    //#    ssthresh = max(ssthresh, 2);  // threshold is at least 2 MSS
    //#    cwnd = cwnd * beta_cubic;     // window reduction

    //# https://tools.ietf.org/rfc/rfc8312.txt#4.6
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
    fn multiplicative_decrease(&mut self, cwnd: usize) -> usize {
        self.w_max = cwnd;

        if self.w_max < self.w_last_max {
            self.w_last_max = self.w_max;
            self.w_max = (self.w_max as f32 * (1.0 + BETA_CUBIC) / 2.0) as usize;
        } else {
            self.w_last_max = self.w_max;
        }

        (cwnd as f32 * BETA_CUBIC) as usize
    }
}

#[cfg(test)]
mod test {
    //TODO
}
