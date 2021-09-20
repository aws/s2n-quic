// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::{
    counter::{Counter, Saturating},
    frame::ack::EcnCounts,
    inet::ExplicitCongestionNotification,
    time::{timer, Duration, Timer, Timestamp},
    transmission,
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
enum State {
    // ECN capability testing has been restarted, but baseline ECN counts are needed
    PendingBaseline,
    // ECN capability is being tested, tracking the number of ECN marked packets sent
    Testing(u8),
    // ECN capability has been tested, but not validated yet
    Unknown,
    // ECN validation has failed
    Failed,
    // ECN validation has succeeded
    Capable,
}

#[derive(Clone, Debug)]
pub struct Controller {
    state: State,
    // A count of the number of packets with ECN marking lost since
    // the last time a packet with ECN marking was acknowledged.
    black_hole_counter: Counter<u8, Saturating>,
    // The largest acknowledged packet sent with an ECN marking. Used when tracking
    // packets that have been lost for the purpose of detecting a black hole.
    last_acked_ecn_packet_timestamp: Option<Timestamp>,
    // ECN Counts at the beginning of the testing period or after successful validation
    baseline_ecn_counts: EcnCounts,
    // Timer for re-testing the path for ECN capability after failure
    retest_timer: Timer,
}

impl Default for Controller {
    fn default() -> Self {
        Controller::new()
    }
}

impl Controller {
    /// Construct a new ecn::Controller in the `Testing` state.
    pub fn new() -> Self {
        Self {
            state: State::Testing(0),
            black_hole_counter: Default::default(),
            last_acked_ecn_packet_timestamp: None,
            baseline_ecn_counts: EcnCounts::default(),
            retest_timer: Default::default(),
        }
    }

    /// Restart testing of ECN capability
    pub fn restart(&mut self) {
        self.state = State::PendingBaseline;
        self.black_hole_counter = Default::default();
        self.retest_timer.cancel();
    }

    /// Called when the connection timer expires
    pub fn on_timeout(&mut self, now: Timestamp) {
        if self.retest_timer.poll_expiration(now).is_ready() {
            self.restart();
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
            _ => ExplicitCongestionNotification::NotEct,
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
    pub fn validate(
        &mut self,
        expected_ecn_counts: EcnCounts,
        latest_ecn_counts: EcnCounts,
        ack_frame_ecn_counts: Option<EcnCounts>,
        now: Timestamp,
    ) {
        if matches!(self.state, State::Failed) {
            // Validation had already failed
            return;
        }

        if expected_ecn_counts.as_option().is_some() && ack_frame_ecn_counts.is_none() {
            //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.1
            //# If an ACK frame newly acknowledges a packet that the endpoint sent with
            //# either the ECT(0) or ECT(1) codepoint set, ECN validation fails if the
            //# corresponding ECN counts are not present in the ACK frame. This check
            //# detects a network element that zeroes the ECN field or a peer that does
            //# not report ECN markings.
            self.fail(now);
            return;
        }

        if let Some(ack_frame_ecn_counts) = ack_frame_ecn_counts {
            //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.1
            //# ECN validation also fails if the sum of the increase in ECT(0)
            //# and ECN-CE counts is less than the number of newly acknowledged
            //# packets that were originally sent with an ECT(0) marking.
            let ect_0_increase = (ack_frame_ecn_counts.ect_0_count + ack_frame_ecn_counts.ce_count)
                .saturating_sub(latest_ecn_counts.ect_0_count + latest_ecn_counts.ce_count);
            if ect_0_increase < expected_ecn_counts.ect_0_count {
                self.fail(now);
                return;
            }

            if ack_frame_ecn_counts.ect_0_count
                > expected_ecn_counts
                    .ect_0_count
                    .saturating_add(self.baseline_ecn_counts.ect_0_count)
                || ack_frame_ecn_counts.ect_1_count
                    > expected_ecn_counts
                        .ect_1_count
                        .saturating_add(self.baseline_ecn_counts.ect_1_count)
            {
                //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.1
                //# ECN validation can fail if the received total count for either ECT(0) or ECT(1)
                //# exceeds the total number of packets sent with each corresponding ECT codepoint.
                self.fail(now);
                return;
            }

            // The ECN counts are valid so they become the new baseline
            self.baseline_ecn_counts = ack_frame_ecn_counts;
        } else {
            // No ECN counts to validate
            return;
        }

        if matches!(self.state, State::Unknown) {
            self.state = State::Capable;
        }
    }

    /// This method gets called when a packet has been sent
    pub fn on_packet_sent(&mut self, ecn: ExplicitCongestionNotification) {
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
                self.state = State::Unknown
            }
        }
    }

    /// This method gets called when a packet delivery got acknowledged
    pub fn on_packet_ack(
        &mut self,
        time_sent: Timestamp,
        ecn: ExplicitCongestionNotification,
        ack_frame_ecn_counts: Option<EcnCounts>,
    ) {
        if matches!(self.state, State::PendingBaseline) {
            self.baseline_ecn_counts = ack_frame_ecn_counts.unwrap_or_default();
            self.state = State::Testing(0);
        }

        if ecn.using_ecn()
            && self
                .last_acked_ecn_packet_timestamp
                .map_or(true, |last_acked| last_acked < time_sent)
        {
            // Reset the black hole counter since a packet with ECN marking
            // has been acknowledged, indicating the path may still be ECN-capable
            self.black_hole_counter = Default::default();
            self.last_acked_ecn_packet_timestamp = Some(time_sent);
        }
    }

    /// This method gets called when a packet loss is reported
    pub fn on_packet_loss(
        &mut self,
        time_sent: Timestamp,
        ecn: ExplicitCongestionNotification,
        now: Timestamp,
    ) {
        if matches!(self.state, State::Failed) {
            return;
        }

        if ecn.using_ecn()
            && self
                .last_acked_ecn_packet_timestamp
                .map_or(true, |last_acked| last_acked < time_sent)
        {
            // An ECN marked packet that was sent after the last
            // acknowledged ECN marked packet has been lost
            self.black_hole_counter += 1;
        }

        if self.black_hole_counter > TESTING_PACKET_THRESHOLD {
            self.fail(now);
        }
    }

    /// Set the state to Failed and arm the retest timer
    fn fail(&mut self, now: Timestamp) {
        self.state = State::Failed;
        self.black_hole_counter = Default::default();
        //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.2
        //# Even if validation fails, an endpoint MAY revalidate ECN for the same path at any later
        //# time in the connection. An endpoint could continue to periodically attempt validation.
        self.retest_timer.set(now + RETEST_COOL_OFF_DURATION);
    }
}

impl timer::Provider for Controller {
    #[inline]
    fn timers<Q: timer::Query>(&self, query: &mut Q) -> timer::Result {
        self.retest_timer.timers(query)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests;
