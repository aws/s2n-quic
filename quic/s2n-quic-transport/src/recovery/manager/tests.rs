// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    connection::{ConnectionIdMapper, InternalConnectionIdGenerator},
    endpoint::{
        self,
        testing::{Client as ClientConfig, Server as ServerConfig},
    },
    recovery,
};
use bolero::TypeGenerator;
use core::{ops::RangeInclusive, time::Duration};
use s2n_quic_core::{
    ack, connection,
    event::testing::Publisher,
    frame::ack_elicitation::AckElicitation,
    inet::{DatagramInfo, ExplicitCongestionNotification, SocketAddress},
    packet::number::PacketNumberSpace,
    path::{migration, mtu, RemoteAddress, INITIAL_PTO_BACKOFF, MINIMUM_MAX_DATAGRAM_SIZE},
    random,
    recovery::{
        congestion_controller::testing::mock::{
            CongestionController as MockCongestionController, Endpoint,
        },
        loss::K_PACKET_THRESHOLD,
        RttEstimator, DEFAULT_INITIAL_RTT, K_GRANULARITY,
    },
    time::{clock::testing as time, testing::now, Clock, NoopClock},
    transmission::Outcome,
    varint::VarInt,
};
use std::{collections::HashSet, net::SocketAddr};

// alias the manager and paths over the config so we don't have to annotate it everywhere
type ServerManager = super::Manager<ServerConfig>;
type ClientManager = super::Manager<ClientConfig>;
type Path = super::Path<ServerConfig>;

//= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.2
//= type=test
//# When no previous RTT is available, the initial RTT
//# SHOULD be set to 333 milliseconds.  This results in handshakes
//# starting with a PTO of 1 second, as recommended for TCP's initial
//# RTO; see Section 2 of [RFC6298].
#[test]
fn one_second_pto_when_no_previous_rtt_available() {
    let space = PacketNumberSpace::Handshake;
    let mut manager = ServerManager::new(space);
    let now = time::now();

    let path = Path::new(
        Default::default(),
        connection::PeerId::TEST_ID,
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
    );

    manager
        .pto
        .update(now, path.rtt_estimator.pto_period(path.pto_backoff, space));

    assert!(manager.pto.is_armed());
    assert_eq!(
        manager.pto.next_expiration(),
        Some(now + Duration::from_millis(999))
    );
}

//= https://www.rfc-editor.org/rfc/rfc9002#appendix-A.5
//= type=test
#[test]
fn on_packet_sent() {
    let now = time::now();
    let mut time_sent = now;
    let ecn = ExplicitCongestionNotification::Ect0;
    let space = PacketNumberSpace::ApplicationData;
    let mut publisher = Publisher::snapshot();
    let (_first_addr, first_path_id, _second_addr, _second_path_id, mut manager, mut path_manager) =
        helper_generate_multi_path_manager(space, &mut publisher);
    let mut context = MockContext::new(&mut path_manager);

    // Validate the path so it is not amplification limited and we can verify PTO arming
    //
    // simulate receiving a handshake packet to force path validation
    context.path_mut().on_handshake_packet();

    let mut expected_bytes_in_flight = 0;

    for i in 1..=10 {
        // Reset pto_update_pending so we can confirm it was set correctly
        manager.pto_update_pending = false;

        let sent_packet = space.new_packet_number(VarInt::from_u8(i));
        let ack_elicitation = if i % 2 == 0 {
            AckElicitation::Eliciting
        } else {
            AckElicitation::NonEliciting
        };
        let app_limited = if i % 2 == 0 { Some(true) } else { Some(false) };

        let outcome = transmission::Outcome {
            ack_elicitation,
            is_congestion_controlled: i % 3 == 0,
            bytes_sent: (2 * i) as usize,
            bytes_progressed: 0,
        };

        manager.on_packet_sent(
            sent_packet,
            outcome,
            time_sent,
            ecn,
            transmission::Mode::Normal,
            app_limited,
            &mut context,
            &mut publisher,
        );

        assert!(manager.sent_packets.get(sent_packet).is_some());
        let actual_sent_packet = manager.sent_packets.get(sent_packet).unwrap();
        assert_eq!(
            actual_sent_packet.congestion_controlled,
            outcome.is_congestion_controlled
        );
        assert_eq!(actual_sent_packet.time_sent, time_sent);
        assert_eq!(actual_sent_packet.ecn, ecn);
        assert_eq!(
            app_limited,
            context.path().congestion_controller.app_limited
        );

        if outcome.is_congestion_controlled {
            assert_eq!(actual_sent_packet.sent_bytes as usize, outcome.bytes_sent);
            expected_bytes_in_flight += outcome.bytes_sent;
        } else {
            assert_eq!(actual_sent_packet.sent_bytes, 0);
        }

        if outcome.ack_elicitation.is_ack_eliciting() {
            assert_eq!(Some(time_sent), manager.time_of_last_ack_eliciting_packet);
            //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
            //= type=test
            //# A sender SHOULD restart its PTO timer every time an ack-eliciting
            //# packet is sent
            assert!(manager.pto_update_pending);
        } else {
            // No ack eliciting packets have been sent yet
            assert!(!manager.pto_update_pending);
        }

        time_sent += Duration::from_millis(10);
    }

    assert_eq!(manager.sent_packets.iter().count(), 10);
    assert_eq!(
        expected_bytes_in_flight as u32,
        context
            .path_by_id(first_path_id)
            .congestion_controller
            .bytes_in_flight
    );
}

