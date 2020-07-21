use crate::packet::number::PacketNumberSpace;
use core::{cmp::min, time::Duration, u64};

#[compliance::implements(
    /// When no previous RTT is available, the initial RTT SHOULD be set to 333ms,
    /// resulting in a 1 second initial timeout, as recommended in [RFC6298].
    "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.2")]
const DEFAULT_INITIAL_RTT_MILLISECONDS: u64 = 333;

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
    pub max_ack_delay: Duration,
    /// True if no rtt samples have been received yet
    pub is_first_rtt_sample: bool,
}

impl RTTEstimator {

    /// Creates a new RTT Estimator with default inital values using the given `max_ack_delay`.
    fn new(max_ack_delay: Duration) -> Self {
        compliance::citation!(
            /// Before any RTT samples are available, the initial RTT is used as rtt_sample.
            "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3");
        let rtt_sample = Duration::from_millis(DEFAULT_INITIAL_RTT_MILLISECONDS);
        compliance::citation!(
            /// When there are no samples for a network path, and on the first RTT
            /// sample for the network path:
            ///
            /// smoothed_rtt = rtt_sample
            /// rttvar = rtt_sample / 2
            "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3");
        Self {
            latest_rtt: Duration::from_millis(0),
            min_rtt: Duration::from_millis(0),
            smoothed_rtt: rtt_sample,
            rttvar: rtt_sample / 2,
            max_ack_delay,
            is_first_rtt_sample: true,
        }
    }
}

impl RTTEstimator {

    pub fn update_rtt(&mut self, mut ack_delay: Duration, rtt_sample: Duration, space: PacketNumberSpace) {

        self.latest_rtt = rtt_sample;

        compliance::citation!(
            /// min_rtt is set to the latest_rtt on the first RTT sample,
            "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.2");
        if self.is_first_rtt_sample {
            self.min_rtt = self.latest_rtt;
            self.smoothed_rtt = self.latest_rtt;
            self.rttvar = self.latest_rtt / 2;
            self.is_first_rtt_sample = false;
            return
        }

        compliance::citation!(
            /// and to the lesser of min_rtt and latest_rtt on
            /// subsequent samples.
            "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.2");
        self.min_rtt = min(self.min_rtt, self.latest_rtt);

        compliance::citation!(
            /// When adjusting an RTT sample using peer-reported acknowledgement
            /// delays, an endpoint:
            ///
            /// *  MUST ignore the Ack Delay field of the ACK frame for packets sent in the Initial
            ///    and Handshake packet number space.
            "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3");
        if space.is_initial() || space.is_handshake() {
            ack_delay = Duration::from_millis(0);
        }

        compliance::citation!(
            /// *  MUST use the lesser of the value reported in Ack Delay field of
            ///    the ACK frame and the peer's max_ack_delay transport parameter.
            "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3");
        let ack_delay = min(ack_delay, self.max_ack_delay);

        let mut adjusted_rtt = self.latest_rtt;

        compliance::citation!(
            /// *  MUST NOT apply the adjustment if the resulting RTT sample
            ///    is smaller than the min_rtt.
            "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3");
        if self.min_rtt + ack_delay < self.latest_rtt {
            adjusted_rtt -= ack_delay;
        }

        compliance::citation!(
        /// On subsequent RTT samples, smoothed_rtt and rttvar evolve as follows:
        ///
        /// ack_delay = min(Ack Delay in ACK Frame, max_ack_delay)
        /// adjusted_rtt = latest_rtt
        /// if (min_rtt + ack_delay < latest_rtt):
        /// adjusted_rtt = latest_rtt - ack_delay
        /// smoothed_rtt = 7/8 * smoothed_rtt + 1/8 * adjusted_rtt
        /// rttvar_sample = abs(smoothed_rtt - adjusted_rtt)
        /// rttvar = 3/4 * rttvar + 1/4 * rttvar_sample
        "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3");
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

// TODO add tests https://cs.chromium.org/chromium/src/net/third_party/quiche/src/quic/core/congestion_control/rtt_stats_test.cc?g=0
