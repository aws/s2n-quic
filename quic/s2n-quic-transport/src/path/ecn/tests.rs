// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use super::*;
use s2n_quic_core::time::timer::Provider;
use std::ops::Deref;

#[test]
fn new() {
    let controller = Controller::new();
    assert_eq!(0, *controller.black_hole_counter.deref());
    assert!(!controller.retest_timer.is_armed());
    assert_eq!(State::Testing(0), controller.state);
    assert_eq!(None, controller.last_acked_ecn_packet_timestamp);
}

#[test]
fn restart() {
    let mut controller = Controller::new();
    let now = s2n_quic_platform::time::now();
    controller.state = State::Failed;
    controller.retest_timer.set(now);
    controller.black_hole_counter += 1;

    controller.restart();

    assert_eq!(State::PendingBaseline, controller.state);
    assert!(!controller.retest_timer.is_armed());
    assert_eq!(0, *controller.black_hole_counter.deref());
}

#[test]
fn on_timeout() {
    let mut controller = Controller::new();
    let now = s2n_quic_platform::time::now();
    controller.fail(now);

    assert_eq!(State::Failed, controller.state);
    assert_eq!(0, *controller.black_hole_counter.deref());
    assert!(controller.retest_timer.is_armed());

    let now = now + RETEST_COOL_OFF_DURATION - Duration::from_secs(1);

    // Too soon
    controller.on_timeout(now);

    assert_eq!(State::Failed, controller.state);
    assert_eq!(0, *controller.black_hole_counter.deref());
    assert!(controller.retest_timer.is_armed());

    let now = now + Duration::from_secs(1);
    controller.on_timeout(now);

    assert_eq!(State::PendingBaseline, controller.state);
    assert!(!controller.retest_timer.is_armed());
    assert_eq!(0, *controller.black_hole_counter.deref());
}

#[test]
fn ecn() {
    for &transmission_mode in &[
        transmission::Mode::Normal,
        transmission::Mode::MtuProbing,
        transmission::Mode::PathValidationOnly,
    ] {
        let mut controller = Controller::new();
        assert!(controller.ecn(transmission_mode).using_ecn());

        //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.2
        //= type=test
        //# Upon successful validation, an endpoint MAY continue to set an ECT
        //# codepoint in subsequent packets it sends, with the expectation that
        //# the path is ECN-capable.
        controller.state = State::Capable;
        assert!(controller.ecn(transmission_mode).using_ecn());

        //= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.2
        //= type=test
        //# If validation fails, then the endpoint MUST disable ECN. It stops setting the ECT
        //# codepoint in IP packets that it sends, assuming that either the network path or
        //# the peer does not support ECN.
        controller.fail(s2n_quic_platform::time::now());
        assert!(!controller.ecn(transmission_mode).using_ecn());

        controller.state = State::Unknown;
        assert!(!controller.ecn(transmission_mode).using_ecn());
    }
}

#[test]
fn ecn_loss_recovery_probing() {
    for state in vec![
        State::Capable,
        State::Testing(0),
        State::Unknown,
        State::Failed,
    ] {
        let mut controller = Controller::new();
        controller.state = state;
        assert!(!controller
            .ecn(transmission::Mode::LossRecoveryProbing)
            .using_ecn());
    }
}

#[test]
fn is_capable() {
    for state in vec![State::Testing(0), State::Unknown, State::Failed] {
        let mut controller = Controller::new();
        controller.state = state;
        assert!(!controller.is_capable());
    }

    let mut controller = Controller::new();
    controller.state = State::Capable;
    assert!(controller.is_capable());
}

#[test]
fn validate_already_failed() {
    let mut controller = Controller::new();
    let now = s2n_quic_platform::time::now();
    controller.fail(now);
    controller.validate(
        EcnCounts::default(),
        EcnCounts::default(),
        None,
        now + Duration::from_secs(5),
    );

    assert_eq!(State::Failed, controller.state);
    assert_eq!(
        controller.next_expiration(),
        Some(now + RETEST_COOL_OFF_DURATION)
    );
    assert_eq!(EcnCounts::default(), controller.baseline_ecn_counts);
}

//= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.1
//= type=test
//# If an ACK frame newly acknowledges a packet that the endpoint sent with
//# either the ECT(0) or ECT(1) codepoint set, ECN validation fails if the
//# corresponding ECN counts are not present in the ACK frame. This check
//# detects a network element that zeroes the ECN field or a peer that does
//# not report ECN markings.
#[test]
fn validate_ecn_counts_not_in_ack() {
    let mut controller = Controller::new();
    let now = s2n_quic_platform::time::now();
    let mut expected_ecn_counts = EcnCounts::default();
    expected_ecn_counts.increment(ExplicitCongestionNotification::Ect0);
    controller.validate(expected_ecn_counts, EcnCounts::default(), None, now);

    assert_eq!(State::Failed, controller.state);
    assert_eq!(EcnCounts::default(), controller.baseline_ecn_counts);
}

//= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.1
//= type=test
//# ECN validation also fails if the sum of the increase in ECT(0)
//# and ECN-CE counts is less than the number of newly acknowledged
//# packets that were originally sent with an ECT(0) marking.
#[test]
fn validate_ecn_ce_remarking() {
    let mut controller = Controller::new();
    let now = s2n_quic_platform::time::now();
    let mut expected_ecn_counts = EcnCounts::default();
    expected_ecn_counts.increment(ExplicitCongestionNotification::Ect0);
    controller.validate(
        expected_ecn_counts,
        EcnCounts::default(),
        Some(EcnCounts::default()),
        now,
    );

    assert_eq!(State::Failed, controller.state);
    assert_eq!(EcnCounts::default(), controller.baseline_ecn_counts);
}

//= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.1
//= type=test
//# ECN validation can fail if the received total count for either ECT(0) or ECT(1)
//# exceeds the total number of packets sent with each corresponding ECT codepoint.
#[test]
fn validate_ect_0_remarking() {
    let mut controller = Controller::new();
    let now = s2n_quic_platform::time::now();
    let mut expected_ecn_counts = EcnCounts::default();
    expected_ecn_counts.increment(ExplicitCongestionNotification::Ect0);
    let mut ack_frame_ecn_counts = EcnCounts::default();
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ect1);
    controller.validate(
        expected_ecn_counts,
        EcnCounts::default(),
        Some(ack_frame_ecn_counts),
        now,
    );

    assert_eq!(State::Failed, controller.state);
    assert_eq!(EcnCounts::default(), controller.baseline_ecn_counts);
}

#[test]
fn validate_ect_0_remarking_after_restart() {
    let mut controller = Controller::new();
    let now = s2n_quic_platform::time::now();
    let mut expected_ecn_counts = EcnCounts::default();
    expected_ecn_counts.increment(ExplicitCongestionNotification::Ect0);
    let mut ack_frame_ecn_counts = EcnCounts::default();
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ect1);
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ect1);
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ect1);
    let mut baseline_ecn_counts = EcnCounts::default();
    baseline_ecn_counts.increment(ExplicitCongestionNotification::Ect1);
    baseline_ecn_counts.increment(ExplicitCongestionNotification::Ect1);
    controller.baseline_ecn_counts = baseline_ecn_counts;
    controller.validate(
        expected_ecn_counts,
        EcnCounts::default(),
        Some(ack_frame_ecn_counts),
        now,
    );

    assert_eq!(State::Failed, controller.state);
    assert_eq!(baseline_ecn_counts, controller.baseline_ecn_counts);
}

#[test]
fn validate_no_ecn_counts() {
    let mut controller = Controller::new();
    controller.state = State::Unknown;
    let now = s2n_quic_platform::time::now();
    controller.validate(EcnCounts::default(), EcnCounts::default(), None, now);

    assert_eq!(State::Unknown, controller.state);
}

#[test]
fn validate_capable() {
    let mut controller = Controller::new();
    controller.state = State::Unknown;
    let now = s2n_quic_platform::time::now();
    let mut expected_ecn_counts = EcnCounts::default();
    expected_ecn_counts.increment(ExplicitCongestionNotification::Ect0);
    expected_ecn_counts.increment(ExplicitCongestionNotification::Ect0);
    let mut ack_frame_ecn_counts = EcnCounts::default();
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ce);
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ect0);
    controller.validate(
        expected_ecn_counts,
        EcnCounts::default(),
        Some(ack_frame_ecn_counts),
        now,
    );

    assert_eq!(State::Capable, controller.state);
    assert_eq!(ack_frame_ecn_counts, controller.baseline_ecn_counts);

    // Additional ECN counts are still valid since they may be the result
    // of lost Ack frames
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ect0);
    controller.validate(
        expected_ecn_counts,
        EcnCounts::default(),
        Some(ack_frame_ecn_counts),
        now,
    );

    assert_eq!(State::Capable, controller.state);
    assert_eq!(ack_frame_ecn_counts, controller.baseline_ecn_counts);

    // Successful validation when not in the Unknown state does nothing
    for state in vec![State::Testing(0), State::Capable, State::Failed] {
        controller.state = state.clone();
        controller.validate(
            expected_ecn_counts,
            EcnCounts::default(),
            Some(ack_frame_ecn_counts),
            now,
        );
        assert_eq!(state, controller.state);
    }
}