#[test]
// The pto timer is shared for all paths and should be armed if packet if received on any of
// the paths.
//
// Setup 1:
// - create path manager with two validated and not AmplificationLimited paths
// - reset pto timer
//
// Trigger 1:
// - send packet on path 1
//   - packet: 1
//   - ack_elicitation: Eliciting
//   - is_congestion_controlled: true
//
// Expectation 1:
// - bytes sent, congestion_controlled, time_sent match that of sent
// - pto is armed
//
// Setup 2:
// - reset pto timer
//
// Trigger 2:
// - send packet on path 2
//   - packet: 2
//   - ack_elicitation: Eliciting
//   - is_congestion_controlled: true
//
// Expectation 2:
// - bytes sent, congestion_controlled, time_sent match that of sent
// - pto is armed
fn on_packet_sent_across_multiple_paths() {
    let now = time::now();
    let ecn = ExplicitCongestionNotification::default();
    let mut time_sent = now;
    let mut publisher = Publisher::snapshot();
    // Call on validated so the path is not amplification limited so we can verify PTO arming
    let space = PacketNumberSpace::ApplicationData;
    let packet_bytes = 128;
    // Setup 1:
    let (_first_addr, _first_path_id, _second_addr, second_path_id, mut manager, mut path_manager) =
        helper_generate_multi_path_manager(space, &mut publisher);
    let mut context = MockContext::new(&mut path_manager);
    // simulate receiving a handshake packet to force path validation
    context.path_mut().on_handshake_packet();

    // Reset pto_update_pending so we can confirm it was set correctly
    manager.pto_update_pending = false;

    // Trigger 1:
    let sent_packet = space.new_packet_number(VarInt::from_u8(1));
    let ack_elicitation = AckElicitation::Eliciting;

    let outcome = transmission::Outcome {
        ack_elicitation,
        is_congestion_controlled: true,
        bytes_sent: packet_bytes,
        bytes_progressed: 0,
    };

    manager.on_packet_sent(
        sent_packet,
        outcome,
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // Expectation 1:
    assert!(manager.sent_packets.get(sent_packet).is_some());
    let actual_sent_packet = manager.sent_packets.get(sent_packet).unwrap();
    assert_eq!(
        actual_sent_packet.congestion_controlled,
        outcome.is_congestion_controlled
    );
    assert_eq!(actual_sent_packet.time_sent, time_sent);

    // checking outcome.is_congestion_controlled
    assert_eq!(actual_sent_packet.sent_bytes as usize, outcome.bytes_sent);

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
    //= type=test
    //# A sender SHOULD restart its PTO timer every time an ack-eliciting
    //# packet is sent
    assert!(manager.pto_update_pending);

    // Setup 2:
    // send 2nd packet on path 2nd path
    let sent_packet = space.new_packet_number(VarInt::from_u8(2));
    time_sent += Duration::from_millis(10);
    let outcome = transmission::Outcome {
        ack_elicitation,
        is_congestion_controlled: true,
        bytes_sent: packet_bytes,
        bytes_progressed: 0,
    };

    // Reset pto_update_pending so we can confirm it was set correctly
    manager.pto_update_pending = false;

    // Trigger 2:
    context.set_path_id(second_path_id);
    manager.on_packet_sent(
        sent_packet,
        outcome,
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // Expectation 2:
    assert!(manager.sent_packets.get(sent_packet).is_some());
    let actual_sent_packet = manager.sent_packets.get(sent_packet).unwrap();
    assert_eq!(
        actual_sent_packet.congestion_controlled,
        outcome.is_congestion_controlled
    );
    assert_eq!(actual_sent_packet.time_sent, time_sent);

    // checking outcome.is_congestion_controlled
    assert_eq!(actual_sent_packet.sent_bytes as usize, outcome.bytes_sent);

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
    //= type=test
    //# A sender SHOULD restart its PTO timer every time an ack-eliciting
    //# packet is sent
    assert!(manager.pto_update_pending);
}

//= https://www.rfc-editor.org/rfc/rfc9002#appendix-A.7
//= type=test
#[test]
fn on_ack_frame() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let packet_bytes = 128;
    let ecn = ExplicitCongestionNotification::default();
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();

    // Start the pto backoff at 2 so we can tell if it was reset
    context.path_mut().pto_backoff = 2;

    let time_sent = time::now() + Duration::from_secs(10);

    // Send packets 1 to 10
    for i in 1..=10 {
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(i)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                bytes_progressed: 0,
            },
            time_sent,
            ecn,
            transmission::Mode::Normal,
            None,
            &mut context,
            &mut publisher,
        );
    }

    assert_eq!(manager.sent_packets.iter().count(), 10);

    // Ack packets 1 to 3
    let ack_receive_time = time_sent + Duration::from_millis(500);
    ack_packets(
        1..=3,
        ack_receive_time,
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    assert_eq!(context.path().congestion_controller.lost_bytes, 0);
    assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
    assert_eq!(context.path().pto_backoff, INITIAL_PTO_BACKOFF);
    assert_eq!(manager.sent_packets.iter().count(), 7);
    assert_eq!(
        manager.largest_acked_packet,
        Some(space.new_packet_number(VarInt::from_u8(3)))
    );
    assert_eq!(context.on_packet_ack_count, 1);
    assert_eq!(context.on_new_packet_ack_count, 1);
    assert_eq!(context.validate_packet_ack_count, 1);
    assert_eq!(context.on_packet_loss_count, 0);
    assert_eq!(
        context.path().rtt_estimator.latest_rtt(),
        Duration::from_millis(500)
    );
    assert_eq!(1, context.on_rtt_update_count);

    // Reset the pto backoff to 2 so we can tell if it was reset
    context.path_mut().pto_backoff = 2;

    // Acknowledging already acked packets
    let ack_receive_time = ack_receive_time + Duration::from_secs(1);
    ack_packets(
        1..=3,
        ack_receive_time,
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    //= https://www.rfc-editor.org/rfc/rfc9002#section-5.1
    //= type=test
    //# An RTT sample MUST NOT be generated on receiving an ACK frame that
    //# does not newly acknowledge at least one ack-eliciting packet.

    // Acknowledging already acked packets does not call on_new_packet_ack or change RTT
    assert_eq!(context.path().congestion_controller.lost_bytes, 0);
    assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
    assert_eq!(context.path().pto_backoff, 2);
    assert_eq!(context.on_packet_ack_count, 2);
    assert_eq!(context.on_new_packet_ack_count, 1);
    assert_eq!(context.validate_packet_ack_count, 2);
    assert_eq!(context.on_packet_loss_count, 0);
    assert_eq!(
        context.path().rtt_estimator.latest_rtt(),
        Duration::from_millis(500)
    );
    assert_eq!(1, context.on_rtt_update_count);

    // Ack packets 7 to 9 (4 - 6 will be considered lost)
    let ack_receive_time = ack_receive_time + Duration::from_secs(1);
    ack_packets(
        7..=9,
        ack_receive_time,
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    assert_eq!(
        context.path().congestion_controller.lost_bytes,
        (packet_bytes * 3) as u32
    );
    assert_eq!(context.path().pto_backoff, INITIAL_PTO_BACKOFF);
    assert_eq!(context.on_packet_ack_count, 3);
    assert_eq!(context.on_new_packet_ack_count, 2);
    assert_eq!(context.validate_packet_ack_count, 3);
    assert_eq!(context.on_packet_loss_count, 3);
    assert_eq!(
        context.path().rtt_estimator.latest_rtt(),
        Duration::from_millis(2500)
    );
    assert_eq!(2, context.on_rtt_update_count);

    // Ack packet 10, but with a path that is not peer validated
    let path_id = unsafe { path::Id::new(0) };
    context.path_manager[path_id] = Path::new(
        Default::default(),
        connection::PeerId::TEST_ID,
        connection::LocalId::TEST_ID,
        context.path().rtt_estimator,
        MockCongestionController::default(),
        false,
        mtu::Config::default(),
    );
    context.path_manager.activate_path_for_test(path_id);
    context.path_mut().pto_backoff = 2;
    let ack_receive_time = ack_receive_time + Duration::from_millis(500);
    ack_packets(
        10..=10,
        ack_receive_time,
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );
    assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
    assert_eq!(context.path().pto_backoff, 2);
    assert_eq!(context.on_packet_ack_count, 4);
    assert_eq!(context.on_new_packet_ack_count, 3);
    assert_eq!(context.validate_packet_ack_count, 4);
    assert_eq!(context.on_packet_loss_count, 3);
    assert_eq!(
        context.path().rtt_estimator.latest_rtt(),
        Duration::from_millis(3000)
    );
    assert_eq!(3, context.on_rtt_update_count);

    // Send and ack a non ack eliciting packet
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(11)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::NonEliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    ack_packets(
        11..=11,
        ack_receive_time,
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    assert_eq!(context.path().congestion_controller.lost_bytes, 0);
    assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
    assert_eq!(context.on_packet_ack_count, 5);
    assert_eq!(context.on_new_packet_ack_count, 4);
    assert_eq!(context.validate_packet_ack_count, 5);
    assert_eq!(context.on_packet_loss_count, 3);
    // RTT remains unchanged
    assert_eq!(
        context.path().rtt_estimator.latest_rtt(),
        Duration::from_millis(3000)
    );
    assert_eq!(3, context.on_rtt_update_count);
}

// Test that receiving an invalid ack frame still allows for `on_timeout`
// to be invoked without panicking
#[test]
fn on_invalid_ack_frame() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();
    let random = &mut random::testing::Generator::default();

    let time_sent = time::now() + Duration::from_secs(10);

    // Send packets 1 to 5, skipping packet 3
    for i in [1, 2, 4, 5] {
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(i)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: 128,
                bytes_progressed: 0,
            },
            time_sent,
            ExplicitCongestionNotification::default(),
            transmission::Mode::Normal,
            None,
            &mut context,
            &mut publisher,
        );
    }
    manager.on_transmit_burst_complete(context.active_path(), time_sent, true);

    // Ack packet 2, arming the loss timer for packet 1
    let ack_receive_time = time_sent + Duration::from_millis(10);
    ack_packets(
        2..=2,
        ack_receive_time,
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );
    assert!(manager.loss_timer.is_armed());

    // Receive an ACK that fails validation (packet 3 was never sent)
    context.fail_validation = true;
    ack_packets(
        1..=5,
        ack_receive_time,
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    // Call on_timeout to verify there is no panic
    manager.on_timeout(
        manager.next_expiration().unwrap(),
        random,
        0,
        &mut context,
        &mut publisher,
    );

    // Verify packet 1 is lost
    assert!(context
        .lost_packets
        .contains(&space.new_packet_number(1_u8.into())));
}

#[derive(Clone, Debug, TypeGenerator)]
struct Packet {
    outcome: Outcome,
    transmission_mode: transmission::Mode,
    ecn: ExplicitCongestionNotification,
    congestion_controlled: bool,
}

#[test]
fn on_packet_loss_called_for_all_lost_packets() {
    let space = PacketNumberSpace::ApplicationData;
    let mut publisher = Publisher::no_snapshot();

    bolero::check!()
        .with_type::<Vec<Packet>>()
        .cloned()
        .for_each(|packets| {
            let mut manager = ServerManager::new(space);
            let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
            let mut context = MockContext::new(&mut path_manager);
            let mut packet_number = space.new_packet_number(VarInt::from_u8(0));
            let mut now = now();
            let mut packet_sent_count = 0;

            for packet in packets.iter().filter(|packet| {
                // Non-congestion controlled packets have 0 bytes sent
                ((packet.outcome.bytes_sent > 0) == packet.outcome.is_congestion_controlled)
                    // Ect1 is not used
                    && packet.ecn != ExplicitCongestionNotification::Ect1
            }) {
                manager.on_packet_sent(
                    packet_number,
                    packet.outcome,
                    now,
                    packet.ecn,
                    packet.transmission_mode,
                    None,
                    &mut context,
                    &mut publisher,
                );
                packet_sent_count += 1;
                packet_number = packet_number.next().unwrap();
                now += Duration::from_millis(1);
            }

            // Send and ack one packet much later to trigger loss recovery
            manager.on_packet_sent(
                packet_number,
                Default::default(),
                now,
                Default::default(),
                transmission::Mode::Normal,
                None,
                &mut context,
                &mut publisher,
            );
            ack_packets(
                packet_number.as_u64()..=packet_number.as_u64(),
                now + Duration::from_secs(10),
                &mut context,
                &mut manager,
                None,
                &mut publisher,
            );

            // Every packet should be lost
            assert_eq!(packet_sent_count, context.on_packet_loss_count);
            assert!(manager.sent_packets.is_empty());
        });
}

#[test]
// pto_backoff reset should happen for the path the packet was sent on
//
// Setup 1:
// - create path manager with two validated  paths
// - send a packet on each path
// - set pto_backoff to non-initial value
//
// Trigger 1:
// - send ack for packet 1 on path 1
//
// Expectation 1:
// - pto_backoff for first_path is reset
// - pto_backoff for second_path_id path is not reset
//
// Trigger 2:
// - send ack for packet 2 on path 1
//
// Expectation 2:
// - pto_backoff for first_path path is not reset
// - pto_backoff for second_path is reset
fn process_new_acked_packets_update_pto_timer() {
    // Setup:
    let space = PacketNumberSpace::ApplicationData;
    let mut publisher = Publisher::snapshot();
    let packet_bytes = 128;
    let (first_addr, first_path_id, _second_addr, second_path_id, mut manager, mut path_manager) =
        helper_generate_multi_path_manager(space, &mut publisher);
    let mut context = MockContext::new(&mut path_manager);
    let ecn = ExplicitCongestionNotification::default();
    let time_sent = time::now() + Duration::from_secs(10);

    // Send packets 1 on first_path
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    // Send packets 2 on second_path
    context.set_path_id(second_path_id);
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(2)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // Start the pto backoff at 2 so we can tell if it was reset
    context.path_mut_by_id(first_path_id).pto_backoff = 2;
    context.path_mut_by_id(second_path_id).pto_backoff = 2;

    // Trigger 1:
    // Ack packet first_path
    let ack_receive_time = time_sent + Duration::from_millis(500);
    helper_ack_packets_on_path(
        1..=1,
        ack_receive_time,
        &mut context,
        &mut manager,
        first_addr,
        None,
        &mut publisher,
    );

    // Expectation 1:
    assert_eq!(
        context.path_mut_by_id(first_path_id).pto_backoff,
        INITIAL_PTO_BACKOFF
    );
    assert_eq!(context.path_mut_by_id(second_path_id).pto_backoff, 2);

    // Reset the pto backoff to 2 so we can tell if it was reset
    context.path_mut_by_id(first_path_id).pto_backoff = 2;
    context.path_mut_by_id(second_path_id).pto_backoff = 2;

    // Trigger 2:
    // Ack packet second_path
    let ack_receive_time = ack_receive_time + Duration::from_secs(1);
    helper_ack_packets_on_path(
        2..=2,
        ack_receive_time,
        &mut context,
        &mut manager,
        first_addr,
        None,
        &mut publisher,
    );

    // Expectation 2:
    assert_eq!(context.path_mut_by_id(first_path_id).pto_backoff, 2);
    assert_eq!(
        context.path_mut_by_id(second_path_id).pto_backoff,
        INITIAL_PTO_BACKOFF
    );
}

#[test]
// congestion_controller.on_packet_ack should be updated for the path the packet was sent on
//
// Setup:
// - create path manager with two validated paths
// - send a packet on each path
//  - packet 1 on path 1
//  - packet 2 on path 2
//
// Trigger:
// - send ack for packet 1 and 2 on path 1
//
// Expectation:
// - cc.on_packet_ack should be incremented once for the first_path
// - cc.on_packet_ack should be incremented once for the second_path
fn process_new_acked_packets_congestion_controller() {
    // Setup:
    let space = PacketNumberSpace::ApplicationData;
    let mut publisher = Publisher::snapshot();
    let packet_bytes = 128;
    let (first_addr, first_path_id, _second_addr, second_path_id, mut manager, mut path_manager) =
        helper_generate_multi_path_manager(space, &mut publisher);
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);

    let time_sent = time::now() + Duration::from_secs(10);

    // Send packets 1 on first_path
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    // Send packets 2 on second_path
    context.set_path_id(second_path_id);
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(2)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // Trigger:
    // Ack packets 1 and 2 on path 1
    let ack_receive_time = time_sent + Duration::from_millis(500);
    helper_ack_packets_on_path(
        1..=2,
        ack_receive_time,
        &mut context,
        &mut manager,
        first_addr,
        None,
        &mut publisher,
    );

    // Expectation:
    assert_eq!(
        context
            .path_by_id(first_path_id)
            .congestion_controller
            .on_packet_ack,
        1
    );
    assert_eq!(
        context
            .path_by_id(second_path_id)
            .congestion_controller
            .on_packet_ack,
        1
    );
}

