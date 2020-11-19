use crate::{packet::number::PacketNumberSpace, time::Timestamp};
use core::{
    cmp::{max, min},
    time::Duration,
};

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.2.2
//# When no previous RTT is available, the initial RTT SHOULD be set to 333ms,
//# resulting in a 1 second initial timeout, as recommended in [RFC6298].
pub const DEFAULT_INITIAL_RTT: Duration = Duration::from_millis(333);
const ZERO_DURATION: Duration = Duration::from_millis(0);

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#6.1.2
//# The RECOMMENDED value of the timer granularity (kGranularity) is 1ms.
pub const K_GRANULARITY: Duration = Duration::from_millis(1);

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.1
//# The RECOMMENDED value for kPersistentCongestionThreshold is 3, which
//# results in behavior that is approximately equivalent to a TCP sender
//# declaring an RTO after two TLPs.
const K_PERSISTENT_CONGESTION_THRESHOLD: u32 = 3;

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
    /// The time that the first RTT sample was obtained
    first_rtt_sample: Option<Timestamp>,
}

impl RTTEstimator {
    /// Creates a new RTT Estimator with default initial values using the given `max_ack_delay`.
    pub fn new(max_ack_delay: Duration) -> Self {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
        //# Before any RTT samples are available for a new path or when the
        //# estimator is reset, the estimator is initialized using the initial RTT;
        //# see Section 6.2.2.
        //#
        //# smoothed_rtt and rttvar are initialized as follows, where kInitialRtt
        //# contains the initial RTT value:
        //
        //# smoothed_rtt = kInitialRtt
        //# rttvar = kInitialRtt / 2
        let smoothed_rtt = DEFAULT_INITIAL_RTT;
        let rttvar = DEFAULT_INITIAL_RTT / 2;

        Self {
            latest_rtt: ZERO_DURATION,
            min_rtt: ZERO_DURATION,
            smoothed_rtt,
            rttvar,
            max_ack_delay,
            first_rtt_sample: None,
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

    /// Gets the minimum round trip time
    pub fn min_rtt(&self) -> Duration {
        self.min_rtt
    }

    /// Gets the variance in observed round trip time samples
    pub fn rttvar(&self) -> Duration {
        self.rttvar
    }

    /// Gets the timestamp of the first RTT sample
    pub fn first_rtt_sample(&self) -> Option<Timestamp> {
        self.first_rtt_sample
    }
}

impl RTTEstimator {
    pub fn update_rtt(
        &mut self,
        mut ack_delay: Duration,
        rtt_sample: Duration,
        timestamp: Timestamp,
        is_handshake_confirmed: bool,
        space: PacketNumberSpace,
    ) {
        self.latest_rtt = rtt_sample.max(Duration::from_millis(1));

        if self.first_rtt_sample.is_none() {
            self.first_rtt_sample = Some(timestamp);
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.2
            //# min_rtt MUST be set to the latest_rtt on the first RTT sample.
            self.min_rtt = self.latest_rtt;
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
            //# On the first RTT sample after initialization, smoothed_rtt and rttvar
            //# are set as follows:
            //#
            //# smoothed_rtt = latest_rtt
            //# rttvar = latest_rtt / 2
            self.smoothed_rtt = self.latest_rtt;
            self.rttvar = self.latest_rtt / 2;
            return;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.2
        //# min_rtt MUST be set to the lesser of min_rtt and latest_rtt
        //# (Section 5.1) on all other samples.
        self.min_rtt = min(self.min_rtt, self.latest_rtt);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
        //# when adjusting an RTT sample using peer-reported acknowledgement
        //# delays, an endpoint:
        //#
        //# *  MAY ignore the acknowledgement delay for Initial packets, since
        //#    these acknowledgements are not delayed by the peer (Section 13.2.1
        //#    of [QUIC-TRANSPORT]);
        if space.is_initial() {
            ack_delay = ZERO_DURATION;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
        //# To account for this, the endpoint SHOULD ignore
        //# max_ack_delay until the handshake is confirmed (Section 4.1.2 of
        //# [QUIC-TLS]).

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
        //# *  SHOULD ignore the peer's max_ack_delay until the handshake is
        //#    confirmed;
        if is_handshake_confirmed {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
            //# *  MUST use the lesser of the acknowledgement delay and the peer's
            //#    max_ack_delay after the handshake is confirmed; and
            ack_delay = min(ack_delay, self.max_ack_delay);
        }

        let mut adjusted_rtt = self.latest_rtt;

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
        //# *  MUST NOT subtract the acknowledgement delay from the RTT sample if
        //#    the resulting value is smaller than the min_rtt.
        if self.min_rtt + ack_delay < self.latest_rtt {
            adjusted_rtt -= ack_delay;
        } else if !is_handshake_confirmed {
            //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
            //# Therefore, prior to handshake
            //# confirmation, an endpoint MAY ignore RTT samples if adjusting the RTT
            //# sample for acknowledgement delay causes the sample to be less than
            //# the min_rtt.
            return;
        }

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
        //# On subsequent RTT samples, smoothed_rtt and rttvar evolve as follows:
        //#
        //# ack_delay = decoded acknowledgement delay from ACK frame
        //# if (handshake confirmed):
        //#   ack_delay = min(ack_delay, max_ack_delay)
        //# adjusted_rtt = latest_rtt
        //# if (min_rtt + ack_delay < latest_rtt):
        //#   adjusted_rtt = latest_rtt - ack_delay
        //# smoothed_rtt = 7/8 * smoothed_rtt + 1/8 * adjusted_rtt
        //# rttvar_sample = abs(smoothed_rtt - adjusted_rtt)
        //# rttvar = 3/4 * rttvar + 1/4 * rttvar_sample
        self.smoothed_rtt = 7 * self.smoothed_rtt / 8 + adjusted_rtt / 8;
        let rttvar_sample = abs_difference(self.smoothed_rtt, adjusted_rtt);
        self.rttvar = 3 * self.rttvar / 4 + rttvar_sample / 4;
    }

    /// Calculates the persistent congestion threshold used for determining
    /// if persistent congestion is being encountered.
    pub fn persistent_congestion_threshold(&self) -> Duration {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.1
        //# The persistent congestion duration is computed as follows:
        //#
        //# (smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay) *
        //#    kPersistentCongestionThreshold
        //#
        //# Unlike the PTO computation in Section 6.2, this duration includes the
        //# max_ack_delay irrespective of the packet number spaces in which
        //# losses are established.
        //#
        //# This duration allows a sender to send as many packets before
        //# establishing persistent congestion, including some in response to PTO
        //# expiration, as TCP does with Tail Loss Probes ([RACK]) and a
        //# Retransmission Timeout ([RFC5681]).
        (self.smoothed_rtt + max(4 * self.rttvar, K_GRANULARITY) + self.max_ack_delay)
            * K_PERSISTENT_CONGESTION_THRESHOLD
    }

    /// Allows min_rtt and smoothed_rtt to be overwritten on the next RTT sample
    /// after persistent congestion is established.
    pub fn on_persistent_congestion(&mut self) {
        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.2
        //# Endpoints SHOULD set the min_rtt to the newest RTT sample after
        //# persistent congestion is established.
        self.first_rtt_sample = None;
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
        time::{Clock, Duration, NoopClock},
    };

    /// Test the initial values before any RTT samples
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3")]
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
        let now = NoopClock.get_time();
        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            Duration::from_millis(0),
            now,
            false,
            PacketNumberSpace::ApplicationData,
        );
        assert_eq!(rtt_estimator.min_rtt, Duration::from_millis(1));
        assert_eq!(rtt_estimator.latest_rtt(), Duration::from_millis(1));
        assert_eq!(rtt_estimator.first_rtt_sample(), Some(now));
    }

    #[compliance::tests(
    /// *  MUST use the lesser of the acknowledgement delay and the peer's
    //     max_ack_delay after the handshake is confirmed;.
    "https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3")]
    #[test]
    fn max_ack_delay() {
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let now = NoopClock.get_time();
        rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_millis(100),
            now,
            true,
            PacketNumberSpace::ApplicationData,
        );

