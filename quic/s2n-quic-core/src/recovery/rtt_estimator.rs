// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    packet::number::PacketNumberSpace, time::Timestamp, transport::parameters::MaxAckDelay,
};
use core::{
    cmp::{max, min},
    time::Duration,
};

//= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.2
//# When no previous RTT is available, the initial RTT
//# SHOULD be set to 333 milliseconds.  This results in handshakes
//# starting with a PTO of 1 second, as recommended for TCP's initial
//# RTO; see Section 2 of [RFC6298].
pub const DEFAULT_INITIAL_RTT: Duration = Duration::from_millis(333);

/// The lowest RTT value that the RTT Estimator is capable of tracking
pub const MIN_RTT: Duration = Duration::from_micros(1);

const ZERO_DURATION: Duration = Duration::from_millis(0);

//= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.2
//# The RECOMMENDED value of the timer granularity (kGranularity) is 1 millisecond.
pub const K_GRANULARITY: Duration = Duration::from_millis(1);

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.1
//# The RECOMMENDED value for kPersistentCongestionThreshold is 3, which
//# results in behavior that is approximately equivalent to a TCP sender
//# declaring an RTO after two TLPs.
const K_PERSISTENT_CONGESTION_THRESHOLD: u64 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RttEstimator {
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

impl Default for RttEstimator {
    /// Creates a new RTT Estimator with default initial values
    fn default() -> Self {
        RttEstimator::new(DEFAULT_INITIAL_RTT)
    }
}

impl RttEstimator {
    /// Creates a new RTT Estimator with the given `initial_rtt`
    ///
    /// `on_max_ack_delay` must be called when the `max_ack_delay` transport
    /// parameter is received to initialize the `max_ack_delay` value.
    #[inline]
    pub fn new(initial_rtt: Duration) -> Self {
        Self::new_with_max_ack_delay(Duration::ZERO, initial_rtt)
    }

    /// Creates a new RTT Estimator with the provided initial values using the given `max_ack_delay`.
    #[inline]
    fn new_with_max_ack_delay(max_ack_delay: Duration, initial_rtt: Duration) -> Self {
        debug_assert!(initial_rtt >= MIN_RTT);
        let initial_rtt = initial_rtt.max(MIN_RTT);

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
        //# Before any RTT samples are available for a new path or when the
        //# estimator is reset, the estimator is initialized using the initial RTT;
        //# see Section 6.2.2.
        //#
        //# smoothed_rtt and rttvar are initialized as follows, where kInitialRtt
        //# contains the initial RTT value:
        //
        //# smoothed_rtt = kInitialRtt
        //# rttvar = kInitialRtt / 2
        let smoothed_rtt = initial_rtt;
        let rttvar = initial_rtt / 2;

        Self {
            latest_rtt: initial_rtt,
            min_rtt: initial_rtt,
            smoothed_rtt,
            rttvar,
            max_ack_delay,
            first_rtt_sample: None,
        }
    }

    /// Creates a new RTT Estimator with the `max_ack_delay` from the current instance
    pub fn for_new_path(&self, initial_rtt: Duration) -> Self {
        Self::new_with_max_ack_delay(self.max_ack_delay, initial_rtt)
    }

    /// Gets the latest round trip time sample
    #[inline]
    pub fn latest_rtt(&self) -> Duration {
        self.latest_rtt
    }

    /// Gets the weighted average round trip time
    #[inline]
    pub fn smoothed_rtt(&self) -> Duration {
        self.smoothed_rtt
    }

    /// Gets the minimum round trip time
    #[inline]
    pub fn min_rtt(&self) -> Duration {
        self.min_rtt
    }

    /// Gets the variance in observed round trip time samples
    #[inline]
    pub fn rttvar(&self) -> Duration {
        self.rttvar
    }

    /// Gets the timestamp of the first RTT sample
    #[inline]
    pub fn first_rtt_sample(&self) -> Option<Timestamp> {
        self.first_rtt_sample
    }

    /// Gets the max_ack_delay
    #[inline]
    pub fn max_ack_delay(&self) -> Duration {
        self.max_ack_delay
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
    //# The PTO period is the amount of time that a sender ought to wait for
    //# an acknowledgement of a sent packet.
    #[inline]
    pub fn pto_period(&self, pto_backoff: u32, space: PacketNumberSpace) -> Duration {
        // We operate on microseconds rather than `Duration` to improve efficiency.
        // See https://godbolt.org/z/osEd9rj9a

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
        //# When an ack-eliciting packet is transmitted, the sender schedules a
        //# timer for the PTO period as follows:
        //#
        //# PTO = smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay
        let mut pto_period = self.smoothed_rtt().as_micros() as u64;

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
        //# The PTO period MUST be at least kGranularity, to avoid the timer
        //# expiring immediately.
        pto_period += max(
            self.rttvar_4x().as_micros() as u64,
            K_GRANULARITY.as_micros() as u64,
        );

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
        //# When the PTO is armed for Initial or Handshake packet number spaces,
        //# the max_ack_delay in the PTO period computation is set to 0, since
        //# the peer is expected to not delay these packets intentionally; see
        //# Section 13.2.1 of [QUIC-TRANSPORT].
        if space.is_application_data() {
            pto_period += self.max_ack_delay.as_micros() as u64;
        }

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
        //# Even when there are ack-eliciting packets in flight in multiple
        //# packet number spaces, the exponential increase in PTO occurs across
        //# all spaces to prevent excess load on the network.  For example, a
        //# timeout in the Initial packet number space doubles the length of
        //# the timeout in the Handshake packet number space.
        pto_period *= pto_backoff as u64;

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
        //# The PTO period is the amount of time that a sender ought to wait for
        //# an acknowledgement of a sent packet.
        Duration::from_micros(pto_period)
    }

    /// Sets the `max_ack_delay` value from the peer `MaxAckDelay` transport parameter
    pub fn on_max_ack_delay(&mut self, max_ack_delay: MaxAckDelay) {
        self.max_ack_delay = max_ack_delay.as_duration()
    }

    /// Updates the RTT estimate using the given `rtt_sample`
    #[inline]
    pub fn update_rtt(
        &mut self,
        mut ack_delay: Duration,
        rtt_sample: Duration,
        timestamp: Timestamp,
        is_handshake_confirmed: bool,
        space: PacketNumberSpace,
    ) {
        self.latest_rtt = rtt_sample.max(MIN_RTT);

        if self.first_rtt_sample.is_none() {
            self.first_rtt_sample = Some(timestamp);
            //= https://www.rfc-editor.org/rfc/rfc9002#section-5.2
            //# min_rtt MUST be set to the latest_rtt on the first RTT sample.
            self.min_rtt = self.latest_rtt;
            //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
            //# On the first RTT sample after initialization, smoothed_rtt and rttvar
            //# are set as follows:
            //#
            //# smoothed_rtt = latest_rtt
            //# rttvar = latest_rtt / 2
            self.smoothed_rtt = self.latest_rtt;
            self.rttvar = self.latest_rtt / 2;
            return;
        }

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.2
        //# min_rtt MUST be set to the lesser of min_rtt and latest_rtt
        //# (Section 5.1) on all other samples.
        self.min_rtt = min(self.min_rtt, self.latest_rtt);

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
        //# when adjusting an RTT sample using peer-reported
        //# acknowledgment delays, an endpoint:
        //#
        //# *  MAY ignore the acknowledgment delay for Initial packets, since
        //#    these acknowledgments are not delayed by the peer (Section 13.2.1
        //#    of [QUIC-TRANSPORT]);
        if space.is_initial() {
            ack_delay = ZERO_DURATION;
        }

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
        //# To account for this, the endpoint SHOULD ignore
        //# max_ack_delay until the handshake is confirmed, as defined in
        //# Section 4.1.2 of [QUIC-TLS].

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
        //# *  SHOULD ignore the peer's max_ack_delay until the handshake is
        //#    confirmed;
        if is_handshake_confirmed {
            //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
            //# *  MUST use the lesser of the acknowledgement delay and the peer's
            //#    max_ack_delay after the handshake is confirmed; and
            ack_delay = min(ack_delay, self.max_ack_delay);
        }

        let mut adjusted_rtt = self.latest_rtt;

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
        //# *  MUST NOT subtract the acknowledgement delay from the RTT sample if
        //#    the resulting value is smaller than the min_rtt.
        if self.min_rtt + ack_delay < self.latest_rtt {
            adjusted_rtt -= ack_delay;
        } else if !is_handshake_confirmed {
            //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
            //# Therefore, prior to handshake
            //# confirmation, an endpoint MAY ignore RTT samples if adjusting the RTT
            //# sample for acknowledgement delay causes the sample to be less than
            //# the min_rtt.
            return;
        }

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
        //# On subsequent RTT samples, smoothed_rtt and rttvar evolve as follows:
        //#
        //# ack_delay = decoded acknowledgment delay from ACK frame
        //# if (handshake confirmed):
        //# ack_delay = min(ack_delay, max_ack_delay)
        //# adjusted_rtt = latest_rtt
        //# if (latest_rtt >= min_rtt + ack_delay):
        //#     adjusted_rtt = latest_rtt - ack_delay
        //# smoothed_rtt = 7/8 * smoothed_rtt + 1/8 * adjusted_rtt
        //# rttvar_sample = abs(smoothed_rtt - adjusted_rtt)
        //# rttvar = 3/4 * rttvar + 1/4 * rttvar_sample

        // this logic has been updated to follow the errata reported in https://www.rfc-editor.org/errata/eid7539
        let rttvar_sample = abs_difference(self.smoothed_rtt, adjusted_rtt);
        self.rttvar = weighted_average(self.rttvar, rttvar_sample, 4);
        self.smoothed_rtt = weighted_average(self.smoothed_rtt, adjusted_rtt, 8);
    }

    /// Calculates the persistent congestion threshold used for determining
    /// if persistent congestion is being encountered.
    #[inline]
    pub fn persistent_congestion_threshold(&self) -> Duration {
        // Since K_GRANULARITY is 1ms, we operate on milliseconds rather than `Duration` to improve efficiency.
        // See https://godbolt.org/z/4o71WPods

        //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.1
        //# The persistent congestion duration is computed as follows:
        //#
        //# (smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay) *
        //#     kPersistentCongestionThreshold
        //#
        //# Unlike the PTO computation in Section 6.2, this duration includes the
        //# max_ack_delay irrespective of the packet number spaces in which
        //# losses are established.
        //#
        //# This duration allows a sender to send as many packets before
        //# establishing persistent congestion, including some in response to PTO
        //# expiration, as TCP does with Tail Loss Probes [RFC8985] and an RTO
        //# [RFC5681].
        Duration::from_millis(
            (self.smoothed_rtt.as_millis() as u64
                + max(
                    self.rttvar_4x().as_millis() as u64,
                    K_GRANULARITY.as_millis() as u64,
                )
                + self.max_ack_delay.as_millis() as u64)
                * K_PERSISTENT_CONGESTION_THRESHOLD,
        )
    }

    #[inline]
    pub fn loss_time_threshold(&self) -> Duration {
        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.2
        //# The time threshold is:
        //#
        //# max(kTimeThreshold * max(smoothed_rtt, latest_rtt), kGranularity)
        let mut time_threshold = max(
            self.smoothed_rtt().as_nanos() as u64,
            self.latest_rtt().as_nanos() as u64,
        );

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.2
        //# The RECOMMENDED time threshold (kTimeThreshold), expressed as an
        //# RTT multiplier, is 9/8.
        time_threshold += time_threshold / 8;

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.2
        //# To avoid declaring
        //# packets as lost too early, this time threshold MUST be set to at
        //# least the local timer granularity, as indicated by the kGranularity
        //# constant.
        let time_threshold = max(time_threshold, K_GRANULARITY.as_nanos() as u64);

        Duration::from_nanos(time_threshold)
    }

    /// Allows min_rtt and smoothed_rtt to be overwritten on the next RTT sample
    /// after persistent congestion is established.
    #[inline]
    pub fn on_persistent_congestion(&mut self) {
        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.2
        //# Endpoints SHOULD set the min_rtt to the newest RTT sample after
        //# persistent congestion is established.
        self.first_rtt_sample = None;
    }

    #[inline]
    fn rttvar_4x(&self) -> Duration {
        // Operate on micros instead, as it's more efficient and we don't need the precision Duration gives
        Duration::from_micros(4 * self.rttvar.as_micros() as u64)
    }
}

#[inline]
fn abs_difference<T: core::ops::Sub + PartialOrd>(a: T, b: T) -> <T as core::ops::Sub>::Output {
    if a > b {
        a - b
    } else {
        b - a
    }
}

/// Optimized function for averaging two durations with a weight
/// See https://godbolt.org/z/65f9bYEcs
#[inline]
fn weighted_average(a: Duration, b: Duration, weight: u64) -> Duration {
    let mut a = a.as_nanos() as u64;
    // it's more accurate to multiply first but it risks overflow so we divide first
    a /= weight;
    a *= weight - 1;

    let mut b = b.as_nanos() as u64;
    b /= weight;

    Duration::from_nanos(a + b)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        packet::number::PacketNumberSpace,
        path::INITIAL_PTO_BACKOFF,
        time::{Clock, Duration, NoopClock},
        transport::parameters::MaxAckDelay,
        varint::VarInt,
    };

    /// Test the initial values before any RTT samples
    #[test]
    fn initial_rtt_across_spaces() {
        let rtt_estimator =
            RttEstimator::new_with_max_ack_delay(Duration::from_millis(10), DEFAULT_INITIAL_RTT);
        assert_eq!(rtt_estimator.min_rtt, DEFAULT_INITIAL_RTT);
        assert_eq!(rtt_estimator.latest_rtt(), DEFAULT_INITIAL_RTT);
        assert_eq!(rtt_estimator.smoothed_rtt(), DEFAULT_INITIAL_RTT);
        assert_eq!(rtt_estimator.rttvar(), DEFAULT_INITIAL_RTT / 2);
        assert_eq!(
            rtt_estimator.pto_period(INITIAL_PTO_BACKOFF, PacketNumberSpace::Initial),
            Duration::from_millis(999)
        );
        assert_eq!(
            rtt_estimator.pto_period(INITIAL_PTO_BACKOFF, PacketNumberSpace::Handshake),
            Duration::from_millis(999)
        );
        assert_eq!(
            rtt_estimator.pto_period(INITIAL_PTO_BACKOFF, PacketNumberSpace::ApplicationData),
            Duration::from_millis(1009)
        );
    }

    /// Test a zero RTT value is treated as 1 µs
    #[test]
    fn zero_rtt_sample() {
        let mut rtt_estimator = RttEstimator::new(DEFAULT_INITIAL_RTT);
        let now = NoopClock.get_time();
        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            Duration::from_millis(0),
            now,
            false,
            PacketNumberSpace::ApplicationData,
        );
        assert_eq!(rtt_estimator.min_rtt, MIN_RTT);
        assert_eq!(rtt_estimator.latest_rtt(), MIN_RTT);
        assert_eq!(rtt_estimator.first_rtt_sample(), Some(now));
        assert_eq!(
            rtt_estimator.pto_period(INITIAL_PTO_BACKOFF, PacketNumberSpace::Initial),
            Duration::from_micros(1001)
        );
    }

    #[test]
    fn for_new_path() {
        let mut rtt_estimator = RttEstimator::default();
        let max_ack_delay = Duration::from_millis(10);
        rtt_estimator.on_max_ack_delay(max_ack_delay.try_into().unwrap());
        let new_path_rtt_estimator = rtt_estimator.for_new_path(DEFAULT_INITIAL_RTT);
        assert_eq!(max_ack_delay, new_path_rtt_estimator.max_ack_delay)
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
    //= type=test
    //# *  MUST use the lesser of the acknowledgement delay and the peer's
    //#    max_ack_delay after the handshake is confirmed;
    #[test]
    fn max_ack_delay() {
        let mut rtt_estimator = RttEstimator::default();
        assert_eq!(Duration::ZERO, rtt_estimator.max_ack_delay);

        rtt_estimator.on_max_ack_delay(MaxAckDelay::new(VarInt::from_u8(10)).unwrap());
        assert_eq!(Duration::from_millis(10), rtt_estimator.max_ack_delay);

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

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
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

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
        //= type=test
        //# To account for this, the endpoint SHOULD ignore
        //# max_ack_delay until the handshake is confirmed, as defined in
        //# Section 4.1.2 of [QUIC-TLS].

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
        //= type=test
        //# *  SHOULD ignore the peer's max_ack_delay until the handshake is
        //# confirmed;
        assert_eq!(
            rtt_estimator.smoothed_rtt,
            7 * prev_smoothed_rtt / 8 + Duration::from_millis(200 - 50) / 8
        );
    }

    /// Test several rounds of RTT updates
    #[test]
    fn update_rtt() {
        let mut rtt_estimator =
            RttEstimator::new_with_max_ack_delay(Duration::from_millis(10), DEFAULT_INITIAL_RTT);
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

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.2
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

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.2
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
        assert_eq!(
            rtt_estimator.pto_period(INITIAL_PTO_BACKOFF, PacketNumberSpace::ApplicationData),
            Duration::from_micros(1620466)
        );
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
    //= type=test
    //# *  MUST NOT subtract the acknowledgement delay from the RTT sample if
    //#    the resulting value is smaller than the min_rtt.
    #[test]
    fn must_not_subtract_acknowledgement_delay_if_result_smaller_than_min_rtt() {
        let mut rtt_estimator =
            RttEstimator::new_with_max_ack_delay(Duration::from_millis(200), DEFAULT_INITIAL_RTT);
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

    //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
    //= type=test
    //# Therefore, prior to handshake
    //# confirmation, an endpoint MAY ignore RTT samples if adjusting the RTT
    //# sample for acknowledgement delay causes the sample to be less than
    //# the min_rtt.
    #[test]
    fn prior_to_handshake_ignore_if_less_than_min_rtt() {
        let mut rtt_estimator =
            RttEstimator::new_with_max_ack_delay(Duration::from_millis(200), DEFAULT_INITIAL_RTT);
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

    //= https://www.rfc-editor.org/rfc/rfc9002#section-5.3
    //= type=test
    //# *  MAY ignore the acknowledgment delay for Initial packets, since
    //     these acknowledgments are not delayed by the peer (Section 13.2.1
    //     of [QUIC-TRANSPORT]);
    #[test]
    fn initial_space() {
        let mut rtt_estimator =
            RttEstimator::new_with_max_ack_delay(Duration::from_millis(10), DEFAULT_INITIAL_RTT);
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

    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.1
    //= type=test
    //# The persistent congestion duration is computed as follows:
    //#
    //# (smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay) *
    //#   kPersistentCongestionThreshold
    #[test]
    fn persistent_congestion_duration() {
        let max_ack_delay = Duration::from_millis(10);
        let mut rtt_estimator =
            RttEstimator::new_with_max_ack_delay(max_ack_delay, DEFAULT_INITIAL_RTT);

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

        //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.1
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
        let mut rtt_estimator =
            RttEstimator::new_with_max_ack_delay(Duration::from_millis(10), DEFAULT_INITIAL_RTT);
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

        //= https://www.rfc-editor.org/rfc/rfc9002#section-5.2
        //= type=test
        //# Endpoints SHOULD set the min_rtt to the newest RTT sample after
        //# persistent congestion is established.
        assert_eq!(rtt_estimator.min_rtt(), rtt_sample);
        assert_eq!(rtt_estimator.smoothed_rtt(), rtt_sample);
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
    //= type=test
    //# The PTO period MUST be at least kGranularity, to avoid the timer
    //# expiring immediately.
    #[test]
    fn pto_must_be_at_least_k_granularity() {
        let space = PacketNumberSpace::Handshake;
        let now = NoopClock.get_time();
        let mut rtt_estimator = RttEstimator::new(DEFAULT_INITIAL_RTT);

        // Update RTT with the smallest possible sample
        rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_nanos(1),
            now,
            true,
            space,
        );

        let pto_period = rtt_estimator.pto_period(INITIAL_PTO_BACKOFF, space);
        assert!(pto_period >= K_GRANULARITY);

        // pto_period should have microsecond precision
        assert_eq!(pto_period, Duration::from_micros(1001))
    }

    #[test]
    #[cfg_attr(kani, kani::proof, kani::unwind(3), kani::solver(cadical))]
    #[cfg_attr(miri, ignore)] // This test is too expensive for miri to complete in a reasonable amount of time
    fn weighted_average_test() {
        bolero::check!()
            .with_type::<(u32, u32)>()
            .for_each(|(a, b)| {
                let a = Duration::from_nanos(*a as _);
                let b = Duration::from_nanos(*b as _);

                let weight = 8;

                // perform the unoptimized version
                let expected = ((weight - 1) * a) / weight + b / weight;
                let actual = super::weighted_average(a, b, weight as _);

                // assert that the unoptimized result matches the optimized to the nearest `weight` nanos
                assert!(
                    super::abs_difference(expected.as_nanos(), actual.as_nanos()) as u32 <= weight,
                    "expected: {expected:?}; actual: {actual:?}"
                );
            })
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.2
    //= type=test
    //# The RECOMMENDED time threshold (kTimeThreshold), expressed as an
    //# RTT multiplier, is 9/8.
    #[test]
    fn time_threshold_multiplier_equals_nine_eighths() {
        let mut rtt_estimator =
            RttEstimator::new_with_max_ack_delay(Duration::from_millis(10), DEFAULT_INITIAL_RTT);
        rtt_estimator.update_rtt(
            Duration::from_millis(10),
            Duration::from_secs(1),
            NoopClock.get_time(),
            true,
            PacketNumberSpace::Initial,
        );
        assert_eq!(
            Duration::from_millis(1125), // 9/8 seconds = 1.125 seconds
            rtt_estimator.loss_time_threshold()
        );
    }

    #[test]
    fn timer_granularity() {
        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.2
        //= type=test
        //# The RECOMMENDED value of the
        //# timer granularity (kGranularity) is 1 millisecond.
        assert_eq!(Duration::from_millis(1), K_GRANULARITY);

        let mut rtt_estimator = RttEstimator::default();
        rtt_estimator.update_rtt(
            Duration::from_millis(0),
            Duration::from_nanos(1),
            NoopClock.get_time(),
            true,
            PacketNumberSpace::Initial,
        );

        //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.2
        //= type=test
        //# To avoid declaring
        //# packets as lost too early, this time threshold MUST be set to at
        //# least the local timer granularity, as indicated by the kGranularity
        //# constant.
        assert!(rtt_estimator.loss_time_threshold() >= K_GRANULARITY);
    }
}