#[test]
// since pto is shared across paths, acks on either paths should update pto timer
//
// Setup 1:
// - create path manager with two peer_validated and not AmplificationLimited paths
// -
// - send a packet on each path (make sure sent_packets is non-empty)
//  - packet 1 on path 1
//  - packet 2 on path 2
//
// Trigger 1:
// - send ack for packet 1 on path 1
//
// Expectation 1:
// - pto.timer should be armed
//
// Setup 2:
// - send a packet on path 1 (make sure sent_packets is non-empty)
//
// Trigger 2:
// - send ack for packet 2 on path 2
//
// Expectation 2:
// - pto.timer should be armed
fn process_new_acked_packets_pto_timer() {
    // Setup:
    let space = PacketNumberSpace::ApplicationData;
    let mut publisher = Publisher::snapshot();
    let packet_bytes = 128;
    let (first_addr, _first_path_id, second_addr, second_path_id, mut manager, mut path_manager) =
        helper_generate_multi_path_manager(space, &mut publisher);
    let mut context = MockContext::new(&mut path_manager);
    let ecn = ExplicitCongestionNotification::default();
    let time_sent = time::now() + Duration::from_secs(10);

    // Send packets 1 on first_path
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    // Send packets 2 on second_path
    context.set_path_id(second_path_id);
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(2)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    manager.pto.cancel();

    // Trigger 1:
    // Ack packet 1 on path 1
    let ack_receive_time = time_sent + Duration::from_millis(500);
    helper_ack_packets_on_path(
        1..=1,
        ack_receive_time,
        &mut context,
        &mut manager,
        first_addr,
        None,
        &mut publisher,
    );

    // Expectation 1:
    assert!(manager.pto.is_armed());

    // Setup 2:
    manager.pto.cancel();
    // Send packets 3 on first path so that sent_packets is non-empty
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(3)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // Trigger 2:
    // Ack packet 2 on path 1
    let ack_receive_time = time_sent + Duration::from_millis(500);
    helper_ack_packets_on_path(
        2..=2,
        ack_receive_time,
        &mut context,
        &mut manager,
        second_addr,
        None,
        &mut publisher,
    );

    // Expectation 2:
    assert!(manager.pto.is_armed());
}

// Test that the PTO timer is armed after a non-congestion controlled
// packet is acked
#[test]
fn ack_non_congestion_controlled_acked_packets_pto_timer() {
    // Setup:
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let mut publisher = Publisher::snapshot();
    let packet_bytes = 128;
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let mut context = MockContext::new(&mut path_manager);
    let ecn = ExplicitCongestionNotification::default();
    let time_sent = time::now() + Duration::from_secs(10);

    // Remove amplification limits
    context.path_mut().on_handshake_packet();

    // Send a non congestion controlled packet
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: false,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    // Send another packet
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(2)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // Cancel the PTO timer to verify it is re-armed
    manager.pto.cancel();

    let ack_receive_time = time_sent + Duration::from_millis(500);
    ack_packets(
        1..=1,
        ack_receive_time,
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    // The PTO timer should be armed
    assert!(manager.pto.is_armed());
}

#[test]
// Increase in ECN CE count should cause congestion event
// Out of order Ack Frames should not fail ECN validation
//
// Setup 1:
// - Send 10 ECT0 marked packets
//
// Trigger 1:
// - Acknowledge the packets with valid ECN counts, including
//   an increased CE count
//
// Expectation 1:
// - Congestion Event recorded
//
// Trigger 2:
// - Send out of order Ack
//
// Expectation 2:
// - ECN controller is still capable
fn process_new_acked_packets_process_ecn() {
    // Setup:
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let packet_bytes = 128;
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let mut context = MockContext::new(&mut path_manager);
    let time_sent = time::now() + Duration::from_secs(10);
    let mut publisher = Publisher::snapshot();

    // Send 10 ECT0 marked packets
    for i in 1..=10 {
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(i)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                bytes_progressed: 0,
            },
            time_sent,
            ExplicitCongestionNotification::Ect0,
            transmission::Mode::Normal,
            None,
            &mut context,
            &mut publisher,
        );
    }

    // Trigger 1:
    // Ack packets 2-5 and then 6-10
    let ack_receive_time = time_sent + Duration::from_millis(500);
    let ack_ecn_counts = EcnCounts {
        ect_0_count: VarInt::from_u8(3),
        ect_1_count: Default::default(),
        ce_count: VarInt::from_u8(1),
    };
    ack_packets(
        2..=5,
        ack_receive_time,
        &mut context,
        &mut manager,
        Some(ack_ecn_counts),
        &mut publisher,
    );
    let ack_ecn_counts = EcnCounts {
        ect_0_count: VarInt::from_u8(9),
        ect_1_count: Default::default(),
        ce_count: VarInt::from_u8(1),
    };
    ack_packets(
        6..=10,
        ack_receive_time,
        &mut context,
        &mut manager,
        Some(ack_ecn_counts),
        &mut publisher,
    );

    // Expectation 1:
    assert_eq!(ack_ecn_counts, manager.baseline_ecn_counts);
    assert_eq!(1, context.path().congestion_controller.congestion_events);
    assert!(context.path().ecn_controller.is_capable());

    //= https://www.rfc-editor.org/rfc/rfc9000#section-13.4.2.1
    //= type=test
    //# Validating ECN counts from reordered ACK frames can result in failure.
    //# An endpoint MUST NOT fail ECN validation as a result of processing an
    //# ACK frame that does not increase the largest acknowledged packet number.

    // Trigger 2:
    // Out of order Ack does not fail ECN validation
    let out_of_order_ack_ecn_counts = EcnCounts {
        ect_0_count: VarInt::from_u8(1),
        ect_1_count: Default::default(),
        ce_count: VarInt::from_u8(0),
    };
    ack_packets(
        1..=1,
        ack_receive_time,
        &mut context,
        &mut manager,
        Some(out_of_order_ack_ecn_counts),
        &mut publisher,
    );

    // Expectation 2:
    assert_eq!(ack_ecn_counts, manager.baseline_ecn_counts);
    assert!(context.path().ecn_controller.is_capable());
}

#[test]
// Increase in ECN CE count should not cause congestion event if ECN validation fails
//
// Setup 1:
// - Send 10 ECT0 marked packets
//
// Trigger 1:
// - Acknowledge the packets with invalid ECN counts
//
// Expectation 1:
// - No Congestion Event recorded
fn process_new_acked_packets_failed_ecn_validation_does_not_cause_congestion_event() {
    // Setup:
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let packet_bytes = 128;
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let mut context = MockContext::new(&mut path_manager);
    let time_sent = time::now() + Duration::from_secs(10);
    let mut publisher = Publisher::snapshot();

    // Send 10 ECT0 marked packets
    for i in 1..=10 {
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(i)),
            transmission::Outcome {
                ack_elicitation: AckElicitation::Eliciting,
                is_congestion_controlled: true,
                bytes_sent: packet_bytes,
                bytes_progressed: 0,
            },
            time_sent,
            ExplicitCongestionNotification::Ect0,
            transmission::Mode::Normal,
            None,
            &mut context,
            &mut publisher,
        );
    }

    // Trigger 1:
    // Ack packets with bad ECN counts
    let ack_receive_time = time_sent + Duration::from_millis(500);
    let ack_ecn_counts = EcnCounts {
        ect_0_count: VarInt::from_u8(8),
        ect_1_count: VarInt::from_u8(1), // We never send ECT1 so this is invalid
        ce_count: VarInt::from_u8(1),
    };
    ack_packets(
        1..=10,
        ack_receive_time,
        &mut context,
        &mut manager,
        Some(ack_ecn_counts),
        &mut publisher,
    );

    // Expectation 1:
    assert_eq!(ack_ecn_counts, manager.baseline_ecn_counts);
    assert_eq!(0, context.path().congestion_controller.congestion_events);
    assert!(!context.path().ecn_controller.is_capable());
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-5.1
//= type=test
//# To avoid generating multiple RTT samples for a single packet, an ACK
//# frame SHOULD NOT be used to update RTT estimates if it does not newly
//# acknowledge the largest acknowledged packet.
#[test]
fn no_rtt_update_when_not_acknowledging_the_largest_acknowledged_packet() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let packet_bytes = 128;
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();

    let time_sent = time::now() + Duration::from_secs(10);

    // Send 2 packets
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(0)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    assert_eq!(manager.sent_packets.iter().count(), 2);

    // Ack packet 1
    let ack_receive_time = time_sent + Duration::from_millis(500);
    ack_packets(
        1..=1,
        ack_receive_time,
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    // New rtt estimate because the largest packet was newly acked
    assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
    assert_eq!(
        manager.largest_acked_packet,
        Some(space.new_packet_number(VarInt::from_u8(1)))
    );
    assert_eq!(
        context.path().rtt_estimator.latest_rtt(),
        Duration::from_millis(500)
    );
    assert_eq!(1, context.on_rtt_update_count);

    // Ack packets 0 and 1
    let ack_receive_time = time_sent + Duration::from_millis(1500);
    ack_packets(
        0..=1,
        ack_receive_time,
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    // No new rtt estimate because the largest packet was not newly acked
    assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
    assert_eq!(
        context.path().rtt_estimator.latest_rtt(),
        Duration::from_millis(500)
    );
    assert_eq!(1, context.on_rtt_update_count);
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-9.4
//= type=test
//# Packets sent on the old path MUST NOT contribute to
//# congestion control or RTT estimation for the new path.
#[test]
fn no_rtt_update_when_receiving_packet_on_different_path() {
    let space = PacketNumberSpace::ApplicationData;
    let mut publisher = Publisher::snapshot();
    let (first_addr, _first_path_id, second_addr, _second_path_id, mut manager, mut path_manager) =
        helper_generate_multi_path_manager(space, &mut publisher);
    let packet_bytes = 128;
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);

    let time_sent = time::now() + Duration::from_secs(10);

    // Send packet
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(0)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    assert_eq!(manager.sent_packets.iter().count(), 2);

    // clear the cc counts
    context.path_mut().congestion_controller.on_rtt_update = 0;

    // Ack packet 0 on different path as it was sent.. expect no rtt update
    let ack_receive_time = time_sent + Duration::from_millis(500);
    helper_ack_packets_on_path(
        0..=0,
        ack_receive_time,
        &mut context,
        &mut manager,
        second_addr,
        None,
        &mut publisher,
    );

    // no rtt estimate because the packet was received on different path
    assert_eq!(context.path().congestion_controller.on_rtt_update, 0);
    assert_eq!(
        manager.largest_acked_packet,
        Some(space.new_packet_number(VarInt::from_u8(0)))
    );
    assert_eq!(
        context.path().rtt_estimator.latest_rtt(),
        Duration::from_millis(DEFAULT_INITIAL_RTT.as_millis() as u64)
    );
    assert_eq!(0, context.on_rtt_update_count);

    // Ack packet 1 on same path as it was sent.. expect rtt update
    let ack_receive_time = time_sent + Duration::from_millis(1500);
    helper_ack_packets_on_path(
        1..=1,
        ack_receive_time,
        &mut context,
        &mut manager,
        first_addr,
        None,
        &mut publisher,
    );

    // rtt estimate because the packet was received on same path
    assert_eq!(context.path().congestion_controller.on_rtt_update, 1);
    assert_eq!(
        manager.largest_acked_packet,
        Some(space.new_packet_number(VarInt::from_u8(1)))
    );
    assert_eq!(
        context.path().rtt_estimator.latest_rtt(),
        Duration::from_millis(1500)
    );
    assert_eq!(1, context.on_rtt_update_count);
}

#[test]
// It is possible to receive acks for packets that were sent on different paths. In this case
// we still update rtt if the largest acked packet was sent/received on the same path.
//
// Setup:
// - create path manager with two paths
//
// Trigger:
// - send packet on each path
//   - packet 0: 2nd path: time 300
//   - packet 1: 1st path: time 300
// - send ack for both packet on 1st path
//
// Expectation:
// - update rtt for 1st path using packet 1 time
//   - since packet 1 is largest acked and was sent/received on 1st path
// - rtt for 2nd apth should be unchanged
fn rtt_update_when_receiving_ack_from_multiple_paths() {
    // Setup:
    let space = PacketNumberSpace::ApplicationData;
    let mut publisher = Publisher::snapshot();
    let packet_bytes = 128;
    let (first_addr, first_path_id, _second_addr, second_path_id, mut manager, mut path_manager) =
        helper_generate_multi_path_manager(space, &mut publisher);
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);

    // Trigger:
    let time_sent = time::now() + Duration::from_secs(10);
    let sent_time = time_sent + Duration::from_millis(300);
    let ack_receive_time = time_sent + Duration::from_millis(1000);

    // send packet 0 packet on second path. sent +500
    context.set_path_id(second_path_id);
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(0)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        sent_time,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // send packet 1 (largest) on first path. sent + 200
    context.set_path_id(first_path_id);
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: packet_bytes,
            bytes_progressed: 0,
        },
        sent_time,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    assert_eq!(manager.sent_packets.iter().count(), 2);

    // receive ack for both packets on address of first path (first packet is largest)
    helper_ack_packets_on_path(
        0..=1,
        ack_receive_time,
        &mut context,
        &mut manager,
        first_addr,
        None,
        &mut publisher,
    );

    let first_path = context.path_by_id(first_path_id);
    let second_path = context.path_by_id(second_path_id);

    // Expectation:
    // received/sent largest packet on first path so we expect an rtt update using sent time of first path
    // assert common component
    assert_eq!(
        manager.largest_acked_packet,
        Some(space.new_packet_number(VarInt::from_u8(1)))
    );
    assert_eq!(context.on_rtt_update_count, 1);

    // assert first path
    assert_eq!(
        first_path.rtt_estimator.latest_rtt(),
        ack_receive_time - sent_time
    );
    assert_eq!(first_path.congestion_controller.on_rtt_update, 1);

    // assert second path
    assert_eq!(
        second_path.rtt_estimator.latest_rtt(),
        Duration::from_millis(DEFAULT_INITIAL_RTT.as_millis() as u64)
    );
    assert_eq!(second_path.congestion_controller.on_rtt_update, 0);
}

