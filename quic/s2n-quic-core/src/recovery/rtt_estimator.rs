use crate::packet::number::PacketNumberSpace;
use core::{cmp::min, time::Duration};

//= https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.2
//# When no previous RTT is available, the initial RTT SHOULD be set to 333ms,
//# resulting in a 1 second initial timeout, as recommended in [RFC6298].
const DEFAULT_INITIAL_RTT: Duration = Duration::from_millis(333);
const ZERO_DURATION: Duration = Duration::from_millis(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RTTEstimator {
    /// Latest RTT sample
    pub latest_rtt: Duration,
    /// The minimum value observed over the lifetime of the connection
    pub min_rtt: Duration,
    /// An exponentially-weighted moving average
    pub smoothed_rtt: Duration,
    /// The variance in the observed RTT samples
    pub rttvar: Duration,
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
}

impl RTTEstimator {
    pub fn update_rtt(
        &mut self,
        mut ack_delay: Duration,
        rtt_sample: Duration,
        space: PacketNumberSpace,
    ) {
        self.latest_rtt = rtt_sample;

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
