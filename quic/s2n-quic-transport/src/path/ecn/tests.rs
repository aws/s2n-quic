// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use super::*;
use s2n_quic_core::{event::testing::Publisher, time::timer::Provider, varint::VarInt};
use std::ops::Deref;

/// Return ECN counts with the given counts
fn helper_ecn_counts(ect0: u8, ect1: u8, ce: u8) -> EcnCounts {
    EcnCounts {
        ect_0_count: VarInt::from_u8(ect0),
        ect_1_count: VarInt::from_u8(ect1),
        ce_count: VarInt::from_u8(ce),
    }
}

#[test]
fn default() {
    let controller = Controller::default();
    assert_eq!(0, *controller.black_hole_counter.deref());
    assert_eq!(State::Testing(0), controller.state);
    assert_eq!(None, controller.last_acked_ecn_packet_timestamp);
}

#[test]
fn restart() {
    let mut controller = Controller {
        state: State::Failed(Timer::default()),
        ..Default::default()
    };
    controller.black_hole_counter += 1;

    controller.restart(path::Id::test_id(), &mut Publisher::default());

    assert_eq!(State::Testing(0), controller.state);
    assert_eq!(0, *controller.black_hole_counter.deref());
}

#[test]
fn restart_already_in_testing_0() {
    let mut controller = Controller {
        state: State::Testing(0),
        ..Default::default()
    };
    controller.black_hole_counter += 1;

    controller.restart(path::Id::test_id(), &mut Publisher::default());

    assert_eq!(State::Testing(0), controller.state);
    assert_eq!(0, *controller.black_hole_counter.deref());
}

#[test]
fn on_timeout() {
    let mut controller = Controller::default();
    let now = s2n_quic_platform::time::now();
    controller.fail(now, path::Id::test_id(), &mut Publisher::default());

    if let State::Failed(timer) = &controller.state {
        assert!(timer.is_armed());
    } else {
        panic!("State should be Failed");
    }

    assert_eq!(0, *controller.black_hole_counter.deref());

    let now = now + RETEST_COOL_OFF_DURATION - Duration::from_secs(1);

    // Too soon
    controller.on_timeout(now, path::Id::test_id(), &mut Publisher::default());

    if let State::Failed(timer) = &controller.state {
        assert!(timer.is_armed());
    } else {
        panic!("State should be Failed");
    }

    assert_eq!(0, *controller.black_hole_counter.deref());

    let now = now + Duration::from_secs(1);
    controller.on_timeout(now, path::Id::test_id(), &mut Publisher::default());

    assert_eq!(State::Testing(0), controller.state);
    assert_eq!(0, *controller.black_hole_counter.deref());
}

#[test]
fn ecn() {
    for &transmission_mode in &[
        transmission::Mode::Normal,
        transmission::Mode::MtuProbing,
        transmission::Mode::PathValidationOnly,
    ] {
        let mut controller = Controller::default();
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
        controller.fail(
            s2n_quic_platform::time::now(),
            path::Id::test_id(),
            &mut Publisher::default(),
        );
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
        State::Failed(Timer::default()),
    ] {
        let controller = Controller {
            state,
            ..Default::default()
        };
        assert!(!controller
            .ecn(transmission::Mode::LossRecoveryProbing)
            .using_ecn());
    }
}

#[test]
fn is_capable() {
    for state in vec![
        State::Testing(0),
        State::Unknown,
        State::Failed(Timer::default()),
    ] {
        let controller = Controller {
            state,
            ..Default::default()
        };
        assert!(!controller.is_capable());
    }

    let controller = Controller {
        state: State::Capable,
        ..Default::default()
    };
    assert!(controller.is_capable());
}

#[test]
fn validate_already_failed() {
    let mut controller = Controller::default();
    let now = s2n_quic_platform::time::now();
    controller.fail(now, path::Id::test_id(), &mut Publisher::default());
    let outcome = controller.validate(
        EcnCounts::default(),
        EcnCounts::default(),
        EcnCounts::default(),
        None,
        now + Duration::from_secs(5),
        path::Id::test_id(),
        &mut Publisher::default(),
    );

    if let State::Failed(timer) = &controller.state {
        assert!(timer.is_armed());
        assert_eq!(
            controller.next_expiration(),
            Some(now + RETEST_COOL_OFF_DURATION)
        );
    } else {
        panic!("State should be Failed");
    }
    assert_eq!(ValidationOutcome::Skipped, outcome);
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
    let mut controller = Controller::default();
    let now = s2n_quic_platform::time::now();
    let expected_ecn_counts = helper_ecn_counts(1, 0, 0);
    let outcome = controller.validate(
        expected_ecn_counts,
        EcnCounts::default(),
        EcnCounts::default(),
        None,
        now,
        path::Id::test_id(),
        &mut Publisher::default(),
    );

    assert_eq!(ValidationOutcome::Failed, outcome);
    assert!(matches!(controller.state, State::Failed(_)));
}

//= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.1
//= type=test
//# ECN validation also fails if the sum of the increase in ECT(0)
//# and ECN-CE counts is less than the number of newly acknowledged
//# packets that were originally sent with an ECT(0) marking.
#[test]
fn validate_ecn_ce_remarking() {
    let mut controller = Controller::default();
    let now = s2n_quic_platform::time::now();
    let expected_ecn_counts = helper_ecn_counts(1, 0, 0);
    let sent_packet_ecn_counts = helper_ecn_counts(1, 0, 0);
    let outcome = controller.validate(
        expected_ecn_counts,
        sent_packet_ecn_counts,
        EcnCounts::default(),
        Some(EcnCounts::default()),
        now,
        path::Id::test_id(),
        &mut Publisher::default(),
    );

    assert_eq!(ValidationOutcome::Failed, outcome);
    assert!(matches!(controller.state, State::Failed(_)));
}

//= https://www.rfc-editor.org/rfc/rfc9000.txt#13.4.2.1
//= type=test
//# ECN validation can fail if the received total count for either ECT(0) or ECT(1)
//# exceeds the total number of packets sent with each corresponding ECT codepoint.
#[test]
fn validate_ect_0_remarking() {
    let mut controller = Controller::default();
    let now = s2n_quic_platform::time::now();
    let expected_ecn_counts = helper_ecn_counts(1, 0, 0);
    let sent_packet_ecn_counts = helper_ecn_counts(1, 0, 0);
    let ack_frame_ecn_counts = helper_ecn_counts(1, 1, 0);
    let outcome = controller.validate(
        expected_ecn_counts,
        sent_packet_ecn_counts,
        EcnCounts::default(),
        Some(ack_frame_ecn_counts),
        now,
        path::Id::test_id(),
        &mut Publisher::default(),
    );

    assert_eq!(ValidationOutcome::Failed, outcome);
    assert!(matches!(controller.state, State::Failed(_)));
}

#[test]
fn validate_ect_0_remarking_after_restart() {
    let mut controller = Controller::default();
    let now = s2n_quic_platform::time::now();
    let expected_ecn_counts = helper_ecn_counts(1, 0, 0);
    let ack_frame_ecn_counts = helper_ecn_counts(0, 3, 0);
    let baseline_ecn_counts = helper_ecn_counts(0, 2, 0);
    let sent_packet_ecn_counts = helper_ecn_counts(1, 0, 0);
    let outcome = controller.validate(
        expected_ecn_counts,
        sent_packet_ecn_counts,
        baseline_ecn_counts,
        Some(ack_frame_ecn_counts),
        now,
        path::Id::test_id(),
        &mut Publisher::default(),
    );

    assert_eq!(ValidationOutcome::Failed, outcome);
    assert!(matches!(controller.state, State::Failed(_)));
}

#[test]
fn validate_no_ecn_counts() {
    let mut controller = Controller {
        state: State::Unknown,
        ..Default::default()
    };
    let now = s2n_quic_platform::time::now();
    let outcome = controller.validate(
        EcnCounts::default(),
        EcnCounts::default(),
        EcnCounts::default(),
        None,
        now,
        path::Id::test_id(),
        &mut Publisher::default(),
    );

    assert_eq!(ValidationOutcome::Skipped, outcome);
    assert_eq!(State::Unknown, controller.state);
}

#[test]
fn validate_ecn_decrease() {
    let mut controller = Controller::default();
    let now = s2n_quic_platform::time::now();
    let baseline_ecn_counts = helper_ecn_counts(1, 0, 0);
    let outcome = controller.validate(
        EcnCounts::default(),
        EcnCounts::default(),
        baseline_ecn_counts,
        None,
        now,
        path::Id::test_id(),
        &mut Publisher::default(),
    );

    assert_eq!(ValidationOutcome::Failed, outcome);
    assert!(matches!(controller.state, State::Failed(_)));
}

