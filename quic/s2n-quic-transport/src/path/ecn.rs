// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::path;
use s2n_quic_core::{
    counter::{Counter, Saturating},
    event,
    event::{builder, IntoEvent},
    frame::ack::EcnCounts,
    inet::ExplicitCongestionNotification,
    number::CheckedSub,
    time::{timer, Duration, Timer, Timestamp},
    transmission,
    varint::VarInt,
};

//= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2
//# If an endpoint has cause to expect that IP packets with an ECT codepoint
//# might be dropped by a faulty network element, the endpoint could set an
//# ECT codepoint for only the first ten outgoing packets on a path, or for
//# a period of three PTOs
const TESTING_PACKET_THRESHOLD: u8 = 10;

// After a failure has been detected, the ecn::Controller will wait this duration
// before testing for ECN support again.
const RETEST_COOL_OFF_DURATION: Duration = Duration::from_secs(60);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationOutcome {
    /// The path is ECN capable and congestion was experienced
    CongestionExperienced,
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
    // ECN validation has succeeded
    Capable,
}

impl IntoEvent<builder::EcnState> for &State {
    fn into_event(self) -> builder::EcnState {
        match self {
            State::Testing(_) => builder::EcnState::Testing,
            State::Unknown => builder::EcnState::Unknown,
            State::Failed(_) => builder::EcnState::Failed,
            State::Capable => builder::EcnState::Capable,
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
        path_id: path::Id,
        publisher: &mut Pub,
    ) {
        if self.state != State::Testing(0) {
            self.change_state(State::Testing(0), path_id, publisher);
        }
        self.black_hole_counter = Default::default();
    }

    /// Called when the connection timer expires
    pub fn on_timeout<Pub: event::ConnectionPublisher>(
        &mut self,
        now: Timestamp,
        path_id: path::Id,
        publisher: &mut Pub,
    ) {
        if let State::Failed(ref mut retest_timer) = &mut self.state {
            if retest_timer.poll_expiration(now).is_ready() {
                self.restart(path_id, publisher);
            }
        }
    }

    /// Gets the ECN marking to use on packets sent to the peer
    pub fn ecn(&self, transmission_mode: transmission::Mode) -> ExplicitCongestionNotification {
        if transmission_mode.is_loss_recovery_probing() {
            // Don't mark loss recovery probes as ECN capable in case the ECN
            // marking is causing packet loss
            return ExplicitCongestionNotification::NotEct;
        }

        match self.state {
            //= https://www.rfc-editor.org/rfc/rfc9000.txt#A.4
            //# On paths with a "testing" or "capable" state, the endpoint
            //# sends packets with an ECT marking -- ECT(0) by default;
            //# otherwise, the endpoint sends unmarked packets.

            //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.2
            //# Upon successful validation, an endpoint MAY continue to set an ECT
            //# codepoint in subsequent packets it sends, with the expectation that
            //# the path is ECN-capable.
            State::Testing(_) | State::Capable => ExplicitCongestionNotification::Ect0,
            //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.2
            //# If validation fails, then the endpoint MUST disable ECN. It stops setting the ECT
            //# codepoint in IP packets that it sends, assuming that either the network path or
            //# the peer does not support ECN.
            State::Failed(_) | State::Unknown => ExplicitCongestionNotification::NotEct,
        }
    }

    /// Returns true if the path has been determined to be capable of handling ECN marked packets
    pub fn is_capable(&self) -> bool {
        matches!(self.state, State::Capable)
    }

    //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.2
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
        path_id: path::Id,
        publisher: &mut Pub,
    ) -> ValidationOutcome {
        if matches!(self.state, State::Failed(_)) {
            // Validation had already failed
            return ValidationOutcome::Skipped;
        }

        if ack_frame_ecn_counts.is_none() {
            if newly_acked_ecn_counts.as_option().is_some() {
                //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.1
                //# If an ACK frame newly acknowledges a packet that the endpoint sent with
                //# either the ECT(0) or ECT(1) codepoint set, ECN validation fails if the
                //# corresponding ECN counts are not present in the ACK frame. This check
                //# detects a network element that zeroes the ECN field or a peer that does
                //# not report ECN markings.
                self.fail(now, path_id, publisher);
                return ValidationOutcome::Failed;
            }

            if baseline_ecn_counts == EcnCounts::default() {
                // Nothing to validate
                return ValidationOutcome::Skipped;
            }
        }

        let congestion_experienced;

        if let Some(incremental_ecn_counts) = ack_frame_ecn_counts
            .unwrap_or_default()
            .checked_sub(baseline_ecn_counts)
        {
            let ect_0_increase = incremental_ecn_counts
                .ect_0_count
                .saturating_add(incremental_ecn_counts.ce_count);
            if ect_0_increase < newly_acked_ecn_counts.ect_0_count {
                //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.1
                //# ECN validation also fails if the sum of the increase in ECT(0)
                //# and ECN-CE counts is less than the number of newly acknowledged
                //# packets that were originally sent with an ECT(0) marking.
                self.fail(now, path_id, publisher);
                return ValidationOutcome::Failed;
            }

            if incremental_ecn_counts.ect_0_count > sent_packet_ecn_counts.ect_0_count
                || incremental_ecn_counts.ect_1_count > sent_packet_ecn_counts.ect_1_count
            {
                //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.1
                //# ECN validation can fail if the received total count for either ECT(0) or ECT(1)
                //# exceeds the total number of packets sent with each corresponding ECT codepoint.
                self.fail(now, path_id, publisher);
                return ValidationOutcome::Failed;
            }

            congestion_experienced = incremental_ecn_counts.ce_count > VarInt::from_u8(0);
        } else {
            // ECN counts decreased from the baseline
            self.fail(now, path_id, publisher);
            return ValidationOutcome::Failed;
        }

        //= https://www.rfc-editor.org/rfc/rfc9000.txt#A.4
        //# From the "unknown" state, successful validation of the ECN counts in an ACK frame
        //# (see Section 13.4.2.1) causes the ECN state for the path to become "capable",
        //# unless no marked packet has been acknowledged.
        if matches!(self.state, State::Unknown)
            && newly_acked_ecn_counts.ect_0_count > VarInt::from_u8(0)
        {
            self.change_state(State::Capable, path_id, publisher);
        }

        if self.is_capable() && congestion_experienced {
            return ValidationOutcome::CongestionExperienced;
        }

        ValidationOutcome::Passed
    }