        // Update when the handshake is confirmed
        rtt_estimator.update_rtt(
            Duration::from_millis(1000),
            Duration::from_millis(200),
            now,
            true,
            PacketNumberSpace::ApplicationData,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
        //= type=test
        //# *  MUST use the lesser of the acknowledgement delay and the peer's
        //# max_ack_delay after the handshake is confirmed; and
        assert_eq!(
            rtt_estimator.smoothed_rtt,
            7 * Duration::from_millis(100) / 8 + Duration::from_millis(200 - 10) / 8
        );
        assert_eq!(rtt_estimator.first_rtt_sample(), Some(now));

        let prev_smoothed_rtt = rtt_estimator.smoothed_rtt;

        // Update when the handshake is not confirmed
        rtt_estimator.update_rtt(
            Duration::from_millis(50),
            Duration::from_millis(200),
            now,
            false,
            PacketNumberSpace::ApplicationData,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
        //= type=test
        //# To account for this, the endpoint SHOULD ignore
        //# max_ack_delay until the handshake is confirmed (Section 4.1.2 of
        //# [QUIC-TLS]).

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
        //= type=test
        //# *  SHOULD ignore the peer's max_ack_delay until the handshake is
        //# confirmed;
        assert_eq!(
            rtt_estimator.smoothed_rtt,
            7 * prev_smoothed_rtt / 8 + Duration::from_millis(200 - 50) / 8
        );
    }

    /// Test several rounds of RTT updates
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.2")]
    #[compliance::tests("https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3")]
    #[test]
    fn update_rtt() {
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let now = NoopClock.get_time();
        let rtt_sample = Duration::from_millis(500);
        assert!(rtt_estimator.first_rtt_sample.is_none());
        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            rtt_sample,
            now,
            true,
            PacketNumberSpace::ApplicationData,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.2
        //= type=test
        //# min_rtt MUST be set to the latest_rtt on the first RTT sample.
        assert_eq!(rtt_estimator.min_rtt, rtt_sample);
        assert_eq!(rtt_estimator.latest_rtt, rtt_sample);
        assert_eq!(rtt_estimator.smoothed_rtt, rtt_sample);
        assert_eq!(rtt_estimator.rttvar, rtt_sample / 2);
        assert_eq!(rtt_estimator.first_rtt_sample, Some(now));

        let prev_smoothed_rtt = rtt_estimator.smoothed_rtt;
        let rtt_sample = Duration::from_millis(800);
        let ack_delay = Duration::from_millis(10);

        rtt_estimator.update_rtt(
            ack_delay,
            rtt_sample,
            now + Duration::from_secs(1),
            true,
            PacketNumberSpace::ApplicationData,
        );

        let adjusted_rtt = rtt_sample - ack_delay;

        assert_eq!(rtt_estimator.min_rtt, prev_smoothed_rtt);
        assert_eq!(rtt_estimator.latest_rtt, rtt_sample);
        assert_eq!(
            rtt_estimator.smoothed_rtt,
            7 * prev_smoothed_rtt / 8 + adjusted_rtt / 8
        );
        assert_eq!(rtt_estimator.first_rtt_sample, Some(now));

        // This rtt_sample is a new minimum, so the ack_delay is not used for adjustment
        let prev_smoothed_rtt = rtt_estimator.smoothed_rtt;
        let rtt_sample = Duration::from_millis(200);
        let ack_delay = Duration::from_millis(10);

        rtt_estimator.update_rtt(
            ack_delay,
            rtt_sample,
            now + Duration::from_secs(2),
            true,
            PacketNumberSpace::ApplicationData,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.2
        //= type=test
        //# min_rtt MUST be set to the lesser of min_rtt and latest_rtt
        //# (Section 5.1) on all other samples.
        assert_eq!(rtt_estimator.min_rtt, rtt_sample);
        assert_eq!(rtt_estimator.latest_rtt, rtt_sample);
        assert_eq!(
            rtt_estimator.smoothed_rtt,
            7 * prev_smoothed_rtt / 8 + rtt_sample / 8
        );
        assert_eq!(rtt_estimator.first_rtt_sample, Some(now));
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
    //= type=test
    //# *  MUST NOT subtract the acknowledgement delay from the RTT sample if
    //#    the resulting value is smaller than the min_rtt.
    #[test]
    fn must_not_subtract_acknowledgement_delay_if_result_smaller_than_min_rtt() {
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(200));
        let now = NoopClock.get_time();

        rtt_estimator.min_rtt = Duration::from_millis(500);
        rtt_estimator.smoothed_rtt = Duration::from_millis(700);
        rtt_estimator.first_rtt_sample = Some(now);

        let rtt_sample = Duration::from_millis(600);
        let prev_smoothed_rtt = rtt_estimator.smoothed_rtt;

        rtt_estimator.update_rtt(
            Duration::from_millis(200),
            rtt_sample,
            now,
            true,
            PacketNumberSpace::ApplicationData,
        );

        assert_eq!(
            rtt_estimator.smoothed_rtt,
            7 * prev_smoothed_rtt / 8 + rtt_sample / 8
        );
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
    //= type=test
    //# Therefore, prior to handshake
    //# confirmation, an endpoint MAY ignore RTT samples if adjusting the RTT
    //# sample for acknowledgement delay causes the sample to be less than
    //# the min_rtt.
    #[test]
    fn prior_to_handshake_ignore_if_less_than_min_rtt() {
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(200));
        let now = NoopClock.get_time();
        let smoothed_rtt = Duration::from_millis(700);

        rtt_estimator.min_rtt = Duration::from_millis(500);
        rtt_estimator.smoothed_rtt = smoothed_rtt;
        rtt_estimator.first_rtt_sample = Some(now);

        let rtt_sample = Duration::from_millis(600);

        rtt_estimator.update_rtt(
            Duration::from_millis(200),
            rtt_sample,
            now,
            false,
            PacketNumberSpace::ApplicationData,
        );

        assert_eq!(rtt_estimator.smoothed_rtt, smoothed_rtt);
    }

    //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.3
    //= type=test
    //# *  MAY ignore the acknowledgement delay for Initial packets, since
    //#    these acknowledgements are not delayed by the peer (Section 13.2.1
    //#    of [QUIC-TRANSPORT]);
    #[test]
    fn initial_space() {
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let now = NoopClock.get_time();
        let rtt_sample = Duration::from_millis(500);
        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            rtt_sample,
            now,
            true,
            PacketNumberSpace::Initial,
        );

        let prev_smoothed_rtt = rtt_estimator.smoothed_rtt;
        let rtt_sample = Duration::from_millis(1000);

        rtt_estimator.update_rtt(
            Duration::from_millis(100),
            rtt_sample,
            now,
            true,
            PacketNumberSpace::Initial,
        );

        assert_eq!(
            rtt_estimator.smoothed_rtt,
            7 * prev_smoothed_rtt / 8 + rtt_sample / 8
        );
    }

    #[compliance::tests(
    /// The persistent congestion duration is computed as follows:
    /// 
    /// (smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay) *
    ///    kPersistentCongestionThreshold
    "https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.1")]
    #[test]
    fn persistent_congestion_duration() {
        let max_ack_delay = Duration::from_millis(10);
        let mut rtt_estimator = RTTEstimator::new(max_ack_delay);

        rtt_estimator.smoothed_rtt = Duration::from_millis(100);
        rtt_estimator.rttvar = Duration::from_millis(50);

        // persistent congestion period =
        // (smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay) * kPersistentCongestionThreshold
        // = (100 + max(4*50, 1) + 10) * 3 = 930
        assert_eq!(
            Duration::from_millis(930),
            rtt_estimator.persistent_congestion_threshold()
        );

        rtt_estimator.rttvar = Duration::from_millis(0);

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#7.6.1
        //= type=test
        //# The RECOMMENDED value for kPersistentCongestionThreshold is 3, which
        //# results in behavior that is approximately equivalent to a TCP sender
        //# declaring an RTO after two TLPs.

        // persistent congestion period =
        // (smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay) * kPersistentCongestionThreshold
        // = (100 + max(0, 1) + 10) * 3 = 333
        assert_eq!(
            Duration::from_millis(333),
            rtt_estimator.persistent_congestion_threshold()
        );
    }

    #[test]
    fn set_min_rtt_to_latest_sample_after_persistent_congestion() {
        let mut rtt_estimator = RTTEstimator::new(Duration::from_millis(10));
        let now = NoopClock.get_time();
        let mut rtt_sample = Duration::from_millis(500);
        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            rtt_sample,
            now,
            true,
            PacketNumberSpace::Initial,
        );

        assert_eq!(rtt_estimator.min_rtt(), rtt_sample);

        rtt_sample = Duration::from_millis(200);

        rtt_estimator.on_persistent_congestion();

        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            rtt_sample,
            now,
            true,
            PacketNumberSpace::Initial,
        );

        //= https://tools.ietf.org/id/draft-ietf-quic-recovery-32.txt#5.2
        //= type=test
        //# Endpoints SHOULD set the min_rtt to the newest RTT sample after
        //# persistent congestion is established.
        assert_eq!(rtt_estimator.min_rtt(), rtt_sample);
        assert_eq!(rtt_estimator.smoothed_rtt(), rtt_sample);
    }
}