//= https://www.rfc-editor.org/rfc/rfc9002#appendix-A.10
//= type=test
#[test]
fn detect_and_remove_lost_packets() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let now = time::now();
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();
    let random = &mut random::testing::Generator::default();

    manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(10)));

    let mut time_sent = time::now();
    let outcome = transmission::Outcome {
        ack_elicitation: AckElicitation::Eliciting,
        is_congestion_controlled: true,
        bytes_sent: 1,
        bytes_progressed: 0,
    };

    // Send a packet that was sent too long ago (lost)
    let old_packet_time_sent = space.new_packet_number(VarInt::from_u8(0));
    manager.on_packet_sent(
        old_packet_time_sent,
        outcome,
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // time threshold = max(kTimeThreshold * max(smoothed_rtt, latest_rtt), kGranularity)
    // time threshold = max(9/8 * 8) = 9
    context.path_mut().rtt_estimator.update_rtt(
        Duration::from_secs(0),
        Duration::from_secs(8),
        now,
        true,
        space,
    );
    let expected_time_threshold = Duration::from_secs(9);
    assert_eq!(
        expected_time_threshold,
        context.path().rtt_estimator.loss_time_threshold(),
    );

    time_sent += Duration::from_secs(10);

    // Send a packet that was sent within the time threshold but is with a packet number
    // K_PACKET_THRESHOLD away from the largest (lost)
    let old_packet_packet_number =
        space.new_packet_number(VarInt::new(10 - K_PACKET_THRESHOLD).unwrap());
    manager.on_packet_sent(
        old_packet_packet_number,
        outcome,
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // Send a packet that is less than the largest acked but not lost
    let not_lost = space.new_packet_number(VarInt::from_u8(9));
    manager.on_packet_sent(
        not_lost,
        outcome,
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // Send a packet larger than the largest acked (not lost)
    let larger_than_largest = manager.largest_acked_packet.unwrap().next().unwrap();
    manager.on_packet_sent(
        larger_than_largest,
        outcome,
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // Four packets sent, each size 1 byte
    let bytes_in_flight: u16 = manager
        .sent_packets
        .iter()
        .map(|(_, info)| info.sent_bytes)
        .sum();
    assert_eq!(bytes_in_flight, 4);

    let now = time_sent;
    manager.detect_and_remove_lost_packets(now, random, &mut context, &mut publisher);

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.2
    //= type=test
    //# Once a later packet within the same packet number space has been
    //# acknowledged, an endpoint SHOULD declare an earlier packet lost if it
    //# was sent a threshold amount of time in the past.

    // Two packets lost, each size 1 byte
    assert_eq!(context.path().congestion_controller.lost_bytes, 2);
    // Two packets remaining
    let bytes_in_flight: u16 = manager
        .sent_packets
        .iter()
        .map(|(_, info)| info.sent_bytes)
        .sum();
    assert_eq!(bytes_in_flight, 2);

    let sent_packets = &manager.sent_packets;
    assert!(context.lost_packets.contains(&old_packet_time_sent));
    assert!(sent_packets.get(old_packet_time_sent).is_none());

    assert!(context.lost_packets.contains(&old_packet_packet_number));
    assert!(sent_packets.get(old_packet_packet_number).is_none());

    assert!(!context.lost_packets.contains(&larger_than_largest));
    assert!(sent_packets.get(larger_than_largest).is_some());

    assert!(!context.lost_packets.contains(&not_lost));
    assert!(sent_packets.get(not_lost).is_some());

    let expected_loss_time =
        sent_packets.get(not_lost).unwrap().time_sent + expected_time_threshold;
    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.1.2
    //= type=test
    //# If packets sent prior to the largest acknowledged packet cannot yet
    //# be declared lost, then a timer SHOULD be set for the remaining time.
    assert!(manager.loss_timer.is_armed());
    assert_eq!(
        Some(expected_loss_time),
        manager.loss_timer.next_expiration()
    );
}

#[test]
// persistent_congestion should only be calculated for the specified path
//
// Setup:
// - create path manager with two paths
// - largest ack is 20 (way above K_PACKET_THRESHOLD)
// - 1-2 contiguous lost packets for path 1 (period: 2-1 = 1)
// - 3-6 contiguous lost packets for path 2 (period: 6-3 = 3)
// - 7-9 contiguous lost packets for path 1 (period: 9-7 = 2)
//
// Trigger:
// - call detect_lost_packets for path 1
//
// Expectation:
// - ensure max_persistent_congestion_period is 2 corresponding to range 7-9
// - ensure path_id is 1
fn detect_lost_packets_persistent_congestion_path_aware() {
    // Setup:
    let space = PacketNumberSpace::ApplicationData;
    let mut publisher = Publisher::snapshot();
    let (_first_addr, first_path_id, _second_addr, second_path_id, mut manager, mut path_manager) =
        helper_generate_multi_path_manager(space, &mut publisher);
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);

    let mut now = time::now();
    manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(20)));

    // create first rtt samples so they can enter enter persistent_congestion
    context
        .path_mut_by_id(first_path_id)
        .rtt_estimator
        .update_rtt(
            Duration::from_secs(0),
            Duration::from_secs(8),
            now,
            true,
            space,
        );
    context
        .path_mut_by_id(second_path_id)
        .rtt_estimator
        .update_rtt(
            Duration::from_secs(0),
            Duration::from_secs(8),
            now,
            true,
            space,
        );

    // Trigger:
    let outcome = transmission::Outcome {
        ack_elicitation: AckElicitation::Eliciting,
        is_congestion_controlled: true,
        bytes_sent: 1,
        bytes_progressed: 0,
    };

    // Send a packet that was sent too long ago (lost)
    for i in 1..=2 {
        now += Duration::from_secs(1);
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(i)),
            outcome,
            now,
            ecn,
            transmission::Mode::Normal,
            None,
            &mut context,
            &mut publisher,
        );
    }
    // Send a packet that was sent too long ago (lost)
    for i in 3..=6 {
        now += Duration::from_secs(1);
        context.set_path_id(second_path_id);
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(i)),
            outcome,
            now,
            ecn,
            transmission::Mode::Normal,
            None,
            &mut context,
            &mut publisher,
        );
    }
    // Send a packet that was sent too long ago (lost)
    for i in 7..=9 {
        now += Duration::from_secs(1);
        context.set_path_id(first_path_id);
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(i)),
            outcome,
            now,
            ecn,
            transmission::Mode::Normal,
            None,
            &mut context,
            &mut publisher,
        );
    }

    // increase the time so all sent packets will be considered lost
    now += Duration::from_secs(10);

    let expected_time_threshold = Duration::from_secs(9);
    assert_eq!(
        expected_time_threshold,
        context
            .path_by_id(first_path_id)
            .rtt_estimator
            .loss_time_threshold(),
    );

    // 1-9 packets packets sent, each size 1 byte
    let bytes_in_flight: u16 = manager
        .sent_packets
        .iter()
        .map(|(_, info)| info.sent_bytes)
        .sum();
    assert_eq!(bytes_in_flight, 9);

    let (max_persistent_congestion_period, _sent_packets_to_remove) =
        manager.detect_lost_packets(now, &mut context, &mut publisher);

    // Expectation:
    assert_eq!(max_persistent_congestion_period, Duration::from_secs(2));
}

