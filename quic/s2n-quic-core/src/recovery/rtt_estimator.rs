use crate::frame::{ack::Ack, ack::AckRanges, ack_elicitation::AckElicitable};
use crate::packet::number::{PacketNumber, PacketNumberSpace};
use crate::time::Timestamp;
use core::{cmp::min, time::Duration, u64};

#[compliance::implements(
    /// When no previous RTT is available, the initial RTT SHOULD be set to 333ms,
    /// resulting in a 1 second initial timeout, as recommended in [RFC6298].
    "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.2"
)]
const DEFAULT_INITIAL_RTT_MILLISECONDS: u64 = 333;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RTTEstimator {
    /// Latest RTT sample,
    pub latest_rtt: Duration,
    /// The minimum value observed over the lifetime of the connection
    pub min_rtt: Duration,
    /// An exponentially-weighted moving average
    pub smoothed_rtt: Duration,
    /// The variance in the observed RTT samples
    pub rttvar: Duration,
    pub max_ack_delay: Duration,
    pub is_first_rtt_sample: bool,
}

impl RTTEstimator {
    fn new(max_ack_delay: Duration) -> Self {
        let initial_rtt = Duration::from_millis(DEFAULT_INITIAL_RTT_MILLISECONDS);
        compliance::citation!(
            /// When there are no samples for a network path, and on the first RTT
            /// sample for the network path:
            ///
            /// smoothed_rtt = rtt_sample
            /// rttvar = rtt_sample / 2
            ///
            /// Before any RTT samples are available, the initial RTT is used as
            /// rtt_sample.
            "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#5.3"
        );
        Self {
            latest_rtt: Duration::from_millis(0),
            min_rtt: Duration::from_millis(0),
            smoothed_rtt: initial_rtt,
            rttvar: initial_rtt / 2,
            max_ack_delay,
            is_first_rtt_sample: true,
        }
    }

    fn resume(max_ack_delay: Duration, smoothed_rtt: Duration) -> Self {
        compliance::citation!(
            /// Resumed connections over the same network MAY use the previous
            /// connection's final smoothed RTT value as the resumed connection's
            /// initial RTT.
            "https://tools.ietf.org/id/draft-ietf-quic-recovery-29.txt#6.2.2"
        );
        Self {
            latest_rtt: smoothed_rtt,
            min_rtt: smoothed_rtt,
            smoothed_rtt,
            rttvar: smoothed_rtt / 2,
            max_ack_delay,
            is_first_rtt_sample: false,
        }
    }
}

impl RTTEstimator {
    pub fn on_ack_received<A: AckRanges>(
        &mut self,
        largest_acked_packet_number: &mut Option<PacketNumber>,
        send_time_of_largest_acked: &mut Timestamp,
        ack: &Ack<A>,
        ack_time: &Timestamp,
        packet_number_space: PacketNumberSpace,
    ) -> bool {
        let ack_largest_acknowledged =
            packet_number_space.new_packet_number(ack.largest_acknowledged());

        //= https://tools.ietf.org/html/draft-ietf-quic-recovery-23#section-4.1
        //# 4.1.  Generating RTT samples
        //#
        //#    An endpoint generates an RTT sample on receiving an ACK frame that
        //#    meets the following two conditions:
        //#
        //#    o  the largest acknowledged packet number is newly acknowledged, and

        // If the largest acked packet number does not exist or is less than the currnet ack's largest, then continue, otherwise abort
        if !largest_acked_packet_number
            .map(|largest| ack_largest_acknowledged > largest)
            .unwrap_or(true)
        {
            return false;
        }

        //#    o  at least one of the newly acknowledged packets was ack-eliciting.

        if !ack.ack_elicitation().is_ack_eliciting() {
            return false;
        }

        //#    The RTT sample, latest_rtt, is generated as the time elapsed since
        //#    the largest acknowledged packet was sent:
        //#
        //#    latest_rtt = ack_time - send_time_of_largest_acked

        self.latest_rtt = *ack_time - *send_time_of_largest_acked;

        // Update the packet space
        *largest_acked_packet_number = Some(ack_largest_acknowledged);
        *send_time_of_largest_acked = *ack_time;

        //#    An RTT sample is generated using only the largest acknowledged packet
        //#    in the received ACK frame.  This is because a peer reports host
        //#    delays for only the largest acknowledged packet in an ACK frame.
        //#    While the reported host delay is not used by the RTT sample
        //#    measurement, it is used to adjust the RTT sample in subsequent
        //#    computations of smoothed_rtt and rttvar Section 4.3.
        //#
        //#    To avoid generating multiple RTT samples using the same packet, an
        //#    ACK frame SHOULD NOT be used to update RTT estimates if it does not
        //#    newly acknowledge the largest acknowledged packet.
        //#
        //#    An RTT sample MUST NOT be generated on receiving an ACK frame that
        //#    does not newly acknowledge at least one ack-eliciting packet.  A peer
        //#    does not send an ACK frame on receiving only non-ack-eliciting
        //#    packets, so an ACK frame that is subsequently sent can include an
        //#    arbitrarily large Ack Delay field.  Ignoring such ACK frames avoids
        //#    complications in subsequent smoothed_rtt and rttvar computations.
        //#
        //#    A sender might generate multiple RTT samples per RTT when multiple
        //#    ACK frames are received within an RTT.  As suggested in [RFC6298],
        //#    doing so might result in inadequate history in smoothed_rtt and
        //#    rttvar.  Ensuring that RTT estimates retain sufficient history is an
        //#    open research question.

        self.compute_min_rtt();
        self.update_rtt(ack.ack_delay(), packet_number_space);

        true
    }

