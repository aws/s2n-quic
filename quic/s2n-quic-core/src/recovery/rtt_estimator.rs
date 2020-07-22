use crate::packet::number::PacketNumberSpace;
use core::{cmp::min, time::Duration};

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.2
//# When no previous RTT is available, the initial RTT SHOULD be set to 333ms,
//# resulting in a 1 second initial timeout, as recommended in [RFC6298].
pub const DEFAULT_INITIAL_RTT: Duration = Duration::from_millis(333);
const ZERO_DURATION: Duration = Duration::from_millis(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RTTEstimator {
    /// Latest RTT sample
    latest_rtt: Duration,
    /// The minimum value observed over the lifetime of the connection
    min_rtt: Duration,
    /// An exponentially-weighted moving average
    smoothed_rtt: Duration,
    /// The variance in the observed RTT samples
    rttvar: Duration,
    /// The maximum amount of time by which the receiver intends to delay acknowledgments for
    /// packets in the ApplicationData packet number space. The actual ack_delay in a received
    /// ACK frame may be larger due to late timers, reordering, or lost ACK frames.
    max_ack_delay: Duration,
}

impl RTTEstimator {
    /// Creates a new RTT Estimator with default initial values using the given `max_ack_delay`.
    pub fn new(max_ack_delay: Duration) -> Self {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3
        //# Before any RTT samples are available, the initial RTT is used as rtt_sample.
        let rtt_sample = DEFAULT_INITIAL_RTT;

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3
        //# When there are no samples for a network path, and on the first RTT
        //# sample for the network path:
        //#
        //# smoothed_rtt = rtt_sample
        //# rttvar = rtt_sample / 2
        let smoothed_rtt = rtt_sample;
        let rttvar = rtt_sample / 2;

        Self {
            latest_rtt: ZERO_DURATION,
            min_rtt: ZERO_DURATION,
            smoothed_rtt,
            rttvar,
            max_ack_delay,
        }
    }

    /// Gets the latest round trip time sample
    pub fn latest_rtt(&self) -> Duration {
        self.latest_rtt
    }

    /// Gets the weighted average round trip time
    pub fn smoothed_rtt(&self) -> Duration {
        self.smoothed_rtt
    }

    /// Gets the variance in observed round trip time samples
    pub fn rttvar(&self) -> Duration {
        self.rttvar
    }
}

impl RTTEstimator {
    pub fn update_rtt(
        &mut self,
        mut ack_delay: Duration,
        rtt_sample: Duration,
        space: PacketNumberSpace,
    ) {
        self.latest_rtt = rtt_sample.max(Duration::from_millis(1));

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.2
        //# min_rtt is set to the latest_rtt on the first RTT sample,
        if self.min_rtt == ZERO_DURATION {
            self.min_rtt = self.latest_rtt;
            self.smoothed_rtt = self.latest_rtt;
            self.rttvar = self.latest_rtt / 2;
            return;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.2
        //# and to the lesser of min_rtt and latest_rtt on
        //# subsequent samples.
        self.min_rtt = min(self.min_rtt, self.latest_rtt);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3
        //# When adjusting an RTT sample using peer-reported acknowledgement
        //# delays, an endpoint:
        //#
        //# *  MUST ignore the Ack Delay field of the ACK frame for packets sent in the Initial
        //#    and Handshake packet number space.
        if space.is_initial() || space.is_handshake() {
            ack_delay = ZERO_DURATION;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3
        //# *  MUST use the lesser of the value reported in Ack Delay field of
        //#    the ACK frame and the peer's max_ack_delay transport parameter.
        let ack_delay = min(ack_delay, self.max_ack_delay);

        let mut adjusted_rtt = self.latest_rtt;

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3
        //# *  MUST NOT apply the adjustment if the resulting RTT sample
        //#    is smaller than the min_rtt.
        if self.min_rtt + ack_delay < self.latest_rtt {
            adjusted_rtt -= ack_delay;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3
        //# On subsequent RTT samples, smoothed_rtt and rttvar evolve as follows:
        //#
        //# ack_delay = min(Ack Delay in ACK Frame, max_ack_delay)
        //# adjusted_rtt = latest_rtt
        //# if (min_rtt + ack_delay < latest_rtt):
        //# adjusted_rtt = latest_rtt - ack_delay
        //# smoothed_rtt = 7/8 * smoothed_rtt + 1/8 * adjusted_rtt
        //# rttvar_sample = abs(smoothed_rtt - adjusted_rtt)
        //# rttvar = 3/4 * rttvar + 1/4 * rttvar_sample
        self.smoothed_rtt = 7 * self.smoothed_rtt / 8 + adjusted_rtt / 8;
        let rttvar_sample = abs_difference(self.smoothed_rtt, adjusted_rtt);
        self.rttvar = 3 * self.rttvar / 4 + rttvar_sample / 4;
    }
}

fn abs_difference<T: core::ops::Sub + PartialOrd>(a: T, b: T) -> <T as core::ops::Sub>::Output {
    if a > b {
        a - b
    } else {
        b - a
    }
}

#[cfg(test)]
mod test {
    use crate::{
        packet::number::PacketNumberSpace,
        recovery::{RTTEstimator, DEFAULT_INITIAL_RTT},
        time::Duration,
    };

    /// Test the initial values before any RTT samples
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3")]
    #[test]
    fn initial_rtt() {
        let rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        assert_eq!(rtt_estimator.min_rtt, Duration::from_millis(0));
        assert_eq!(rtt_estimator.latest_rtt(), Duration::from_millis(0));
        assert_eq!(rtt_estimator.smoothed_rtt(), DEFAULT_INITIAL_RTT);
        assert_eq!(rtt_estimator.rttvar(), DEFAULT_INITIAL_RTT / 2);
    }

    /// Test a zero RTT value is treated as 1 ms
    #[test]
    fn zero_rtt_sample() {
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            Duration::from_millis(0),
            PacketNumberSpace::ApplicationData,
        );
        assert_eq!(rtt_estimator.min_rtt, Duration::from_millis(1));
        assert_eq!(rtt_estimator.latest_rtt(), Duration::from_millis(1));
    }

    #[compliance::tests(
    /// MUST use the lesser of the value reported in Ack Delay field of the ACK frame and the peer's
    /// max_ack_delay transport parameter.
    "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3")]
    #[test]
    fn max_ack_delay() {
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(100),
            PacketNumberSpace::ApplicationData,
        );
        rtt_estimator.update_rtt(
            Duration::from_millis(1000),
            Duration::from_millis(200),
            PacketNumberSpace::ApplicationData,
        );
        assert_eq!(
            rtt_estimator.smoothed_rtt,
            7 * Duration::from_millis(100) / 8 + Duration::from_millis(200 - 10) / 8
        );
    }

    /// Test several rounds of RTT updates
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.2")]
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3")]
    #[test]
    fn update_rtt() {
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let rtt_sample = Duration::from_millis(500);
        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            rtt_sample,
            PacketNumberSpace::ApplicationData,
        );
        assert_eq!(rtt_estimator.min_rtt, rtt_sample);
        assert_eq!(rtt_estimator.latest_rtt, rtt_sample);
        assert_eq!(rtt_estimator.smoothed_rtt, rtt_sample);
        assert_eq!(rtt_estimator.rttvar, rtt_sample / 2);

        let prev_smoothed_rtt = rtt_estimator.smoothed_rtt;
        let rtt_sample = Duration::from_millis(800);
        let ack_delay = Duration::from_millis(10);

        rtt_estimator.update_rtt(ack_delay, rtt_sample, PacketNumberSpace::ApplicationData);

        let adjusted_rtt = rtt_sample - ack_delay;

        assert_eq!(rtt_estimator.min_rtt, prev_smoothed_rtt);
        assert_eq!(rtt_estimator.latest_rtt, rtt_sample);
        assert_eq!(
            rtt_estimator.smoothed_rtt,
            7 * prev_smoothed_rtt / 8 + adjusted_rtt / 8
        );

        // This rtt_sample is a new minimum, so the ack_delay is not used for adjustment
        let prev_smoothed_rtt = rtt_estimator.smoothed_rtt;
        let rtt_sample = Duration::from_millis(200);
        let ack_delay = Duration::from_millis(10);

        rtt_estimator.update_rtt(ack_delay, rtt_sample, PacketNumberSpace::ApplicationData);

        assert_eq!(rtt_estimator.min_rtt, rtt_sample);
        assert_eq!(rtt_estimator.latest_rtt, rtt_sample);
        assert_eq!(
            rtt_estimator.smoothed_rtt,
            7 * prev_smoothed_rtt / 8 + rtt_sample / 8
        );
    }

    #[compliance::tests(
    /// MUST ignore the Ack Delay field of the ACK frame for packets 
    /// sent in the Initial and Handshake packet number space.
    "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3")]
    #[test]
    fn initial_and_handshake_space() {
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let rtt_sample = Duration::from_millis(500);
        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            rtt_sample,
            PacketNumberSpace::Initial,
        );

        let prev_smoothed_rtt = rtt_estimator.smoothed_rtt;
        let rtt_sample = Duration::from_millis(1000);

        rtt_estimator.update_rtt(
            Duration::from_millis(100),
            rtt_sample,
            PacketNumberSpace::Initial,
        );

        assert_eq!(
            rtt_estimator.smoothed_rtt,
            7 * prev_smoothed_rtt / 8 + rtt_sample / 8
        );

        let prev_smoothed_rtt = rtt_estimator.smoothed_rtt;
        let rtt_sample = Duration::from_millis(2000);

        rtt_estimator.update_rtt(
            Duration::from_millis(100),
            rtt_sample,
            PacketNumberSpace::Handshake,
        );

        assert_eq!(
            rtt_estimator.smoothed_rtt,
            7 * prev_smoothed_rtt / 8 + rtt_sample / 8
        );
    }
}