#[test]
// persistent_congestion should only be calculated for the specified path
//
// Setup:
// - create path manager with two paths that are not persistent_congestion
// - create PacketDetails for lost packets on each path
// - verify both paths are not in persistent_congestion
// - provide both paths with rrt sample so we can verify they are cleared later
//
// Trigger:
// - call remove_lost_packets for path 2
//
// Expectation:
// - ensure path 1 is NOT persistent_congestion
// - ensure path 1 first_rtt_sample is NOT cleared
// - ensure path 2 is persistent_congestion
// - ensure path 2 first_rtt_sample is cleared
fn remove_lost_packets_persistent_congestion_path_aware() {
    // Setup:
    let space = PacketNumberSpace::ApplicationData;
    let mut publisher = Publisher::snapshot();
    let (_first_addr, first_path_id, _second_addr, second_path_id, mut manager, mut path_manager) =
        helper_generate_multi_path_manager(space, &mut publisher);
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let mut now = time::now();
    let random = &mut random::testing::Generator::default();

    assert_eq!(
        context
            .path_by_id(first_path_id)
            .congestion_controller
            .persistent_congestion,
        None
    );
    assert_eq!(
        context
            .path_by_id(second_path_id)
            .congestion_controller
            .persistent_congestion,
        None
    );

    // create first rtt samples so we can check it later
    context
        .path_mut_by_id(first_path_id)
        .rtt_estimator
        .update_rtt(
            Duration::from_secs(0),
            Duration::from_secs(0),
            now,
            true,
            space,
        );
    context
        .path_mut_by_id(second_path_id)
        .rtt_estimator
        .update_rtt(
            Duration::from_secs(0),
            Duration::from_secs(0),
            now,
            true,
            space,
        );
    assert!(context
        .path_by_id(first_path_id)
        .rtt_estimator
        .first_rtt_sample()
        .is_some());
    assert!(context
        .path_by_id(second_path_id)
        .rtt_estimator
        .first_rtt_sample()
        .is_some());

    now += Duration::from_secs(10);
    let sent_packets_to_remove = PacketNumberRange::new(
        space.new_packet_number(VarInt::from_u8(9)),
        space.new_packet_number(VarInt::from_u8(10)),
    );
    context.set_path_id(first_path_id);
    manager.on_packet_sent(
        sent_packets_to_remove.start(),
        Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            bytes_progressed: 0,
        },
        now,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    context.set_path_id(second_path_id);
    manager.on_packet_sent(
        sent_packets_to_remove.end(),
        Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            bytes_progressed: 0,
        },
        now,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // Trigger:
    context.set_path_id(second_path_id);
    manager.remove_lost_packets(
        now,
        Duration::from_secs(20),
        sent_packets_to_remove,
        random,
        &mut context,
        &mut publisher,
    );

    // Expectation:
    assert_eq!(
        context
            .path_by_id(first_path_id)
            .congestion_controller
            .persistent_congestion,
        Some(false)
    );
    assert!(context
        .path_by_id(first_path_id)
        .rtt_estimator
        .first_rtt_sample()
        .is_some());
    assert_eq!(
        context
            .path_by_id(second_path_id)
            .congestion_controller
            .persistent_congestion,
        Some(true)
    );
    assert!(context
        .path_by_id(second_path_id)
        .rtt_estimator
        .first_rtt_sample()
        .is_none());
}

#[test]
fn detect_and_remove_lost_packets_nothing_lost() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(10)));
    let mut publisher = Publisher::snapshot();
    let random = &mut random::testing::Generator::default();

    let time_sent = time::now();
    let outcome = transmission::Outcome {
        ack_elicitation: AckElicitation::Eliciting,
        is_congestion_controlled: true,
        bytes_sent: 1,
        bytes_progressed: 0,
    };

    // Send a packet that is less than the largest acked but not lost
    let not_lost = space.new_packet_number(VarInt::from_u8(9));
    manager.on_packet_sent(
        not_lost,
        outcome,
        time_sent,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    manager.detect_and_remove_lost_packets(time_sent, random, &mut context, &mut publisher);

    // Verify no lost bytes are sent to the congestion controller and
    // on_packets_lost is not called
    assert_eq!(context.lost_packets.len(), 0);
    assert_eq!(context.path().congestion_controller.lost_bytes, 0);
    assert_eq!(context.path().congestion_controller.on_packets_lost, 0);
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-14.4
//= type=test
//# Loss of a QUIC packet that is carried in a PMTU probe is therefore not a
//# reliable indication of congestion and SHOULD NOT trigger a congestion
//# control reaction; see Item 7 in Section 3 of [DPLPMTUD].

//= https://www.rfc-editor.org/rfc/rfc8899#section-3
//= type=test
//# Loss of a probe packet SHOULD NOT be treated as an
//# indication of congestion and SHOULD NOT trigger a congestion
//# control reaction [RFC4821] because this could result in
//# unnecessary reduction of the sending rate.
#[test]
fn detect_and_remove_lost_packets_mtu_probe() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(10)));
    let mut publisher = Publisher::snapshot();
    let random = &mut random::testing::Generator::default();

    let time_sent = time::now();
    let outcome = transmission::Outcome {
        ack_elicitation: AckElicitation::Eliciting,
        is_congestion_controlled: true,
        bytes_sent: MINIMUM_MAX_DATAGRAM_SIZE as usize + 1,
        bytes_progressed: 0,
    };

    // Send an MTU probe packet
    let lost_packet = space.new_packet_number(VarInt::from_u8(2));
    manager.on_packet_sent(
        lost_packet,
        outcome,
        time_sent,
        ecn,
        transmission::Mode::MtuProbing,
        None,
        &mut context,
        &mut publisher,
    );
    assert_eq!(
        context.path().congestion_controller.bytes_in_flight,
        MINIMUM_MAX_DATAGRAM_SIZE as u32 + 1
    );

    manager.detect_and_remove_lost_packets(time_sent, random, &mut context, &mut publisher);

    // Verify no lost bytes are sent to the congestion controller and
    // on_packets_lost is not called, but bytes_in_flight is reduced
    assert_eq!(context.lost_packets.len(), 1);
    assert_eq!(context.path().congestion_controller.lost_bytes, 0);
    assert_eq!(context.path().congestion_controller.on_packets_lost, 0);
    assert_eq!(context.path().congestion_controller.bytes_in_flight, 0);
}