//= https://www.rfc-editor.org/rfc/rfc9000.txt#A.4
//= type=test
//# From the "unknown" state, successful validation of the ECN counts in an ACK frame
//# (see Section 13.4.2.1) causes the ECN state for the path to become "capable",
//# unless no marked packet has been acknowledged.
#[test]
fn validate_no_marked_packets_acked() {
    let mut controller = Controller {
        state: State::Unknown,
        ..Default::default()
    };
    let now = s2n_quic_platform::time::now();
    let outcome = controller.validate(
        EcnCounts::default(),
        EcnCounts::default(),
        EcnCounts::default(),
        Some(EcnCounts::default()),
        now,
        path::Id::test_id(),
        &mut Publisher::default(),
    );

    assert_eq!(ValidationOutcome::Passed, outcome);
    assert_eq!(State::Unknown, controller.state);
}

#[test]
fn validate_capable() {
    let mut controller = Controller {
        state: State::Unknown,
        ..Default::default()
    };
    let now = s2n_quic_platform::time::now();
    let expected_ecn_counts = helper_ecn_counts(2, 0, 0);
    let ack_frame_ecn_counts = helper_ecn_counts(2, 0, 0);
    let sent_packet_ecn_counts = helper_ecn_counts(2, 0, 0);
    let outcome = controller.validate(
        expected_ecn_counts,
        sent_packet_ecn_counts,
        EcnCounts::default(),
        Some(ack_frame_ecn_counts),
        now,
        path::Id::test_id(),
        &mut Publisher::default(),
    );

    assert_eq!(ValidationOutcome::Passed, outcome);
    assert_eq!(State::Capable, controller.state);
}

#[test]
fn validate_capable_congestion_experienced() {
    let mut controller = Controller {
        state: State::Unknown,
        ..Default::default()
    };
    let now = s2n_quic_platform::time::now();
    let expected_ecn_counts = helper_ecn_counts(2, 0, 0);
    let ack_frame_ecn_counts = helper_ecn_counts(1, 0, 1);
    let sent_packet_ecn_counts = helper_ecn_counts(2, 0, 0);
    let outcome = controller.validate(
        expected_ecn_counts,
        sent_packet_ecn_counts,
        EcnCounts::default(),
        Some(ack_frame_ecn_counts),
        now,
        path::Id::test_id(),
        &mut Publisher::default(),
    );

    assert_eq!(ValidationOutcome::CongestionExperienced, outcome);
    assert_eq!(State::Capable, controller.state);
}

/// Successful validation when not in the Unknown state does not change the state
#[test]
fn validate_capable_not_in_unknown_state() {
    for state in vec![
        State::Testing(0),
        State::Capable,
        State::Failed(Timer::default()),
    ] {
        let mut controller = Controller {
            state,
            ..Default::default()
        };
        let now = s2n_quic_platform::time::now();
        let expected_ecn_counts = helper_ecn_counts(1, 0, 0);
        let ack_frame_ecn_counts = helper_ecn_counts(1, 0, 0);
        let sent_packet_ecn_counts = helper_ecn_counts(1, 0, 0);

        let expected_state = controller.state.clone();
        controller.validate(
            expected_ecn_counts,
            sent_packet_ecn_counts,
            EcnCounts::default(),
            Some(ack_frame_ecn_counts),
            now,
            path::Id::test_id(),
            &mut Publisher::default(),
        );

        assert_eq!(expected_state, controller.state);
    }
}

#[test]
fn validate_capable_lost_ack_frame() {
    let mut controller = Controller {
        state: State::Unknown,
        ..Default::default()
    };
    let now = s2n_quic_platform::time::now();

    // We sent three ECT0 packets
    let sent_packet_ecn_counts = helper_ecn_counts(3, 0, 0);

    // The peer is acknowledging 2 of them, the third was acknowledge in an ack frame
    // that was lost
    let ack_frame_ecn_counts = helper_ecn_counts(3, 0, 0);

    let expected_ecn_counts = helper_ecn_counts(2, 0, 0);

    let outcome = controller.validate(
        expected_ecn_counts,
        sent_packet_ecn_counts,
        EcnCounts::default(),
        Some(ack_frame_ecn_counts),
        now,
        path::Id::test_id(),
        &mut Publisher::default(),
    );

    assert_eq!(ValidationOutcome::Passed, outcome);
    assert_eq!(State::Capable, controller.state);
}