#[test]
fn validate_capable_after_restart() {
    let mut controller = Controller::new();
    controller.state = State::Unknown;
    let now = s2n_quic_platform::time::now();
    let mut expected_ecn_counts = EcnCounts::default();
    expected_ecn_counts.increment(ExplicitCongestionNotification::Ect0);
    expected_ecn_counts.increment(ExplicitCongestionNotification::Ect0);
    let mut ack_frame_ecn_counts = EcnCounts::default();
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ce);
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ect0);
    // These Ect1 markings would normally fail validation, but since they are included
    // in the baseline ecn counts below, that means we've already accounted for them.
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ect1);
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ect1);
    let mut baseline_ecn_counts = EcnCounts::default();
    baseline_ecn_counts.increment(ExplicitCongestionNotification::Ect1);
    baseline_ecn_counts.increment(ExplicitCongestionNotification::Ect1);
    controller.baseline_ecn_counts = baseline_ecn_counts;
    controller.validate(
        expected_ecn_counts,
        EcnCounts::default(),
        Some(ack_frame_ecn_counts),
        now,
    );

    assert_eq!(State::Capable, controller.state);
    assert_eq!(ack_frame_ecn_counts, controller.baseline_ecn_counts);
}

#[test]
fn on_packet_sent() {
    let mut controller = Controller::new();
    controller.state = State::Testing(0);

    for i in 0..TESTING_PACKET_THRESHOLD {
        assert_eq!(State::Testing(i), controller.state);
        controller.on_packet_sent(ExplicitCongestionNotification::Ect0);
    }

    assert_eq!(State::Unknown, controller.state);
}

#[test]
fn on_packet_ack_pending_baseline() {
    let mut controller = Controller::new();
    let now = s2n_quic_platform::time::now();
    let mut ack_frame_ecn_counts = EcnCounts::default();
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ect0);
    ack_frame_ecn_counts.increment(ExplicitCongestionNotification::Ect1);

    controller.state = State::PendingBaseline;

    controller.on_packet_ack(
        now,
        ExplicitCongestionNotification::Ect0,
        Some(ack_frame_ecn_counts),
    );

    assert_eq!(State::Testing(0), controller.state);
    assert_eq!(ack_frame_ecn_counts, controller.baseline_ecn_counts);

    controller.state = State::PendingBaseline;

    controller.on_packet_ack(now, ExplicitCongestionNotification::Ect0, None);

    assert_eq!(State::Testing(0), controller.state);
    assert_eq!(EcnCounts::default(), controller.baseline_ecn_counts);
}

#[test]
fn on_packet_loss() {
    for state in vec![State::Testing(0), State::Capable, State::Unknown] {
        let mut controller = Controller::new();
        controller.state = state;
        let now = s2n_quic_platform::time::now();
        let time_sent = now + Duration::from_secs(1);

        controller.last_acked_ecn_packet_timestamp = Some(now);

        for i in 0..TESTING_PACKET_THRESHOLD + 1 {
            assert_eq!(i, *controller.black_hole_counter.deref());
            assert_ne!(State::Failed, controller.state);
            controller.on_packet_loss(time_sent, ExplicitCongestionNotification::Ect0, time_sent);
        }

        assert_eq!(State::Failed, controller.state);
        assert_eq!(
            Some(time_sent + RETEST_COOL_OFF_DURATION),
            controller.next_expiration()
        );
        assert_eq!(0, *controller.black_hole_counter.deref());
    }
}

#[test]
fn on_packet_loss_already_failed() {
    let mut controller = Controller::new();
    let now = s2n_quic_platform::time::now();
    let time_sent = now + Duration::from_secs(1);

    controller.last_acked_ecn_packet_timestamp = Some(now);
    controller.fail(now);

    for _i in 0..TESTING_PACKET_THRESHOLD + 1 {
        assert_eq!(0, *controller.black_hole_counter.deref());
        assert_eq!(State::Failed, controller.state);
        controller.on_packet_loss(time_sent, ExplicitCongestionNotification::Ect0, time_sent);
    }

    assert_eq!(State::Failed, controller.state);
    assert_eq!(
        Some(now + RETEST_COOL_OFF_DURATION),
        controller.next_expiration()
    );
    assert_eq!(0, *controller.black_hole_counter.deref());
}