#[test]
fn persistent_congestion() {
    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.2
    //= type=test
    //# A sender that does not have state for all packet
    //# number spaces or an implementation that cannot compare send times
    //# across packet number spaces MAY use state for just the packet number
    //# space that was acknowledged.
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();

    let time_zero = time::now() + Duration::from_secs(10);
    // The RFC doesn't mention it, but it is implied that the first RTT sample has already
    // been received when this example begins, otherwise packet #2 would not be considered
    // part of the persistent congestion period.
    context.path_mut().rtt_estimator.update_rtt(
        Duration::from_millis(10),
        Duration::from_millis(600),
        time::now(),
        true,
        space,
    );

    let mut outcome = transmission::Outcome {
        ack_elicitation: AckElicitation::Eliciting,
        is_congestion_controlled: true,
        bytes_sent: 1,
        bytes_progressed: 0,
    };

    // t=0: Send packet #1 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        outcome,
        time_zero,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=1: Send packet #2 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(2)),
        outcome,
        time_zero + Duration::from_secs(1),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=1.2: Recv acknowledgement of #1
    ack_packets(
        1..=1,
        time_zero + Duration::from_millis(1200),
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    // t=2-6: Send packets #3 - #7 (app data)
    // These packets are NonEliciting, which are allowed to be part of a Persistent Congestion Period
    // as long as they are not the start or end of the period.
    outcome.ack_elicitation = AckElicitation::NonEliciting;
    for t in 2..=6 {
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(t + 1)),
            outcome,
            time_zero + Duration::from_secs(t.into()),
            ecn,
            transmission::Mode::Normal,
            None,
            &mut context,
            &mut publisher,
        );
    }
    outcome.ack_elicitation = AckElicitation::Eliciting;

    // t=8: Send packet #8 (PTO 1)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(8)),
        outcome,
        time_zero + Duration::from_secs(8),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=12: Send packet #9 (PTO 2)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(9)),
        outcome,
        time_zero + Duration::from_secs(12),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=12.2: Recv acknowledgement of #9
    ack_packets(
        9..=9,
        time_zero + Duration::from_millis(12200),
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.3
    //# Packets 2 through 8 are declared lost when the acknowledgment for
    //# packet 9 is received at "t = 12.2".
    assert_eq!(7, context.on_packet_loss_count);

    //= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.3
    //# The congestion period is calculated as the time between the oldest
    //# and newest lost packets: "8 - 1 = 7".
    assert!(
        context
            .path()
            .rtt_estimator
            .persistent_congestion_threshold()
            < Duration::from_secs(7)
    );
    assert_eq!(
        Some(true),
        context.path().congestion_controller.persistent_congestion
    );
    assert_eq!(context.path().rtt_estimator.first_rtt_sample(), None);
    assert_eq!(1, context.path().congestion_controller.loss_bursts);

    // t=20: Send packet #10
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(10)),
        outcome,
        time_zero + Duration::from_secs(20),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=21: Recv acknowledgement of #10
    ack_packets(
        10..=10,
        time_zero + Duration::from_secs(21),
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    //= https://www.rfc-editor.org/rfc/rfc9002#section-5.2
    //= type=test
    //# Endpoints SHOULD set the min_rtt to the newest RTT sample after
    //# persistent congestion is established.
    assert_eq!(
        context.path().rtt_estimator.min_rtt(),
        Duration::from_secs(1)
    );
    assert_eq!(
        context.path().rtt_estimator.smoothed_rtt(),
        Duration::from_secs(1)
    );
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.6
//= type=test
#[test]
fn persistent_congestion_multiple_periods() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let time_zero = time::now() + Duration::from_secs(10);
    let mut publisher = Publisher::snapshot();

    let outcome = transmission::Outcome {
        ack_elicitation: AckElicitation::Eliciting,
        is_congestion_controlled: true,
        bytes_sent: 1,
        bytes_progressed: 0,
    };

    // t=0: Send packet #1 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        outcome,
        time_zero,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=1: Send packet #2 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(2)),
        outcome,
        time_zero + Duration::from_secs(1),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=1.2: Recv acknowledgement of #1
    ack_packets(
        1..=1,
        time_zero + Duration::from_millis(1200),
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    // t=2-6: Send packets #3 - #7 (app data)
    for t in 2..=6 {
        manager.on_packet_sent(
            space.new_packet_number(VarInt::from_u8(t + 1)),
            outcome,
            time_zero + Duration::from_secs(t.into()),
            ecn,
            transmission::Mode::Normal,
            None,
            &mut context,
            &mut publisher,
        );
    }

    // Skip packet #8, which ends one persistent congestion period.

    // t=8: Send packet #9 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(9)),
        outcome,
        time_zero + Duration::from_secs(8),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=20: Send packet #10 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(10)),
        outcome,
        time_zero + Duration::from_secs(20),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=30: Send packet #11 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(11)),
        outcome,
        time_zero + Duration::from_secs(30),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=30.2: Recv acknowledgement of #11
    ack_packets(
        11..=11,
        time_zero + Duration::from_millis(30200),
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    // Packets 2 though 7 and 9-10 should be lost
    assert_eq!(8, context.on_packet_loss_count);

    // The largest contiguous period of lost packets is #9 (sent at t8) to #10 (sent at t20)
    assert!(
        context
            .path()
            .rtt_estimator
            .persistent_congestion_threshold()
            < Duration::from_secs(12)
    );
    assert_eq!(
        Some(true),
        context.path().congestion_controller.persistent_congestion
    );
    assert_eq!(2, context.path().congestion_controller.loss_bursts);
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.2
//= type=test
//# The persistent congestion period SHOULD NOT start until there is at
//# least one RTT sample.
#[test]
fn persistent_congestion_period_does_not_start_until_rtt_sample() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let time_zero = time::now() + Duration::from_secs(10);
    let mut publisher = Publisher::snapshot();

    let outcome = transmission::Outcome {
        ack_elicitation: AckElicitation::Eliciting,
        is_congestion_controlled: true,
        bytes_sent: 1,
        bytes_progressed: 0,
    };

    // t=0: Send packet #1 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        outcome,
        time_zero,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=10: Send packet #2 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(2)),
        outcome,
        time_zero + Duration::from_secs(10),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=20: Send packet #3 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(3)),
        outcome,
        time_zero + Duration::from_secs(20),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=20.1: Recv acknowledgement of #3. The first RTT sample is collected
    //         now, at t=20.1
    ack_packets(
        3..=3,
        time_zero + Duration::from_millis(20_100),
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    // There is no persistent congestion, because the lost packets were all
    // sent prior to the first RTT sample
    assert_eq!(context.path().congestion_controller.on_packets_lost, 2);
    assert_eq!(
        context.path().congestion_controller.persistent_congestion,
        Some(false)
    );
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.6.2
//= type=test
//# These two packets MUST be ack-eliciting, since a receiver is required
//# to acknowledge only ack-eliciting packets within its maximum
//# acknowledgment delay; see Section 13.2 of [QUIC-TRANSPORT].
#[test]
fn persistent_congestion_not_ack_eliciting() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();

    let time_zero = time::now() + Duration::from_secs(10);
    context.path_mut().rtt_estimator.update_rtt(
        Duration::from_millis(10),
        Duration::from_millis(700),
        time::now(),
        true,
        space,
    );

    let mut outcome = transmission::Outcome {
        ack_elicitation: AckElicitation::NonEliciting,
        is_congestion_controlled: true,
        bytes_sent: 1,
        bytes_progressed: 0,
    };

    // t=0: Send packet #1 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        outcome,
        time_zero,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // The first packet was not ack-eliciting, but subsequent ones are
    outcome.ack_elicitation = AckElicitation::Eliciting;

    // t=10: Send packet #2 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(2)),
        outcome,
        time_zero + Duration::from_secs(10),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=20: Send packet #3 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(3)),
        outcome,
        time_zero + Duration::from_secs(20),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=20.1: Recv acknowledgement of #3
    ack_packets(
        3..=3,
        time_zero + Duration::from_millis(20_100),
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    // There is no persistent congestion because the first packet in the potential
    // persistent congestion period was not ack-eliciting.
    assert_eq!(context.path().congestion_controller.on_packets_lost, 2);
    assert_eq!(
        context.path().congestion_controller.persistent_congestion,
        Some(false)
    );
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-14.4
//= type=test
//# Loss of a QUIC packet that is carried in a PMTU probe is therefore not a
//# reliable indication of congestion and SHOULD NOT trigger a congestion
//# control reaction; see Item 7 in Section 3 of [DPLPMTUD].
#[test]
fn persistent_congestion_mtu_probe() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();

    let time_zero = time::now() + Duration::from_secs(10);
    context.path_mut().rtt_estimator.update_rtt(
        Duration::from_millis(10),
        Duration::from_millis(700),
        time::now(),
        true,
        space,
    );

    let outcome = transmission::Outcome {
        ack_elicitation: AckElicitation::Eliciting,
        is_congestion_controlled: true,
        bytes_sent: 1,
        bytes_progressed: 0,
    };

    // t=0: Send packet #1 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        outcome,
        time_zero,
        ecn,
        transmission::Mode::MtuProbing,
        None,
        &mut context,
        &mut publisher,
    );

    // t=10: Send packet #2 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(2)),
        outcome,
        time_zero + Duration::from_secs(10),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=20: Send packet #3 (app data)
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(3)),
        outcome,
        time_zero + Duration::from_secs(20),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // t=20.1: Recv acknowledgement of #3
    ack_packets(
        3..=3,
        time_zero + Duration::from_millis(20_100),
        &mut context,
        &mut manager,
        None,
        &mut publisher,
    );

    // There is no persistent congestion because the first packet in the potential
    // persistent congestion period was an MTU probe and the remaining packets are
    // not a long enough period to be considered persistent congestion.
    assert_eq!(context.path().congestion_controller.on_packets_lost, 1);
    assert_eq!(
        context.path().congestion_controller.persistent_congestion,
        Some(false)
    );
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
//= type=test
#[test]
fn update_pto_timer() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let now = time::now() + Duration::from_secs(10);
    let is_handshake_confirmed = true;
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();

    context.path_mut().rtt_estimator.update_rtt(
        Duration::from_millis(0),
        Duration::from_millis(500),
        now,
        true,
        space,
    );
    context.path_mut().rtt_estimator.update_rtt(
        Duration::from_millis(0),
        Duration::from_millis(1000),
        now,
        true,
        space,
    );
    // The path will be at the anti-amplification limit
    let amplification_outcome = context.path_mut().on_bytes_received(1200);
    assert!(amplification_outcome.is_active_path_unblocked());
    context.path_mut().on_bytes_transmitted((1200 * 3) + 1);
    // Arm the PTO so we can verify it is cancelled
    manager.pto.update(now, Duration::from_secs(10));
    manager.pto_update_pending = true;
    manager.update_pto_timer(context.path(), now, is_handshake_confirmed);

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.2.1
    //= type=test
    //# If no additional data can be sent, the server's PTO timer MUST NOT be
    //# armed until datagrams have been received from the client, because
    //# packets sent on PTO count against the anti-amplification limit.
    assert!(!manager.pto.is_armed());
    assert!(!manager.pto_update_pending);

    // Arm the PTO so we can verify it is cancelled
    manager.pto.update(now, Duration::from_secs(10));
    manager.pto_update_pending = true;
    // Validate the path so it is not at the anti-amplification limit
    //
    // simulate receiving a handshake packet to force path validation
    context.path_mut().on_handshake_packet();
    context.path_mut().on_peer_validated();
    manager.update_pto_timer(context.path(), now, is_handshake_confirmed);

    // Since the path is peer validated and sent packets is empty, PTO is cancelled
    assert!(!manager.pto.is_armed());
    assert!(!manager.pto_update_pending);

    // Reset the path back to not peer validated
    let path_id = unsafe { path::Id::new(0) };
    let mut rtt_estimator = RttEstimator::default();
    rtt_estimator.on_max_ack_delay(Duration::from_millis(10).try_into().unwrap());
    context.path_manager[path_id] = Path::new(
        Default::default(),
        connection::PeerId::TEST_ID,
        connection::LocalId::TEST_ID,
        rtt_estimator,
        MockCongestionController::default(),
        false,
        mtu::Config::default(),
    );
    context.path_manager.activate_path_for_test(path_id);
    // simulate receiving a handshake packet to force path validation
    context.path_mut().on_handshake_packet();
    context.path_mut().pto_backoff = 2;
    manager.pto_update_pending = true;
    let is_handshake_confirmed = false;
    manager.update_pto_timer(context.path(), now, is_handshake_confirmed);

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
    //= type=test
    //# An endpoint MUST NOT set its PTO timer for the Application Data
    //# packet number space until the handshake is confirmed.
    assert!(!manager.pto.is_armed());
    assert!(!manager.pto_update_pending);

    // Set is handshake confirmed back to true
    let is_handshake_confirmed = true;
    manager.pto_update_pending = true;
    manager.update_pto_timer(context.path(), now, is_handshake_confirmed);

    // Now the PTO is armed
    assert!(manager.pto.is_armed());
    assert!(!manager.pto_update_pending);

    // Send a packet to validate behavior when sent_packets is not empty
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            bytes_progressed: 0,
        },
        now,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    let expected_pto_base_timestamp = now - Duration::from_secs(5);
    manager.time_of_last_ack_eliciting_packet = Some(expected_pto_base_timestamp);
    // This will update the smoother_rtt to 2000, and rtt_var to 1000
    context.path_mut().rtt_estimator.update_rtt(
        Duration::from_millis(0),
        Duration::from_millis(2000),
        now,
        true,
        space,
    );
    manager.pto_update_pending = true;
    manager.update_pto_timer(context.path(), now, is_handshake_confirmed);

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
    //# When an ack-eliciting packet is transmitted, the sender schedules a
    //# timer for the PTO period as follows:
    //#
    //# PTO = smoothed_rtt + max(4*rttvar, kGranularity) + max_ack_delay
    // Including the pto backoff (2) =:
    // PTO = (2000 + max(4*1000, 1) + 10) * 2 = 12020
    assert!(manager.pto.is_armed());
    assert_eq!(
        manager.pto.next_expiration().unwrap(),
        expected_pto_base_timestamp + Duration::from_millis(12020)
    );
    assert!(!manager.pto_update_pending);
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.2.1
//= type=test
//# That is,
//# the client MUST set the PTO timer if the client has not received an
//# acknowledgment for any of its Handshake packets and the handshake is
//# not confirmed (see Section 4.1.2 of [QUIC-TLS]), even if there are no
//# packets in flight.
#[test]
fn pto_armed_if_handshake_not_confirmed() {
    let space = PacketNumberSpace::Handshake;
    let mut manager = ServerManager::new(space);
    let now = time::now() + Duration::from_secs(10);
    let is_handshake_confirmed = false;
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let path_id = unsafe { path::Id::new(0) };
    path_manager[path_id] = Path::new(
        Default::default(),
        connection::PeerId::TEST_ID,
        connection::LocalId::TEST_ID,
        RttEstimator::new(Duration::from_millis(10)),
        Default::default(),
        false,
        mtu::Config::default(),
    );
    path_manager.activate_path_for_test(path_id);

    // simulate receiving a handshake packet to force path validation
    path_manager[path_id].on_handshake_packet();

    manager.update_pto_timer(&path_manager[path_id], now, is_handshake_confirmed);

    assert!(manager.pto.is_armed());
}
//= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
//= type=test
//# The PTO period MUST be at least kGranularity, to avoid the timer
//# expiring immediately.
#[test]
fn pto_must_be_at_least_k_granularity() {
    let space = PacketNumberSpace::Handshake;
    let mut manager = ServerManager::new(space);
    let now = time::now();

    let mut path = Path::new(
        Default::default(),
        connection::PeerId::TEST_ID,
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
    );

    // Update RTT with the smallest possible sample
    path.rtt_estimator.update_rtt(
        Duration::from_millis(0),
        Duration::from_nanos(1),
        now,
        true,
        space,
    );

    manager
        .pto
        .update(now, path.rtt_estimator.pto_period(path.pto_backoff, space));

    assert!(manager.pto.is_armed());
    assert!(manager.pto.next_expiration().unwrap() >= now + K_GRANULARITY);
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
//= type=test
#[test]
fn on_timeout() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let now = time::now() + Duration::from_secs(10);
    manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(10)));
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();
    let random = &mut random::testing::Generator::default();

    // Remove amplification limits
    context.path_mut().on_handshake_packet();

    let mut expected_pto_backoff = context.path().pto_backoff;

    // Loss timer is armed but not expired yet, nothing happens
    manager.loss_timer.set(now + Duration::from_secs(10));
    manager.on_timeout(now, random, u32::MAX, &mut context, &mut publisher);
    assert_eq!(context.on_packet_loss_count, 0);
    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
    //= type=test
    //# The PTO timer MUST NOT be set if a timer is set for time threshold
    //# loss detection; see Section 6.1.2.
    assert!(!manager.pto.is_armed());
    assert_eq!(expected_pto_backoff, context.path().pto_backoff);

    // Send a packet that will be considered lost
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            bytes_progressed: 0,
        },
        now - Duration::from_secs(5),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    manager.on_transmit_burst_complete(context.path(), now, true);

    // Loss timer is armed and expired, on_packet_loss is called
    manager.loss_timer.set(now - Duration::from_secs(1));
    manager.on_timeout(now, random, u32::MAX, &mut context, &mut publisher);
    assert_eq!(context.on_packet_loss_count, 1);
    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
    //= type=test
    //# The PTO timer MUST NOT be set if a timer is set for time threshold
    //# loss detection; see Section 6.1.2.
    assert!(!manager.pto.is_armed());
    assert_eq!(expected_pto_backoff, context.path().pto_backoff);

    // Loss timer is not armed, pto timer is not armed
    manager.loss_timer.cancel();
    manager.on_timeout(now, random, u32::MAX, &mut context, &mut publisher);
    assert_eq!(expected_pto_backoff, context.path().pto_backoff);

    // Loss timer is not armed, pto timer is armed but not expired
    manager.loss_timer.cancel();
    manager.pto.update(now, Duration::from_secs(5));
    manager.on_timeout(now, random, u32::MAX, &mut context, &mut publisher);
    assert_eq!(expected_pto_backoff, context.path().pto_backoff);

    // Loss timer is not armed, pto timer is expired without bytes in flight
    expected_pto_backoff *= 2;
    manager
        .pto
        .update(now - Duration::from_secs(5), Duration::ZERO);
    manager.on_timeout(now, random, u32::MAX, &mut context, &mut publisher);
    assert_eq!(expected_pto_backoff, context.path().pto_backoff);
    assert_eq!(manager.pto.transmissions(), 1);

    // Loss timer is not armed, pto timer is expired with bytes in flight

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.1
    //= type=test
    //# When a PTO timer expires, the PTO backoff MUST be increased,
    //# resulting in the PTO period being set to twice its current value.
    expected_pto_backoff *= 2;
    manager.sent_packets.insert(
        space.new_packet_number(VarInt::from_u8(1)),
        SentPacketInfo::new(
            true,
            1,
            now,
            AckElicitation::Eliciting,
            unsafe { path::Id::new(0) },
            ecn,
            transmission::Mode::Normal,
            Default::default(),
        ),
    );
    manager
        .pto
        .update(now - Duration::from_secs(5), Duration::ZERO);
    manager.on_timeout(now, random, u32::MAX, &mut context, &mut publisher);
    assert_eq!(expected_pto_backoff, context.path().pto_backoff);
    assert!(manager.pto.is_armed());

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
    //= type=test
    //# When a PTO timer expires, a sender MUST send at least one ack-
    //# eliciting packet in the packet number space as a probe.

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2.4
    //= type=test
    //# An endpoint
    //# MAY send up to two full-sized datagrams containing ack-eliciting
    //# packets to avoid an expensive consecutive PTO expiration due to a
    //# single lost datagram or to transmit data from multiple packet number
    //# spaces.
    assert_eq!(manager.pto.transmissions(), 2);

    //= https://www.rfc-editor.org/rfc/rfc9002#section-6.2
    //= type=test
    //# A PTO timer expiration event does not indicate packet loss and MUST
    //# NOT cause prior unacknowledged packets to be marked as lost.
    assert!(manager
        .sent_packets
        .get(space.new_packet_number(VarInt::from_u8(1)))
        .is_some());
}