#[test]
fn validate_capable_after_restart() {
    let mut controller = Controller {
        state: State::Unknown,
        ..Default::default()
    };
    let now = s2n_quic_platform::time::now();
    let sent_packet_ecn_counts = helper_ecn_counts(2, 0, 0);
    let expected_ecn_counts = helper_ecn_counts(2, 0, 0);
    // The Ect1 markings would normally fail validation, but since they are included
    // in the baseline ecn counts below, that means we've already accounted for them.
    let ack_frame_ecn_counts = helper_ecn_counts(1, 2, 1);
    let baseline_ecn_counts = helper_ecn_counts(0, 2, 0);
    let outcome = controller.validate(
        expected_ecn_counts,
        sent_packet_ecn_counts,
        baseline_ecn_counts,
        Some(ack_frame_ecn_counts),
        now,
        path::Id::test_id(),
        &mut Publisher::default(),
    );

    assert_eq!(ValidationOutcome::CongestionExperienced, outcome);
    assert_eq!(State::Capable, controller.state);
}

#[test]
fn on_packet_sent() {
    let mut controller = Controller::default();

    for i in 0..TESTING_PACKET_THRESHOLD {
        assert_eq!(State::Testing(i), controller.state);
        controller.on_packet_sent(
            ExplicitCongestionNotification::Ect0,
            path::Id::test_id(),
            &mut Publisher::default(),
        );
    }

    assert_eq!(State::Unknown, controller.state);
}

#[test]
fn on_packet_loss() {
    for state in vec![State::Testing(0), State::Capable, State::Unknown] {
        let mut controller = Controller {
            state,
            ..Default::default()
        };
        let now = s2n_quic_platform::time::now();
        let time_sent = now + Duration::from_secs(1);

        controller.last_acked_ecn_packet_timestamp = Some(now);

        for i in 0..TESTING_PACKET_THRESHOLD + 1 {
            assert_eq!(i, *controller.black_hole_counter.deref());
            assert!(!matches!(controller.state, State::Failed(_)));
            controller.on_packet_loss(
                time_sent,
                ExplicitCongestionNotification::Ect0,
                time_sent,
                path::Id::test_id(),
                &mut Publisher::default(),
            );
        }

        if let State::Failed(timer) = &controller.state {
            assert!(timer.is_armed());
            assert_eq!(
                Some(time_sent + RETEST_COOL_OFF_DURATION),
                controller.next_expiration()
            );
        } else {
            panic!("State should be Failed");
        }

        assert_eq!(0, *controller.black_hole_counter.deref());
    }
}

#[test]
fn on_packet_loss_already_failed() {
    let mut controller = Controller::default();
    let now = s2n_quic_platform::time::now();
    let time_sent = now + Duration::from_secs(1);

    controller.last_acked_ecn_packet_timestamp = Some(now);
    controller.fail(now, path::Id::test_id(), &mut Publisher::default());

    for _i in 0..TESTING_PACKET_THRESHOLD + 1 {
        assert_eq!(0, *controller.black_hole_counter.deref());
        assert!(matches!(controller.state, State::Failed(_)));
        controller.on_packet_loss(
            time_sent,
            ExplicitCongestionNotification::Ect0,
            time_sent,
            path::Id::test_id(),
            &mut Publisher::default(),
        );
    }

    if let State::Failed(timer) = &controller.state {
        assert!(timer.is_armed());
        assert_eq!(
            Some(now + RETEST_COOL_OFF_DURATION),
            controller.next_expiration()
        );
    } else {
        panic!("State should be Failed");
    }

    assert_eq!(0, *controller.black_hole_counter.deref());
}

#[test]
fn fuzz_validate() {
    let now = s2n_quic_platform::time::now();

    bolero::check!()
        .with_type::<(EcnCounts, EcnCounts, EcnCounts, Option<EcnCounts>)>()
        .cloned()
        .for_each(
            |(
                newly_acked_ecn_counts,
                sent_packet_ecn_counts,
                baseline_ecn_counts,
                ack_frame_ecn_counts,
            )| {
                let mut controller = Controller::default();
                let outcome = controller.validate(
                    newly_acked_ecn_counts,
                    sent_packet_ecn_counts,
                    baseline_ecn_counts,
                    ack_frame_ecn_counts,
                    now,
                    path::Id::test_id(),
                    &mut Publisher::default(),
                );

                if outcome == ValidationOutcome::Failed {
                    assert!(!controller.is_capable());
                    assert_eq!(
                        ExplicitCongestionNotification::NotEct,
                        controller.ecn(transmission::Mode::Normal)
                    );
                    assert!(matches!(controller.state, State::Failed(_)));
                }
            },
        );
}