    /// This method gets called when a packet has been sent
    pub fn on_packet_sent<Pub: event::ConnectionPublisher>(
        &mut self,
        ecn: ExplicitCongestionNotification,
        path_id: path::Id,
        publisher: &mut Pub,
    ) {
        debug_assert!(
            !matches!(ecn, ExplicitCongestionNotification::Ect1),
            "Ect1 is not used"
        );
        debug_assert!(
            !matches!(ecn, ExplicitCongestionNotification::Ce),
            "Endpoints should not mark packets as Ce"
        );

        if let (true, State::Testing(ref mut packet_count)) = (ecn.using_ecn(), &mut self.state) {
            *packet_count += 1;

            if *packet_count >= TESTING_PACKET_THRESHOLD {
                self.change_state(State::Unknown, path_id, publisher);
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
        path_id: path::Id,
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
            self.fail(now, path_id, publisher);
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
        path_id: path::Id,
        publisher: &mut Pub,
    ) {
        //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.2
        //# Even if validation fails, an endpoint MAY revalidate ECN for the same path at any later
        //# time in the connection. An endpoint could continue to periodically attempt validation.
        let mut retest_timer = Timer::default();
        retest_timer.set(now + RETEST_COOL_OFF_DURATION);
        self.change_state(State::Failed(retest_timer), path_id, publisher);
        self.black_hole_counter = Default::default();
    }

    fn change_state<Pub: event::ConnectionPublisher>(
        &mut self,
        state: State,
        path_id: path::Id,
        publisher: &mut Pub,
    ) {
        debug_assert_ne!(self.state, state);

        self.state = state;

        publisher.on_ecn_state_changed(event::builder::EcnStateChanged {
            path_id: path_id.into_event(),
            state: self.state.into_event(),
            capable: self.is_capable(),
        })
    }
}

impl timer::Provider for Controller {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        if let State::Failed(timer) = &self.state {
            timer.timers(query)?
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests;
