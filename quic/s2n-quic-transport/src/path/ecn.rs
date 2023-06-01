// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::ops::RangeInclusive;
use s2n_quic_core::{
    counter::{Counter, Saturating},
    event,
    event::IntoEvent,
    frame::ack::EcnCounts,
    inet::ExplicitCongestionNotification,
    number::CheckedSub,
    random,
    time::{timer, Duration, Timer, Timestamp},
    transmission,
    varint::VarInt,
};

//= https://www.rfc-editor.org/rfc/rfc9000#section-13.4.2
//# If an endpoint has cause to expect that IP packets with an ECT codepoint
//# might be dropped by a faulty network element, the endpoint could set an
//# ECT codepoint for only the first ten outgoing packets on a path, or for
//# a period of three PTOs
const TESTING_PACKET_THRESHOLD: u8 = 10;

// After a failure has been detected, the ecn::Controller will wait this duration
// before testing for ECN support again.
const RETEST_COOL_OFF_DURATION: Duration = Duration::from_secs(60);

// The number of round trip times an ECN capable path will wait before transmitting an ECN-CE marked packet.
const CE_SUPPRESSION_TESTING_RTT_MULTIPLIER: RangeInclusive<u16> = 10..=100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationOutcome {
    /// The path is ECN capable and congestion was experienced
    ///
    /// Contains the incremental count of packets that experienced congestion
    CongestionExperienced(VarInt),
    /// The path failed validation
    Failed,
    /// The path passed validation
    Passed,
    /// Validation was not performed
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    // ECN capability is being tested, tracking the number of ECN marked packets sent
    Testing(u8),
    // ECN capability has been tested, but not validated yet
    Unknown,
    // ECN validation has failed. Validation will be restarted based on the timer
    Failed(Timer),
    // ECN validation has succeeded. CE suppression will be tested based on the timer.
    Capable(Timer),
}

impl IntoEvent<event::builder::EcnState> for &State {
    #[inline]
    fn into_event(self) -> event::builder::EcnState {
        match self {
            State::Testing(_) => event::builder::EcnState::Testing,
            State::Unknown => event::builder::EcnState::Unknown,
            State::Failed(_) => event::builder::EcnState::Failed,
            State::Capable(_) => event::builder::EcnState::Capable,
        }
    }
}