// Test that the PTO timer is re-armed after the loss timer has expired
#[test]
fn on_timeout_packet_lost() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let now = time::now() + Duration::from_secs(10);
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();
    let random = &mut random::testing::Generator::default();

    // Remove amplification limits
    context.path_mut().on_handshake_packet();

    manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(2)));

    // Send a packet that will be considered lost
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            bytes_progressed: 0,
        },
        now - Duration::from_secs(5),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    // Send a tail packet
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(3)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            bytes_progressed: 0,
        },
        now - Duration::from_secs(5),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    manager.on_transmit_burst_complete(context.path(), now - Duration::from_secs(5), true);

    assert!(manager.pto.is_armed());

    // Loss timer is armed and expired, on_packet_loss is called
    manager.loss_timer.set(now - Duration::from_secs(5));
    manager.on_timeout(
        now - Duration::from_secs(5),
        random,
        u32::MAX,
        &mut context,
        &mut publisher,
    );

    // It was too early to declare Packet 1 as lost, so the loss timer is armed
    assert_eq!(0, context.on_packet_loss_count);
    assert!(manager.loss_timer.is_armed());
    assert!(!manager.pto.is_armed());

    manager.on_timeout(now, random, u32::MAX, &mut context, &mut publisher);

    // Now Packet 1 is declared lost. Packet 2 is still outstanding though,
    // so confirm the PTO timer is armed
    assert_eq!(1, context.on_packet_loss_count);
    assert!(!manager.loss_timer.is_armed());
    assert!(manager.pto.is_armed());
}

// Test that multiple PTO timeouts only doubles the PTO backoff once
#[test]
fn max_pto_backoff() {
    let mut initial_space = ServerManager::new(PacketNumberSpace::Initial);
    let mut handshake_space = ServerManager::new(PacketNumberSpace::Handshake);
    let mut application_space = ServerManager::new(PacketNumberSpace::ApplicationData);
    let now = time::now();
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();
    let random = &mut random::testing::Generator::default();

    initial_space.pto.update(now, Duration::ZERO);
    handshake_space.pto.update(now, Duration::ZERO);
    application_space.pto.update(now, Duration::ZERO);

    assert_eq!(INITIAL_PTO_BACKOFF, context.path().pto_backoff);
    let max_pto_backoff = INITIAL_PTO_BACKOFF * 2;

    initial_space.on_timeout(now, random, max_pto_backoff, &mut context, &mut publisher);
    handshake_space.on_timeout(now, random, max_pto_backoff, &mut context, &mut publisher);
    application_space.on_timeout(now, random, max_pto_backoff, &mut context, &mut publisher);

    assert_eq!(max_pto_backoff, context.path().pto_backoff);
}

// Test that calling `on_timeout` and `on_transmit_burst_complete` on a new client recovery::Manager does nothing
#[test]
fn new_client_space() {
    let space = PacketNumberSpace::Handshake;
    let mut manager = ClientManager::new(space);
    let now = time::now() + Duration::from_secs(10);
    manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(10)));
    let mut path_manager =
        helper_generate_client_path_manager(Duration::from_millis(10), Default::default());
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();
    let random = &mut random::testing::Generator::default();

    let expected_pto_backoff = context.path().pto_backoff;

    // No timers are armed yet, nothing happens
    assert_eq!(0, manager.armed_timer_count());
    manager.on_timeout(now, random, u32::MAX, &mut context, &mut publisher);
    assert_eq!(context.on_packet_loss_count, 0);
    assert!(!manager.loss_timer.is_armed());
    assert!(!manager.pto.is_armed());
    assert_eq!(expected_pto_backoff, context.path().pto_backoff);

    // No PTO update was pending, nothing happens
    assert!(!manager.pto_update_pending);
    manager.on_transmit_burst_complete(context.path(), now, false);
    assert!(!manager.pto_update_pending);
}

#[test]
fn timers() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let loss_time = time::now() + Duration::from_secs(5);
    let pto_time = time::now() + Duration::from_secs(10);

    // No timer is set
    assert_eq!(manager.armed_timer_count(), 0);

    // Loss timer is armed
    manager.loss_timer.set(loss_time);
    assert_eq!(manager.armed_timer_count(), 1);
    assert_eq!(manager.next_expiration(), Some(loss_time));

    // PTO timer is armed
    manager.loss_timer.cancel();
    manager.pto.update(pto_time, Duration::ZERO);
    assert_eq!(manager.armed_timer_count(), 1);
    assert_eq!(manager.next_expiration(), Some(pto_time));

    // Both timers are armed, only loss time is returned
    manager.loss_timer.set(loss_time);
    manager.pto.update(pto_time, Duration::ZERO);
    assert_eq!(manager.armed_timer_count(), 1);
    assert_eq!(manager.next_expiration(), Some(loss_time));
}

// Helper function that will call on_ack_frame with the given packet numbers
fn helper_ack_packets_on_path(
    range: RangeInclusive<u64>,
    ack_receive_time: Timestamp,
    context: &mut MockContext<ServerConfig>,
    manager: &mut ServerManager,
    remote_address: RemoteAddress,
    ecn_counts: Option<EcnCounts>,
    publisher: &mut Publisher,
) {
    let (id, _) = context
        .path_manager
        .path(&remote_address)
        .expect("missing path");
    context.path_id = id;
    let random = &mut random::testing::Generator::default();

    let acked_packets = PacketNumberRange::new(
        manager
            .space
            .new_packet_number(VarInt::new(*range.start()).unwrap()),
        manager
            .space
            .new_packet_number(VarInt::new(*range.end()).unwrap()),
    );

    let datagram = DatagramInfo {
        timestamp: ack_receive_time,
        payload_len: 0,
        ecn: Default::default(),
        destination_connection_id: connection::LocalId::TEST_ID,
        destination_connection_id_classification: connection::id::Classification::Local,
        source_connection_id: None,
    };

    let mut ack_range = ack::Ranges::new(acked_packets.count());

    for acked_packet in acked_packets {
        assert!(ack_range.insert_packet_number(acked_packet).is_ok());
    }

    let frame = frame::Ack {
        ack_delay: VarInt::from_u8(10),
        ack_ranges: (&ack_range),
        ecn_counts,
    };

    let result = manager.on_ack_frame(
        datagram.timestamp,
        frame,
        acked_packets.start(),
        random,
        context,
        publisher,
    );

    if context.fail_validation {
        assert!(result.is_err());
    } else {
        assert!(result.is_ok());
        for packet in acked_packets {
            assert!(manager.sent_packets.get(packet).is_none());
        }
    }
}

// Helper function that will call on_ack_frame with the given packet numbers
fn ack_packets(
    range: RangeInclusive<u64>,
    ack_receive_time: Timestamp,
    context: &mut MockContext<ServerConfig>,
    manager: &mut ServerManager,
    ecn_counts: Option<EcnCounts>,
    publisher: &mut Publisher,
) {
    let addr = context.path().handle;
    helper_ack_packets_on_path(
        range,
        ack_receive_time,
        context,
        manager,
        addr,
        ecn_counts,
        publisher,
    )
}