    #[inline]
    fn compute_min_rtt(&mut self) {
        //= https://tools.ietf.org/html/draft-ietf-quic-recovery-23#section-4.2
        //# 4.2.  Estimating min_rtt
        //#
        //#    min_rtt is the minimum RTT observed over the lifetime of the
        //#    connection.  min_rtt is set to the latest_rtt on the first sample in
        //#    a connection, and to the lesser of min_rtt and latest_rtt on
        //#    subsequent samples.
        //#
        //#    An endpoint uses only locally observed times in computing the min_rtt
        //#    and does not adjust for host delays reported by the peer.  Doing so
        //#    allows the endpoint to set a lower bound for the smoothed_rtt based
        //#    entirely on what it observes (see Section 4.3), and limits potential
        //#    underestimation due to erroneously-reported delays by the peer.

        self.min_rtt = min(self.min_rtt, self.latest_rtt);
    }

    #[inline]
    fn update_rtt(&mut self, mut ack_delay: Duration, packet_number_space: PacketNumberSpace) {
        //= https://tools.ietf.org/html/draft-ietf-quic-recovery-23#section-4.3
        //# 4.3.  Estimating smoothed_rtt and rttvar
        //#
        //#    smoothed_rtt is an exponentially-weighted moving average of an
        //#    endpoint's RTT samples, and rttvar is the endpoint's estimated
        //#    variance in the RTT samples.
        //#
        //#    The calculation of smoothed_rtt uses path latency after adjusting RTT
        //#    samples for host delays.  For packets sent in the ApplicationData
        //#    packet number space, a peer limits any delay in sending an
        //#    acknowledgement for an ack-eliciting packet to no greater than the
        //#    value it advertised in the max_ack_delay transport parameter.
        //#
        //#    Consequently, when a peer reports an Ack Delay that is greater than
        //#    its max_ack_delay, the delay is attributed to reasons out of the
        //#    peer's control, such as scheduler latency at the peer or loss of
        //#    previous ACK frames.  Any delays beyond the peer's max_ack_delay are
        //#    therefore considered effectively part of path delay and incorporated
        //#    into the smoothed_rtt estimate.
        //#
        //#    When adjusting an RTT sample using peer-reported acknowledgement
        //#    delays, an endpoint:
        //#
        //#    o  MUST ignore the Ack Delay field of the ACK frame for packets sent
        //#       in the Initial and Handshake packet number space.

        if packet_number_space.is_initial() || packet_number_space.is_handshake() {
            ack_delay = Duration::from_millis(0);
        }

        //#    o  MUST use the lesser of the value reported in Ack Delay field of
        //#       the ACK frame and the peer's max_ack_delay transport parameter.
        //#
        //#    o  MUST NOT apply the adjustment if the resulting RTT sample is
        //#       smaller than the min_rtt.  This limits the underestimation that a
        //#       misreporting peer can cause to the smoothed_rtt.
        //#
        //#    On the first RTT sample in a connection, the smoothed_rtt is set to
        //#    the latest_rtt.
        //#
        //#    smoothed_rtt and rttvar are computed as follows, similar to
        //#    [RFC6298].  On the first RTT sample in a connection:
        //#
        //#    smoothed_rtt = latest_rtt
        //#    rttvar = latest_rtt / 2

        if self.is_first_rtt_sample {
            self.smoothed_rtt = self.latest_rtt;
            self.rttvar = self.latest_rtt / 2;
            return;
        } else {
            //#    On subsequent RTT samples, smoothed_rtt and rttvar evolve as follows:
            //#
            //#    ack_delay = min(Ack Delay in ACK Frame, max_ack_delay)
            //#    adjusted_rtt = latest_rtt
            //#    if (min_rtt + ack_delay < latest_rtt):
            //#      adjusted_rtt = latest_rtt - ack_delay
            //#    smoothed_rtt = 7/8 * smoothed_rtt + 1/8 * adjusted_rtt
            //#    rttvar_sample = abs(smoothed_rtt - adjusted_rtt)
            //#    rttvar = 3/4 * rttvar + 1/4 * rttvar_sample

            let ack_delay = min(ack_delay, self.max_ack_delay);

            let mut adjusted_rtt = self.latest_rtt;

            if self.min_rtt + ack_delay < adjusted_rtt {
                adjusted_rtt -= ack_delay;
            }

            self.smoothed_rtt = 7 * self.smoothed_rtt / 8 + adjusted_rtt / 8;
            let rttvar_sample = abs_difference(self.smoothed_rtt, adjusted_rtt);
            self.rttvar = 3 * self.rttvar / 4 + rttvar_sample / 4;
        }
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