impl Default for State {
    fn default() -> Self {
        State::Testing(0)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Controller {
    state: State,
    // A count of the number of packets with ECN marking lost since
    // the last time a packet with ECN marking was acknowledged.
    black_hole_counter: Counter<u8, Saturating>,
    // The largest acknowledged packet sent with an ECN marking. Used when tracking
    // packets that have been lost for the purpose of detecting a black hole.
    last_acked_ecn_packet_timestamp: Option<Timestamp>,
}

impl Controller {
    /// Restart testing of ECN capability
    pub fn restart<Pub: event::ConnectionPublisher>(
        &mut self,
        path: event::builder::Path,
        publisher: &mut Pub,
    ) {
        if self.state != State::Testing(0) {
            self.change_state(State::Testing(0), path, publisher);
        }
        self.black_hole_counter = Default::default();
    }

    /// Called when the connection timer expires
    pub fn on_timeout<Pub: event::ConnectionPublisher>(
        &mut self,
        now: Timestamp,
        path: event::builder::Path,
        random_generator: &mut dyn random::Generator,
        rtt: Duration,
        publisher: &mut Pub,
    ) {
        match self.state {
            State::Failed(ref mut retest_timer) => {
                if retest_timer.poll_expiration(now).is_ready() {
                    self.restart(path, publisher);
                }
            }
            State::Capable(ref mut ce_suppression_timer) if !ce_suppression_timer.is_armed() => {
                ce_suppression_timer
                    .set(now + Self::next_ce_packet_duration(random_generator, rtt));
            }
            State::Testing(_) | State::Unknown | State::Capable(_) => {}
        }
    }

    /// Gets the ECN marking to use on packets sent to the peer
    pub fn ecn(
        &mut self,
        transmission_mode: transmission::Mode,
        now: Timestamp,
    ) -> ExplicitCongestionNotification {
        if transmission_mode.is_loss_recovery_probing() {
            // Don't mark loss recovery probes as ECN capable in case the ECN
            // marking is causing packet loss
            return ExplicitCongestionNotification::NotEct;
        }

        match self.state {
            //= https://www.rfc-editor.org/rfc/rfc9000#appendix-A.4
            //# On paths with a "testing" or "capable" state, the endpoint
            //# sends packets with an ECT marking -- ECT(0) by default;
            //# otherwise, the endpoint sends unmarked packets.
            State::Testing(_) => ExplicitCongestionNotification::Ect0,
            State::Capable(ref mut ce_suppression_timer) => {
                if ce_suppression_timer.poll_expiration(now).is_ready() {
                    //= https://www.rfc-editor.org/rfc/rfc9002#section-8.3
                    //# A sender can detect suppression of reports by marking occasional
                    //# packets that it sends with an ECN-CE marking.
                    ExplicitCongestionNotification::Ce
                } else {
                    //= https://www.rfc-editor.org/rfc/rfc9000#section-13.4.2.2
                    //# Upon successful validation, an endpoint MAY continue to set an ECT
                    //# codepoint in subsequent packets it sends, with the expectation that
                    //# the path is ECN-capable.
                    ExplicitCongestionNotification::Ect0
                }
            }
            //= https://www.rfc-editor.org/rfc/rfc9000#section-13.4.2.2
            //# If validation fails, then the endpoint MUST disable ECN. It stops setting the ECT
            //# codepoint in IP packets that it sends, assuming that either the network path or
            //# the peer does not support ECN.
            State::Failed(_) | State::Unknown => ExplicitCongestionNotification::NotEct,
        }
    }

    /// Returns a duration based on a randomly generated value in the CE_SUPPRESSION_TESTING_RTT_MULTIPLIER
    /// range multiplied by the given round trip time. This duration represents the amount of time
    /// to wait before an ECN-CE marked packet should be sent, to test if CE reports are being
    /// suppressed by the peer.
    ///
    /// Note: This function performs a modulo operation on the random generated bytes to restrict
    /// the result to the `CE_SUPPRESSION_TESTING_RTT_MULTIPLIER` range. This may introduce a modulo
    /// bias in the resulting count, but does not result in any reduction in security for this
    /// usage. Other usages that require uniform sampling should implement rejection sampling or
    /// other methodologies and not copy this implementation.
    fn next_ce_packet_duration(
        random_generator: &mut dyn random::Generator,
        rtt: Duration,
    ) -> Duration {
        let mut bytes = [0; core::mem::size_of::<u16>()];
        random_generator.public_random_fill(&mut bytes);
        let result = u16::from_le_bytes(bytes);

        let max_variance = (CE_SUPPRESSION_TESTING_RTT_MULTIPLIER.end()
            - CE_SUPPRESSION_TESTING_RTT_MULTIPLIER.start())
        .saturating_add(1);
        let result = CE_SUPPRESSION_TESTING_RTT_MULTIPLIER.start() + result % max_variance;
        result as u32 * rtt
    }

    /// Returns true if the path has been determined to be capable of handling ECN marked packets
    pub fn is_capable(&self) -> bool {
        matches!(self.state, State::Capable(_))
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-13.4.2.2
    //# Network routing and path elements can change mid-connection; an endpoint
    //# MUST disable ECN if validation later fails.
    /// Validate the given `EcnCounts`, updating the current validation state based on the
    /// validation outcome.
    ///
    /// * `newly_acked_ecn_counts` - total ECN counts that were sent on packets newly acknowledged by the peer
    /// * `sent_packet_ecn_counts` - total ECN counts for all outstanding packets, including those newly
    ///                              acknowledged during this validation
    /// * `baseline_ecn_counts` - the ECN counts present in the Ack frame the last time ECN counts were processed
    /// * `ack_frame_ecn_counts` - the ECN counts present in the current Ack frame (if any)
    /// * `now` - the time the Ack frame was received
    #[allow(clippy::too_many_arguments)]
    pub fn validate<Pub: event::ConnectionPublisher>(
        &mut self,
        newly_acked_ecn_counts: EcnCounts,
        sent_packet_ecn_counts: EcnCounts,
        baseline_ecn_counts: EcnCounts,
        ack_frame_ecn_counts: Option<EcnCounts>,
        now: Timestamp,
        rtt: Duration,
        path: event::builder::Path,
        publisher: &mut Pub,
    ) -> ValidationOutcome {
        if matches!(self.state, State::Failed(_)) {
            // Validation had already failed
            return ValidationOutcome::Skipped;
        }

        if ack_frame_ecn_counts.is_none() {
            if newly_acked_ecn_counts.as_option().is_some() {
                //= https://www.rfc-editor.org/rfc/rfc9000#section-13.4.2.1
                //# If an ACK frame newly acknowledges a packet that the endpoint sent with
                //# either the ECT(0) or ECT(1) codepoint set, ECN validation fails if the
                //# corresponding ECN counts are not present in the ACK frame. This check
                //# detects a network element that zeroes the ECN field or a peer that does
                //# not report ECN markings.
                self.fail(now, path, publisher);
                return ValidationOutcome::Failed;
            }

            if baseline_ecn_counts == EcnCounts::default() {
                // Nothing to validate
                return ValidationOutcome::Skipped;
            }
        }

        let congestion_experienced_count = if let Some(incremental_ecn_counts) =
            ack_frame_ecn_counts
                .unwrap_or_default()
                .checked_sub(baseline_ecn_counts)
        {
            if Self::ce_remarking(incremental_ecn_counts, newly_acked_ecn_counts)
                || Self::remarked_to_ect0_or_ect1(incremental_ecn_counts, sent_packet_ecn_counts)
                || Self::ce_suppression(incremental_ecn_counts, newly_acked_ecn_counts)
            {
                self.fail(now, path, publisher);
                return ValidationOutcome::Failed;
            }

            // ce_suppression check above ensures this doesn't underflow
            incremental_ecn_counts.ce_count - newly_acked_ecn_counts.ce_count
        } else {
            // ECN counts decreased from the baseline
            self.fail(now, path, publisher);
            return ValidationOutcome::Failed;
        };

        //= https://www.rfc-editor.org/rfc/rfc9000#appendix-A.4
        //# From the "unknown" state, successful validation of the ECN counts in an ACK frame
        //# (see Section 13.4.2.1) causes the ECN state for the path to become "capable",
        //# unless no marked packet has been acknowledged.
        if matches!(self.state, State::Unknown)
            && newly_acked_ecn_counts.ect_0_count > VarInt::from_u8(0)
        {
            // Arm the ce suppression timer to send a ECN-CE marked packet to test for
            // CE suppression by the peer.
            let mut ce_suppression_timer = Timer::default();
            ce_suppression_timer
                .set(now + *CE_SUPPRESSION_TESTING_RTT_MULTIPLIER.start() as u32 * rtt);
            self.change_state(State::Capable(ce_suppression_timer), path, publisher);
        }

        if self.is_capable() && congestion_experienced_count > VarInt::ZERO {
            return ValidationOutcome::CongestionExperienced(congestion_experienced_count);
        }

        ValidationOutcome::Passed
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-13.4.2.1
    //# ECN validation also fails if the sum of the increase in ECT(0)
    //# and ECN-CE counts is less than the number of newly acknowledged
    //# packets that were originally sent with an ECT(0) marking.
    #[inline]
    fn ce_remarking(incremental_ecn_counts: EcnCounts, newly_acked_ecn_counts: EcnCounts) -> bool {
        let ect_0_increase = incremental_ecn_counts
            .ect_0_count
            .saturating_add(incremental_ecn_counts.ce_count);
        ect_0_increase < newly_acked_ecn_counts.ect_0_count
    }

    //= https://www.rfc-editor.org/rfc/rfc9000#section-13.4.2.1
    //# ECN validation can fail if the received total count for either ECT(0) or ECT(1)
    //# exceeds the total number of packets sent with each corresponding ECT codepoint.
    #[inline]
    fn remarked_to_ect0_or_ect1(
        incremental_ecn_counts: EcnCounts,
        sent_packet_ecn_counts: EcnCounts,
    ) -> bool {
        incremental_ecn_counts.ect_0_count > sent_packet_ecn_counts.ect_0_count
            || incremental_ecn_counts.ect_1_count > sent_packet_ecn_counts.ect_1_count
    }

    //= https://www.rfc-editor.org/rfc/rfc9002#section-8.3
    //# A receiver can misreport ECN markings to alter the congestion
    //# response of a sender.  Suppressing reports of ECN-CE markings could
    //# cause a sender to increase their send rate.  This increase could
    //# result in congestion and loss.

    //= https://www.rfc-editor.org/rfc/rfc9002#section-8.3
    //# A sender can detect suppression of reports by marking occasional
    //# packets that it sends with an ECN-CE marking.  If a packet sent with
    //# an ECN-CE marking is not reported as having been CE marked when the
    //# packet is acknowledged, then the sender can disable ECN for that path
    //# by not setting ECN-Capable Transport (ECT) codepoints in subsequent
    //# packets sent on that path [RFC3168].
    #[inline]
    fn ce_suppression(
        incremental_ecn_counts: EcnCounts,
        newly_acked_ecn_counts: EcnCounts,
    ) -> bool {
        incremental_ecn_counts.ce_count < newly_acked_ecn_counts.ce_count
    }

    /// This method gets called when a packet has been sent
    pub fn on_packet_sent<Pub: event::ConnectionPublisher>(
        &mut self,
        ecn: ExplicitCongestionNotification,
        path: event::builder::Path,
        publisher: &mut Pub,
    ) {
        debug_assert!(
            !matches!(ecn, ExplicitCongestionNotification::Ect1),
            "Ect1 is not used"
        );

        if let (true, State::Testing(ref mut packet_count)) = (ecn.using_ecn(), &mut self.state) {
            *packet_count += 1;

            if *packet_count >= TESTING_PACKET_THRESHOLD {
                self.change_state(State::Unknown, path, publisher);
            }
        }
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack(&mut self, time_sent: Timestamp, ecn: ExplicitCongestionNotification) {
        if self.ecn_packet_sent_after_last_acked_ecn_packet(time_sent, ecn) {
            // Reset the black hole counter since a packet with ECN marking
            // has been acknowledged, indicating the path may still be ECN-capable
            self.black_hole_counter = Default::default();
            self.last_acked_ecn_packet_timestamp = Some(time_sent);
        }
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss<Pub: event::ConnectionPublisher>(
        &mut self,
        time_sent: Timestamp,
        ecn: ExplicitCongestionNotification,
        now: Timestamp,
        path: event::builder::Path,
        publisher: &mut Pub,
    ) {
        if matches!(self.state, State::Failed(_)) {
            return;
        }

        if self.ecn_packet_sent_after_last_acked_ecn_packet(time_sent, ecn) {
            // An ECN marked packet that was sent after the last
            // acknowledged ECN marked packet has been lost
            self.black_hole_counter += 1;
        }

        if self.black_hole_counter > TESTING_PACKET_THRESHOLD {
            self.fail(now, path, publisher);
        }
    }

    /// Returns true if a packet sent at the given `time_sent` with the given ECN marking
    /// was marked as using ECN and was sent after the last time an ECN marked packet had
    /// been acknowledged.
    fn ecn_packet_sent_after_last_acked_ecn_packet(
        &mut self,
        time_sent: Timestamp,
        ecn: ExplicitCongestionNotification,
    ) -> bool {
        ecn.using_ecn()
            && self
                .last_acked_ecn_packet_timestamp
                .map_or(true, |last_acked| last_acked < time_sent)
    }

    /// Set the state to Failed and arm the retest timer
    fn fail<Pub: event::ConnectionPublisher>(
        &mut self,
        now: Timestamp,
        path: event::builder::Path,
        publisher: &mut Pub,
    ) {
        //= https://www.rfc-editor.org/rfc/rfc9000#section-13.4.2.2
        //# Even if validation fails, an endpoint MAY revalidate ECN for the same path at any later
        //# time in the connection. An endpoint could continue to periodically attempt validation.
        let mut retest_timer = Timer::default();
        retest_timer.set(now + RETEST_COOL_OFF_DURATION);
        self.change_state(State::Failed(retest_timer), path, publisher);
        self.black_hole_counter = Default::default();
    }

    fn change_state<Pub: event::ConnectionPublisher>(
        &mut self,
        state: State,
        path: event::builder::Path,
        publisher: &mut Pub,
    ) {
        debug_assert_ne!(self.state, state);

        self.state = state;

        publisher.on_ecn_state_changed(event::builder::EcnStateChanged {
            path,
            state: self.state.into_event(),
        })
    }
}

impl timer::Provider for Controller {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        if let State::Failed(timer) = &self.state {
            timer.timers(query)?
        }
        // The ce suppression timer in State::Capable is not queried here as that
        // timer is passively polled when transmitting and does not require firing
        // precisely.

        Ok(())
    }
}

#[cfg(test)]
mod tests;