//= https://www.rfc-editor.org/rfc/rfc9002#section-7.5
//= type=test
//# A sender MUST however count these packets as being additionally in
//# flight, since these packets add network load without establishing
//# packet loss.
#[test]
fn probe_packets_count_towards_bytes_in_flight() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let ecn = ExplicitCongestionNotification::default();

    manager.pto.update(time::now(), Duration::ZERO);
    let _ = manager.pto.on_timeout(true, time::now());

    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();
    let outcome = transmission::Outcome {
        ack_elicitation: AckElicitation::Eliciting,
        is_congestion_controlled: true,
        bytes_sent: 100,
        bytes_progressed: 0,
    };
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        outcome,
        time::now(),
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    assert_eq!(context.path().congestion_controller.bytes_in_flight, 100);
}

#[test]
fn packet_declared_lost_less_than_1_ms_from_loss_threshold() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();
    let sent_time = time::now() + Duration::from_secs(10);
    let outcome = transmission::Outcome {
        ack_elicitation: AckElicitation::Eliciting,
        is_congestion_controlled: true,
        bytes_sent: 100,
        bytes_progressed: 0,
    };
    let random = &mut random::testing::Generator::default();
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        outcome,
        sent_time,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );
    manager.largest_acked_packet = Some(space.new_packet_number(VarInt::from_u8(2)));

    let loss_time_threshold = context.path().rtt_estimator.loss_time_threshold();

    manager.detect_and_remove_lost_packets(
        sent_time + loss_time_threshold - Duration::from_micros(999),
        random,
        &mut context,
        &mut publisher,
    );

    assert_eq!(1, context.on_packet_loss_count);
}

#[test]
fn on_transmit_burst_complete() {
    let space = PacketNumberSpace::ApplicationData;
    let mut manager = ServerManager::new(space);
    let now = time::now() + Duration::from_secs(10);
    let is_handshake_confirmed = true;
    let mut path_manager = helper_generate_path_manager(Duration::from_millis(10));
    let ecn = ExplicitCongestionNotification::default();
    let mut context = MockContext::new(&mut path_manager);
    let mut publisher = Publisher::snapshot();

    // Send an ack-eliciting packet to trigger a PTO timer update
    manager.on_packet_sent(
        space.new_packet_number(VarInt::from_u8(1)),
        transmission::Outcome {
            ack_elicitation: AckElicitation::Eliciting,
            is_congestion_controlled: true,
            bytes_sent: 1,
            bytes_progressed: 0,
        },
        now,
        ecn,
        transmission::Mode::Normal,
        None,
        &mut context,
        &mut publisher,
    );

    // Validate the path so the PTO timer can be set
    context.path_mut().on_handshake_packet();
    context.path_mut().on_peer_validated();

    assert!(manager.pto_update_pending);
    manager.on_transmit_burst_complete(path_manager.active_path(), now, is_handshake_confirmed);
    assert!(manager.pto.is_armed());
    assert!(!manager.pto_update_pending);

    // Cancel the PTO timer to validate it isn't re-armed when not needed
    manager.pto.cancel();
    manager.sent_packets.clear();
    manager.on_transmit_burst_complete(path_manager.active_path(), now, is_handshake_confirmed);
    assert!(!manager.pto.is_armed());
}

fn helper_generate_multi_path_manager(
    space: PacketNumberSpace,
    publisher: &mut Publisher,
) -> (
    RemoteAddress,
    path::Id,
    RemoteAddress,
    path::Id,
    ServerManager,
    path::Manager<ServerConfig>,
) {
    let manager = ServerManager::new(space);
    let clock = NoopClock {};

    let first_addr: SocketAddr = "127.0.0.1:80".parse().unwrap();
    let first_addr = SocketAddress::from(first_addr);
    let first_addr = RemoteAddress::from(first_addr);
    let second_addr: SocketAddr = "127.0.0.2:80".parse().unwrap();
    let second_addr = SocketAddress::from(second_addr);
    let second_addr = RemoteAddress::from(second_addr);

    let first_path_id = unsafe { path::Id::new(0) };
    let second_path_id = unsafe { path::Id::new(1) };

    // confirm we have one path
    let mut path_manager =
        helper_generate_path_manager_with_first_addr(Duration::from_millis(100), first_addr);
    {
        assert!(path_manager.path(&first_addr).is_some());
        assert!(path_manager.path(&second_addr).is_none());
    }

    // insert and confirm we have two paths
    {
        let datagram = DatagramInfo {
            timestamp: clock.get_time(),
            payload_len: 0,
            ecn: ExplicitCongestionNotification::default(),
            destination_connection_id: connection::LocalId::TEST_ID,
            destination_connection_id_classification: connection::id::Classification::Local,
            source_connection_id: None,
        };
        let _ = path_manager
            .on_datagram_received(
                &second_addr,
                &datagram,
                true,
                &mut Endpoint::default(),
                &mut migration::allow_all::Validator,
                mtu::Config::default(),
                DEFAULT_INITIAL_RTT,
                publisher,
            )
            .unwrap();

        assert!(path_manager.path(&first_addr).is_some());
        assert!(path_manager.path(&second_addr).is_some());
        assert_eq!(path_manager.active_path_id(), first_path_id);
    }

    // start out with both paths validate and not AmplificationLimited
    //
    // simulate receiving a handshake packet to force path validation
    path_manager
        .path_mut(&first_addr)
        .unwrap()
        .1
        .on_handshake_packet();
    path_manager
        .path_mut(&second_addr)
        .unwrap()
        .1
        .on_handshake_packet();
    let first_path = path_manager.path(&first_addr).unwrap().1;
    let second_path = path_manager.path(&second_addr).unwrap().1;
    assert!(!first_path.at_amplification_limit());
    assert!(!second_path.at_amplification_limit());
    assert!(first_path.is_peer_validated());
    assert!(second_path.is_peer_validated());

    (
        first_addr,
        first_path_id,
        second_addr,
        second_path_id,
        manager,
        path_manager,
    )
}

fn helper_generate_path_manager(max_ack_delay: Duration) -> path::Manager<ServerConfig> {
    helper_generate_path_manager_with_first_addr(max_ack_delay, Default::default())
}

fn helper_generate_path_manager_with_first_addr(
    max_ack_delay: Duration,
    first_addr: RemoteAddress,
) -> path::Manager<ServerConfig> {
    let mut random_generator = random::testing::Generator(123);

    let registry = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
        .create_server_peer_id_registry(
            InternalConnectionIdGenerator::new().generate_id(),
            connection::PeerId::TEST_ID,
            true,
        );
    let mut rtt_estimator = RttEstimator::default();
    rtt_estimator.on_max_ack_delay(max_ack_delay.try_into().unwrap());
    let path = Path::new(
        first_addr,
        connection::PeerId::TEST_ID,
        connection::LocalId::TEST_ID,
        rtt_estimator,
        MockCongestionController::new(first_addr),
        true,
        mtu::Config::default(),
    );

    path::Manager::new(path, registry)
}

fn helper_generate_client_path_manager(
    max_ack_delay: Duration,
    first_addr: RemoteAddress,
) -> path::Manager<ClientConfig> {
    let mut random_generator = random::testing::Generator(123);

    let registry = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Client)
        .create_client_peer_id_registry(InternalConnectionIdGenerator::new().generate_id(), true);
    let mut rtt_estimator = RttEstimator::default();
    rtt_estimator.on_max_ack_delay(max_ack_delay.try_into().unwrap());
    let path = super::Path::new(
        first_addr,
        connection::PeerId::TEST_ID,
        connection::LocalId::TEST_ID,
        rtt_estimator,
        MockCongestionController::new(first_addr),
        false,
        mtu::Config::default(),
    );

    path::Manager::new(path, registry)
}

struct MockContext<'a, Config: endpoint::Config> {
    validate_packet_ack_count: u8,
    on_new_packet_ack_count: u8,
    on_packet_ack_count: u8,
    on_packet_loss_count: u8,
    on_rtt_update_count: u8,
    path_id: path::Id,
    lost_packets: HashSet<PacketNumber>,
    path_manager: &'a mut path::Manager<Config>,
    fail_validation: bool,
}

impl<'a, Config: endpoint::Config> MockContext<'a, Config> {
    pub fn new(path_manager: &'a mut path::Manager<Config>) -> Self {
        Self {
            validate_packet_ack_count: 0,
            on_new_packet_ack_count: 0,
            on_packet_ack_count: 0,
            on_packet_loss_count: 0,
            on_rtt_update_count: 0,
            path_id: path_manager.active_path_id(),
            lost_packets: HashSet::default(),
            path_manager,
            fail_validation: false,
        }
    }

    pub fn set_path_id(&mut self, path_id: path::Id) {
        self.path_id = path_id;
    }
}

impl<'a, Config: endpoint::Config> recovery::Context<Config> for MockContext<'a, Config> {
    const ENDPOINT_TYPE: endpoint::Type = Config::ENDPOINT_TYPE;

    fn is_handshake_confirmed(&self) -> bool {
        true
    }

    fn active_path(&self) -> &path::Path<Config> {
        self.path_manager.active_path()
    }

    fn active_path_mut(&mut self) -> &mut path::Path<Config> {
        self.path_manager.active_path_mut()
    }

    fn path(&self) -> &super::Path<Config> {
        &self.path_manager[self.path_id]
    }

    fn path_mut(&mut self) -> &mut super::Path<Config> {
        &mut self.path_manager[self.path_id]
    }

    fn path_by_id(&self, path_id: path::Id) -> &super::Path<Config> {
        &self.path_manager[path_id]
    }

    fn path_mut_by_id(&mut self, path_id: path::Id) -> &mut super::Path<Config> {
        &mut self.path_manager[path_id]
    }

    fn path_id(&self) -> path::Id {
        self.path_id
    }

    fn validate_packet_ack(
        &mut self,
        _timestamp: Timestamp,
        _packet_number_range: &PacketNumberRange,
        _lowest_tracking_packet_number: PacketNumber,
    ) -> Result<(), transport::Error> {
        self.validate_packet_ack_count += 1;

        if self.fail_validation {
            Err(transport::Error::PROTOCOL_VIOLATION)
        } else {
            Ok(())
        }
    }

    fn on_new_packet_ack<Pub: event::ConnectionPublisher>(
        &mut self,
        _packet_number_range: &PacketNumberRange,
        _publisher: &mut Pub,
    ) {
        self.on_new_packet_ack_count += 1;
    }

    fn on_packet_ack(&mut self, _timestamp: Timestamp, _packet_number_range: &PacketNumberRange) {
        self.on_packet_ack_count += 1;
    }

    fn on_packet_loss<Pub: event::ConnectionPublisher>(
        &mut self,
        packet_number_range: &PacketNumberRange,
        _publisher: &mut Pub,
    ) {
        self.on_packet_loss_count += 1;
        self.lost_packets.insert(packet_number_range.start());
    }

    fn on_rtt_update(&mut self, _now: Timestamp) {
        self.on_rtt_update_count += 1;
    }
}
