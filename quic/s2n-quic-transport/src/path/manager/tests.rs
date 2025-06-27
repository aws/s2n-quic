// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    connection::{ConnectionIdMapper, InternalConnectionIdGenerator},
    contexts::testing::{MockWriteContext, OutgoingFrameBuffer},
    endpoint::{
        self,
        testing::{Client, Server},
    },
    path,
};
use core::time::Duration;
use s2n_quic_core::{
    connection::limits::ANTI_AMPLIFICATION_MULTIPLIER,
    event::testing::Publisher,
    inet::{DatagramInfo, ExplicitCongestionNotification, SocketAddress},
    path::{migration, RemoteAddress},
    random::{self, Generator},
    recovery::RttEstimator,
    stateless_reset::token::testing::*,
    time::{Clock, NoopClock},
};
use std::net::SocketAddr;

type ServerManager = super::Manager<Server>;
type ServerPath = super::Path<Server>;
type ClientManager = super::Manager<Client>;
type ClientPath = super::Path<Client>;

// Helper function to easily create a PathManager as a Server
fn manager_server(first_path: ServerPath) -> ServerManager {
    let mut random_generator = random::testing::Generator(123);
    let peer_id_registry = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
        .create_server_peer_id_registry(
            InternalConnectionIdGenerator::new().generate_id(),
            first_path.peer_connection_id,
            true,
        );
    ServerManager::new(first_path, peer_id_registry)
}

// Helper function to easily create a PathManager as a Client
fn manager_client(first_path: ClientPath) -> ClientManager {
    let mut random_generator = random::testing::Generator(123);
    let peer_id_registry = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Client)
        .create_client_peer_id_registry(InternalConnectionIdGenerator::new().generate_id(), true);
    ClientManager::new(first_path, peer_id_registry)
}

#[test]
fn get_path_by_address_test() {
    let first_conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
    let first_local_conn_id = connection::LocalId::TEST_ID;
    let mtu_config = mtu::Config::default();
    let first_path = ServerPath::new(
        Default::default(),
        first_conn_id,
        first_local_conn_id,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu_config,
        ANTI_AMPLIFICATION_MULTIPLIER,
    );

    let second_conn_id = connection::PeerId::try_from_bytes(&[5, 4, 3, 2, 1]).unwrap();
    let second_path = ServerPath::new(
        Default::default(),
        second_conn_id,
        first_local_conn_id,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu_config,
        ANTI_AMPLIFICATION_MULTIPLIER,
    );

    let mut manager = manager_server(first_path.clone());
    manager.paths.push(second_path);
    assert_eq!(manager.paths.len(), 2);

    let (_id, matched_path) = manager.path(&RemoteAddress::default()).unwrap();
    assert_eq!(
        matched_path.peer_connection_id,
        first_path.peer_connection_id
    );
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-9.3.2
//= type=test
//# To protect the connection from failing due to such a spurious
//# migration, an endpoint MUST revert to using the last validated peer
//# address when validation of a new peer address fails.
#[test]
fn test_invalid_path_fallback() {
    let mut publisher = Publisher::snapshot();
    let first_conn_id = connection::PeerId::try_from_bytes(&[0, 1, 2, 3, 4, 5]).unwrap();
    let first_local_conn_id = connection::LocalId::TEST_ID;
    let mut first_path = ServerPath::new(
        Default::default(),
        first_conn_id,
        first_local_conn_id,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    // simulate receiving a handshake packet to force path validation
    first_path.on_handshake_packet();

    // Create a challenge that will expire in 100ms
    let now = NoopClock {}.get_time();
    let expiration = Duration::from_millis(1000);
    let challenge = challenge::Challenge::new(expiration, [0; 8]);
    let mut second_path = ServerPath::new(
        Default::default(),
        first_conn_id,
        first_local_conn_id,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    second_path.set_challenge(challenge);

    let mut manager = manager_server(first_path);
    manager.paths.push(second_path);
    assert_eq!(manager.last_known_active_validated_path, None);
    assert_eq!(manager.active, 0);
    assert!(manager.paths[0].is_validated());

    let amplification_outcome = manager
        .update_active_path(
            path_id(1),
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();
    assert!(amplification_outcome.is_unchanged());
    assert_eq!(manager.active, 1);
    assert_eq!(manager.last_known_active_validated_path, Some(0));

    // send challenge and arm abandon timer
    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut context = MockWriteContext::new(
        now,
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Client,
    );
    manager[path_id(1)].on_transmit(&mut context);

    // After a validation times out, the path should revert to the previous
    let amplification_outcome = manager
        .on_timeout(
            now + expiration + Duration::from_millis(100),
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();
    assert!(amplification_outcome.is_active_path_unblocked());
    assert_eq!(manager.active, 0);
    assert!(manager.last_known_active_validated_path.is_none());
}

#[test]
// a validated path should be assigned to last_known_active_validated_path
fn promote_validated_path_to_last_known_validated_path() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    assert!(!helper.manager.paths[helper.first_path_id.as_u8() as usize].is_validated());

    // Trigger:
    // simulate receiving a handshake packet to force path validation
    helper.manager.paths[helper.first_path_id.as_u8() as usize].on_handshake_packet();
    assert!(helper.manager.paths[helper.first_path_id.as_u8() as usize].is_validated());
    let amplification_outcome = helper
        .manager
        .update_active_path(
            helper.second_path_id,
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();

    // Expectation:
    assert!(amplification_outcome.is_unchanged());
    assert_eq!(helper.manager.last_known_active_validated_path, Some(1));
}

#[test]
// a NOT validated path should NOT be assigned to last_known_active_validated_path
fn dont_promote_non_validated_path_to_last_known_validated_path() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    assert!(!helper.manager.paths[helper.first_path_id.as_u8() as usize].is_validated());

    // Trigger:
    let amplification_outcome = helper
        .manager
        .update_active_path(
            helper.second_path_id,
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();

    // Expectation:
    assert!(amplification_outcome.is_unchanged());
    assert_eq!(helper.manager.last_known_active_validated_path, Some(0));
}

#[test]
// update path to the new active path
fn update_path_to_active_path() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    assert_eq!(helper.manager.active, helper.first_path_id.as_u8());

    // Trigger:
    let amplification_outcome = helper
        .manager
        .update_active_path(
            helper.second_path_id,
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();

    // Expectation:
    assert!(amplification_outcome.is_unchanged());
    assert_eq!(helper.manager.active, helper.second_path_id.as_u8());
}

#[test]
// Don't update path to the new active path if insufficient connection ids
fn dont_update_path_to_active_path_if_no_connection_id_available() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths_base(false, true, &mut publisher);
    assert_eq!(helper.manager.active, helper.first_path_id.as_u8());

    // Trigger:
    assert_eq!(
        helper.manager.update_active_path(
            helper.second_path_id,
            &mut random::testing::Generator(123),
            &mut publisher,
        ),
        Err(transport::Error::INTERNAL_ERROR)
    );

    // Expectation:
    assert_eq!(helper.manager.active, helper.first_path_id.as_u8());
}

#[test]
fn set_path_challenge_on_active_path_on_connection_migration() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    helper.manager[helper.zero_path_id].abandon_challenge(&mut publisher, 0);
    assert!(!helper.manager[helper.zero_path_id].is_challenge_pending());
    assert_eq!(
        helper.manager.last_known_active_validated_path.unwrap(),
        helper.zero_path_id.as_u8()
    );

    // Trigger:
    let amplification_outcome = helper
        .manager
        .update_active_path(
            helper.second_path_id,
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();

    // Expectation:
    assert!(amplification_outcome.is_unchanged());
    assert!(helper.manager[helper.first_path_id].is_challenge_pending());
}

#[test]
// A path should be validated if a PATH_RESPONSE contains data sent
// via PATH_CHALLENGE
//
// Setup:
// - create manager with 2 paths
// - first path is active
// - second path is pending challenge/validation
//
// Trigger 1:
// - call on_timeout just BEFORE challenge should expire
//
// Expectation 1:
// - verify second path is pending challenge
// - verify second path is NOT validated
//
// Trigger 2:
// - call on_path_response with expected data for second path
//
// Expectation 2:
// - verify second path is validated
fn validate_path_before_challenge_expiration() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    assert_eq!(helper.manager.active, helper.first_path_id.as_u8());

    // send challenge and arm abandon timer
    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut context = MockWriteContext::new(
        helper.now,
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Client,
    );
    helper.manager[helper.second_path_id].on_transmit(&mut context);
    assert!(helper.manager[helper.second_path_id].is_challenge_pending());

    // Trigger 1:
    // A response 100ms before the challenge is abandoned
    let amplification_outcome = helper
        .manager
        .on_timeout(
            helper.now + helper.challenge_expiration - Duration::from_millis(100),
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();

    // Expectation 1:
    assert!(amplification_outcome.is_unchanged());
    assert!(helper.manager[helper.second_path_id].is_challenge_pending(),);
    assert!(!helper.manager[helper.second_path_id].is_validated());

    // Trigger 2:
    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.2
    //= type=test
    //# This requirement MUST NOT be enforced by the endpoint that initiates
    //# path validation, as that would enable an attack on migration; see
    //# Section 9.3.3.

    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.3
    //= type=test
    //# A PATH_RESPONSE frame received on any network path validates the path
    //# on which the PATH_CHALLENGE was sent.
    //
    // The above requirements are satisfied because on_path_response is a path
    // agnostic function
    let frame = s2n_quic_core::frame::PathResponse {
        data: &helper.second_expected_data,
    };
    let amplification_outcome = helper.manager.on_path_response(&frame, &mut publisher);

    // Expectation 2:
    assert!(amplification_outcome.is_inactivate_path_unblocked());
    assert!(helper.manager[helper.second_path_id].is_validated());
}

#[test]
// A path should NOT be validated if the challenge has been abandoned
//
// Setup:
// - create manager with 2 paths
// - first path is active
// - second path is pending challenge/validation
//
// Trigger 1:
// - call on_timeout just AFTER challenge should expire
//
// Expectation 1:
// - verify second path is NOT pending challenge
// - verify second path is NOT validated
//
//
// Trigger 2:
// - call on_path_response with expected data for second path
//
// Expectation 2:
// - verify second path is NOT validated
fn dont_validate_path_if_path_challenge_is_abandoned() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    assert_eq!(helper.manager.active, helper.first_path_id.as_u8());

    // send challenge and arm abandon timer
    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut context = MockWriteContext::new(
        helper.now,
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Client,
    );
    helper.manager[helper.second_path_id].on_transmit(&mut context);
    assert!(helper.manager[helper.second_path_id].is_challenge_pending());

    // Trigger 1:
    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.4
    //= type=test
    //# Endpoints SHOULD abandon path validation based on a timer.
    // A response 100ms after the challenge should fail
    let amplification_outcome = helper
        .manager
        .on_timeout(
            helper.now + helper.challenge_expiration + Duration::from_millis(100),
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();

    // Expectation 1:
    assert!(amplification_outcome.is_unchanged());
    assert!(!helper.manager[helper.second_path_id].is_challenge_pending());
    assert!(!helper.manager[helper.second_path_id].is_validated());

    // Trigger 2:
    let frame = s2n_quic_core::frame::PathResponse {
        data: &helper.second_expected_data,
    };
    let amplification_outcome = helper.manager.on_path_response(&frame, &mut publisher);

    // Expectation 2:
    assert!(amplification_outcome.is_unchanged());
    assert!(!helper.manager[helper.second_path_id].is_validated());
}

#[test]
//= https://www.rfc-editor.org/rfc/rfc9000#section-9.3
//# If the recipient permits the migration, it MUST send subsequent
//# packets to the new peer address and MUST initiate path validation
//# (Section 8.2) to verify the peer's ownership of the address if
//# validation is not already underway.
fn initiate_path_challenge_if_new_path_is_not_validated() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    assert!(!helper.manager[helper.first_path_id].is_validated());
    assert!(helper.manager[helper.first_path_id].is_challenge_pending());

    assert!(!helper.manager[helper.second_path_id].is_validated());
    helper.manager[helper.second_path_id].abandon_challenge(&mut publisher, 0);
    assert!(!helper.manager[helper.second_path_id].is_challenge_pending());
    assert_eq!(helper.manager.active_path_id(), helper.first_path_id);

    // Trigger:
    let amplification_outcome = helper
        .manager
        .on_processed_packet(
            helper.second_path_id,
            None,
            path_validation::Probe::NonProbing,
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();

    // Expectation:
    assert!(amplification_outcome.is_unchanged());
    assert!(!helper.manager[helper.second_path_id].is_validated());
    assert_eq!(helper.manager.active_path_id(), helper.second_path_id);
    assert!(helper.manager[helper.second_path_id].is_challenge_pending());
}

#[test]
//= https://www.rfc-editor.org/rfc/rfc9000#section-9
//= type=test
//# When an endpoint has no validated path on which to send packets, it
//# MAY discard connection state.

//= https://www.rfc-editor.org/rfc/rfc9000#section-9.3.2
//= type=test
//# If an endpoint has no state about the last validated peer address, it
//# MUST close the connection silently by discarding all connection
//# state.

//= https://www.rfc-editor.org/rfc/rfc9000#section-10
//= type=test
//# An endpoint MAY discard connection state if it does not have a
//# validated path on which it can send packets; see Section 8.2
//
// If there is no last_known_active_validated_path after a on_timeout then return a
// NoValidPath error
fn silently_return_when_there_is_no_valid_path() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let now = NoopClock {}.get_time();
    let expiration = Duration::from_millis(1000);
    let challenge = challenge::Challenge::new(expiration, [0; 8]);
    let mut first_path = ServerPath::new(
        Default::default(),
        connection::PeerId::try_from_bytes(&[1]).unwrap(),
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    first_path.set_challenge(challenge);
    let mut manager = manager_server(first_path);
    let first_path_id = path_id(0);

    assert!(!manager[first_path_id].is_validated());
    assert!(manager[first_path_id].is_challenge_pending());
    assert_eq!(manager.last_known_active_validated_path, None);

    // Trigger:
    // send challenge and arm abandon timer
    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut context = MockWriteContext::new(
        now,
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Client,
    );
    manager[first_path_id].on_transmit(&mut context);
    let res = manager.on_timeout(
        now + expiration + Duration::from_millis(100),
        &mut random::testing::Generator(123),
        &mut publisher,
    );

    // Expectation:
    assert!(!manager[first_path_id].is_challenge_pending());
    assert!(matches!(
        res.unwrap_err(),
        connection::Error::NoValidPath { .. }
    ));
}

#[test]
//= https://www.rfc-editor.org/rfc/rfc9000#section-9.3
//= type=test
//# After changing the address to which it sends non-probing packets, an
//# endpoint can abandon any path validation for other addresses.
//
// A non-probing (path validation probing) packet will cause the path to become an active
// path but the path is still not validated.
fn dont_abandon_path_challenge_if_new_path_is_not_validated() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    assert!(!helper.manager[helper.first_path_id].is_validated());
    assert!(helper.manager[helper.first_path_id].is_challenge_pending());

    assert!(!helper.manager[helper.second_path_id].is_validated());
    assert!(helper.manager[helper.second_path_id].is_challenge_pending());
    assert_eq!(helper.manager.active_path_id(), helper.first_path_id);

    // Trigger:
    let amplification_outcome = helper
        .manager
        .on_processed_packet(
            helper.second_path_id,
            None,
            path_validation::Probe::NonProbing,
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();

    // Expectation:
    assert!(amplification_outcome.is_unchanged());
    assert!(!helper.manager[helper.second_path_id].is_validated());
    assert_eq!(helper.manager.active_path_id(), helper.second_path_id);
    assert!(helper.manager[helper.second_path_id].is_challenge_pending());
}

#[test]
fn abandon_path_challenges_if_new_path_is_validated() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    assert!(helper.manager[helper.first_path_id].is_challenge_pending());
    assert!(helper.manager[helper.second_path_id].is_challenge_pending());
    assert_eq!(helper.manager.active_path_id(), helper.first_path_id);

    // simulate receiving a handshake packet to force path validation
    helper.manager[helper.second_path_id].on_handshake_packet();
    assert!(helper.manager[helper.second_path_id].is_validated());

    // Trigger:
    let amplification_outcome = helper
        .manager
        .on_processed_packet(
            helper.second_path_id,
            None,
            path_validation::Probe::NonProbing,
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();

    // Expectation:
    assert!(amplification_outcome.is_active_path_unblocked());
    assert_eq!(helper.manager.active_path_id(), helper.second_path_id);
    assert!(!helper.manager[helper.first_path_id].is_challenge_pending());
    assert!(!helper.manager[helper.second_path_id].is_challenge_pending());
}

#[test]
fn abandon_all_path_challenges() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    assert!(helper.manager[helper.zero_path_id].is_challenge_pending());
    assert!(helper.manager[helper.first_path_id].is_challenge_pending());
    assert!(helper.manager[helper.second_path_id].is_challenge_pending());

    // Trigger:
    helper.manager.abandon_all_path_challenges(&mut publisher);

    // Expectation:
    assert!(!helper.manager[helper.zero_path_id].is_challenge_pending());
    assert!(!helper.manager[helper.first_path_id].is_challenge_pending());
    assert!(!helper.manager[helper.second_path_id].is_challenge_pending());
}

#[test]
//= https://www.rfc-editor.org/rfc/rfc9000#section-9.2
//= type=test
//# An endpoint can migrate a connection to a new local address by
//# sending packets containing non-probing frames from that address.
//
// receiving a path_validation::Probing::NonProbing should update path to active path
fn non_probing_should_update_path_to_active_path() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    assert_eq!(helper.manager.active, helper.first_path_id.as_u8());

    // Trigger:
    let amplification_outcome = helper
        .manager
        .on_processed_packet(
            helper.second_path_id,
            None,
            path_validation::Probe::NonProbing,
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();

    // Expectation:
    assert!(amplification_outcome.is_unchanged());
    assert_eq!(helper.manager.active, helper.second_path_id.as_u8());
}

#[test]
//= https://www.rfc-editor.org/rfc/rfc9000#section-9.2
//= type=test
//# An endpoint can migrate a connection to a new local address by
//# sending packets containing non-probing frames from that address.
//
// receiving a path_validation::Probing::Probing should NOT update path to active path
fn probing_should_not_update_path_to_active_path() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    assert_eq!(helper.manager.active, helper.first_path_id.as_u8());

    // Trigger:
    let amplification_outcome = helper
        .manager
        .on_processed_packet(
            helper.second_path_id,
            None,
            path_validation::Probe::Probing,
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();

    // Expectation:
    assert!(amplification_outcome.is_unchanged());
    assert_eq!(helper.manager.active, helper.first_path_id.as_u8());
}

#[test]
// add new path when receiving a datagram on different remote address
// Setup:
// - create path manger with one path
//
// Trigger:
// - call on_datagram_received with new remote address
//
// Expectation:
// - assert we have two paths
fn test_adding_new_path() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let first_conn_id = connection::PeerId::try_from_bytes(&[1]).unwrap();
    let new_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    let first_path = ServerPath::new(
        new_addr,
        first_conn_id,
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    let mut manager = manager_server(first_path);

    // verify we have one path
    assert!(manager.path(&new_addr).is_some());
    let new_addr: SocketAddr = "127.0.0.2:8001".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    assert!(manager.path(&new_addr).is_none());
    assert_eq!(manager.paths.len(), 1);

    // Trigger:
    let datagram = DatagramInfo {
        timestamp: NoopClock {}.get_time(),
        payload_len: 0,
        ecn: ExplicitCongestionNotification::default(),
        destination_connection_id: connection::LocalId::TEST_ID,
        destination_connection_id_classification: connection::id::Classification::Local,
        source_connection_id: None,
    };
    let (path_id, amplification_outcome) = manager
        .on_datagram_received(
            &new_addr,
            &datagram,
            true,
            &mut Default::default(),
            &mut migration::allow_all::Validator,
            &mut mtu::Manager::new(mtu::Config::default()),
            &Limits::default(),
            &mut publisher,
        )
        .unwrap();

    // Expectation:
    assert_eq!(path_id.as_u8(), 1);
    assert!(amplification_outcome.is_unchanged());
    assert!(manager.path(&new_addr).is_some());
    assert_eq!(manager.paths.len(), 2);
}

#[test]
// do NOT add new path if handshake is not confirmed
// Setup:
// - create path manger with one path
//
// Trigger:
// - call on_datagram_received with new remote address bit handshake_confirmed false
//
// Expectation:
// - assert on_datagram_received does not error
// - assert we have one paths
fn do_not_add_new_path_if_handshake_not_confirmed() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let first_conn_id = connection::PeerId::try_from_bytes(&[1]).unwrap();
    let first_path = ServerPath::new(
        Default::default(),
        first_conn_id,
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    let mut manager = manager_server(first_path);

    // verify we have one path
    let new_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    assert_eq!(manager.paths.len(), 1);

    // Trigger:
    let datagram = DatagramInfo {
        timestamp: NoopClock {}.get_time(),
        payload_len: 0,
        ecn: ExplicitCongestionNotification::default(),
        destination_connection_id: connection::LocalId::TEST_ID,
        destination_connection_id_classification: connection::id::Classification::Local,
        source_connection_id: None,
    };
    let handshake_confirmed = false;
    let on_datagram_result = manager.on_datagram_received(
        &new_addr,
        &datagram,
        handshake_confirmed,
        &mut Default::default(),
        &mut migration::allow_all::Validator,
        &mut mtu::Manager::new(mtu::Config::default()),
        &Limits::default(),
        &mut publisher,
    );

    // Expectation:
    assert!(on_datagram_result.is_ok());
    assert!(manager.path(&new_addr).is_none());
    assert_eq!(manager.paths.len(), 1);
}

#[test]
//= https://www.rfc-editor.org/rfc/rfc9000#section-9
//= type=test
//# If a client receives packets from an unknown server address,
//# the client MUST discard these packets.
//
// Setup:
// - create path manager with one path as a client
//
// Trigger:
// - call on_datagram_received with new remote address bit
//
// Expectation:
// - asset on_datagram_received errors
// - assert we have one path
fn do_not_add_new_path_if_client() {
    // Setup:
    let first_conn_id = connection::PeerId::try_from_bytes(&[1]).unwrap();
    let first_path = ClientPath::new(
        Default::default(),
        first_conn_id,
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    let mut manager = manager_client(first_path);
    let mut publisher = Publisher::snapshot();

    // verify we have one path
    let new_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    assert_eq!(manager.paths.len(), 1);

    // Trigger:
    let datagram = DatagramInfo {
        timestamp: NoopClock {}.get_time(),
        payload_len: 0,
        ecn: ExplicitCongestionNotification::default(),
        destination_connection_id: connection::LocalId::TEST_ID,
        destination_connection_id_classification: connection::id::Classification::Local,
        source_connection_id: None,
    };
    let on_datagram_result = manager.on_datagram_received(
        &new_addr,
        &datagram,
        true,
        &mut Default::default(),
        &mut migration::allow_all::Validator,
        &mut mtu::Manager::new(mtu::Config::default()),
        &Limits::default(),
        &mut publisher,
    );

    // Expectation:
    assert!(on_datagram_result.is_err());
    assert!(manager.path(&new_addr).is_none());
    assert_eq!(manager.paths.len(), 1);
}

#[test]
//= https://www.rfc-editor.org/rfc/rfc9000#section-7.2
//= type=test
//# Until a packet is received from the server, the client MUST
//# use the same Destination Connection ID value on all packets in this
//# connection.
fn switch_destination_connection_id_after_first_server_response() {
    // Setup:
    let initial_cid = connection::PeerId::try_from_bytes(&[0, 0]).unwrap();
    let zero_path_id = path_id(0);
    let path_handle = Default::default();
    let zero_path = ClientPath::new(
        path_handle,
        initial_cid,
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    let mut manager = manager_client(zero_path);
    assert_eq!(manager[zero_path_id].peer_connection_id, initial_cid);

    // Trigger:
    let server_destination_cid = connection::PeerId::try_from_bytes(&[1, 1]).unwrap();
    let mut publisher = Publisher::snapshot();
    let processed_packet = manager.on_processed_packet(
        zero_path_id,
        Some(server_destination_cid),
        path_validation::Probe::NonProbing,
        &mut random::testing::Generator(123),
        &mut publisher,
    );

    // Expectation:
    assert!(processed_packet.is_ok());
    assert_eq!(
        manager[zero_path_id].peer_connection_id,
        server_destination_cid
    );
}

#[test]
fn limit_number_of_connection_migrations() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let new_addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    let first_path = ServerPath::new(
        new_addr,
        connection::PeerId::try_from_bytes(&[1]).unwrap(),
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    let mut manager = manager_server(first_path);
    let mut total_paths = 1;

    for i in 1..u8::MAX {
        let new_addr: SocketAddr = format!("127.0.0.2:{i}").parse().unwrap();
        let new_addr = SocketAddress::from(new_addr);
        let new_addr = RemoteAddress::from(new_addr);
        let now = NoopClock {}.get_time();
        let datagram = DatagramInfo {
            timestamp: now,
            payload_len: 0,
            ecn: ExplicitCongestionNotification::default(),
            destination_connection_id: connection::LocalId::TEST_ID,
            destination_connection_id_classification: connection::id::Classification::Local,
            source_connection_id: None,
        };

        let res = manager.handle_connection_migration(
            &new_addr,
            &datagram,
            &mut Default::default(),
            &mut migration::allow_all::Validator,
            &mut mtu::Manager::new(mtu::Config::default()),
            &Limits::default(),
            &mut publisher,
        );
        match res {
            Ok((id, _)) => {
                let _ = manager.on_processed_packet(
                    id,
                    None,
                    path_validation::Probe::NonProbing,
                    &mut random::testing::Generator(123),
                    &mut publisher,
                );
                total_paths += 1
            }
            Err(_) => break,
        }
    }
    assert_eq!(total_paths, MAX_ALLOWED_PATHS);
}

// Connection migration is still allowed to proceed even if the `disable_active_migration`
// transport parameter is sent, as there is no way to definitely distinguish an active
// migration from a NAT rebind.
#[test]
fn active_connection_migration_disabled() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let new_addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    let first_path = ServerPath::new(
        new_addr,
        connection::PeerId::try_from_bytes(&[1]).unwrap(),
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    let mut manager = manager_server(first_path);
    // Give the path manager some new CIDs so it's able to use one for an active migration.
    // id_2 will be moved to `InUse` immediately due to the handshake CID rotation feature,
    // so id_3 is added as well to have an unused CID available for connection migration
    let id_2 = connection::PeerId::try_from_bytes(b"id02").unwrap();
    assert!(manager
        .on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_1, &mut publisher)
        .is_ok());
    let id_3 = connection::PeerId::try_from_bytes(b"id03").unwrap();
    assert!(manager
        .on_new_connection_id(&id_3, 2, 0, &TEST_TOKEN_2, &mut publisher)
        .is_ok());

    let new_addr: SocketAddr = "127.0.0.2:1".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    let new_cid_1 = connection::LocalId::try_from_bytes(b"id02").unwrap();
    let new_cid_2 = connection::LocalId::try_from_bytes(b"id03").unwrap();
    let now = NoopClock {}.get_time();
    let mut datagram = DatagramInfo {
        timestamp: now,
        payload_len: 0,
        ecn: ExplicitCongestionNotification::default(),
        destination_connection_id: new_cid_1,
        destination_connection_id_classification: connection::id::Classification::Local,
        source_connection_id: None,
    };

    // First try an active migration with active migration disabled
    let res = manager.handle_connection_migration(
        &new_addr,
        &datagram,
        &mut Default::default(),
        &mut migration::allow_all::Validator,
        &mut mtu::Manager::new(mtu::Config::default()),
        // Active connection migration is disabled
        &Limits::default()
            .with_active_connection_migration(false)
            .unwrap(),
        &mut publisher,
    );

    // The migration succeeds
    assert!(res.is_ok());
    assert_eq!(2, manager.paths.len());
    // The new path uses a new CID since there were enough supplied
    assert_eq!(
        manager.paths[res.unwrap().0.as_u8() as usize].peer_connection_id,
        id_3
    );

    // Clear the pending packet authentication to allow another migration to proceed
    manager.pending_packet_authentication = None;

    // Try an active connection migration with active migration enabled (default)
    datagram.destination_connection_id = new_cid_2;

    let res = manager.handle_connection_migration(
        &new_addr,
        &datagram,
        &mut Default::default(),
        &mut migration::allow_all::Validator,
        &mut mtu::Manager::new(mtu::Config::default()),
        &Limits::default(),
        &mut publisher,
    );

    // The migration succeeds
    assert!(res.is_ok());
    assert_eq!(3, manager.paths.len());
    // The new path uses the existing id since there wasn't a new one available
    assert_eq!(
        manager.paths[res.unwrap().0.as_u8() as usize].peer_connection_id,
        id_2
    );

    // Now try a non-active (passive) migration, with active migration disabled
    // the same CID is used, so it's not an active migration
    datagram.destination_connection_id = connection::LocalId::TEST_ID;
    let new_addr: SocketAddr = "127.0.0.3:1".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    // Clear the pending packet authentication to allow another migration to proceed
    manager.pending_packet_authentication = None;

    let res = manager.handle_connection_migration(
        &new_addr,
        &datagram,
        &mut Default::default(),
        &mut migration::allow_all::Validator,
        &mut mtu::Manager::new(mtu::Config::default()),
        // Active connection migration is disabled
        &Limits::default()
            .with_active_connection_migration(false)
            .unwrap(),
        &mut publisher,
    );

    // The passive migration succeeds
    assert!(res.is_ok());
    assert_eq!(4, manager.paths.len());
    // The new path uses the existing id since the peer did not change their destination CID
    assert_eq!(
        manager.paths[res.unwrap().0.as_u8() as usize].peer_connection_id,
        id_2
    );
}

#[test]
fn connection_migration_challenge_behavior() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let first_conn_id = connection::PeerId::try_from_bytes(&[1]).unwrap();
    let new_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    let first_path = ServerPath::new(
        new_addr,
        first_conn_id,
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    let mut manager = manager_server(first_path);

    let new_addr: SocketAddr = "127.0.0.2:8001".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    let now = NoopClock {}.get_time();
    let datagram = DatagramInfo {
        timestamp: now,
        payload_len: 0,
        ecn: ExplicitCongestionNotification::default(),
        destination_connection_id: connection::LocalId::TEST_ID,
        destination_connection_id_classification: connection::id::Classification::Local,
        source_connection_id: None,
    };

    let (path_id, _amplification_outcome) = manager
        .handle_connection_migration(
            &new_addr,
            &datagram,
            &mut Default::default(),
            &mut migration::allow_all::Validator,
            &mut mtu::Manager::new(mtu::Config::default()),
            &Limits::default(),
            &mut publisher,
        )
        .unwrap();

    // verify we have two paths
    assert!(manager.path(&new_addr).is_some());
    assert_eq!(manager.paths.len(), 2);

    assert!(!manager[path_id].is_challenge_pending());

    // notify the manager that the datagram was authenticated - the path should now issue a challenge
    let _ = manager.on_processed_packet(
        path_id,
        None,
        path_validation::Probe::NonProbing,
        &mut random::testing::Generator(123),
        &mut publisher,
    );

    //= https://www.rfc-editor.org/rfc/rfc9000#section-9
    //= type=test
    //# An endpoint MUST
    //# perform path validation (Section 8.2) if it detects any change to a
    //# peer's address, unless it has previously validated that address.
    assert!(manager[path_id].is_challenge_pending());

    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.1
    //= type=test
    //# The endpoint MUST use unpredictable data in every PATH_CHALLENGE
    //# frame so that it can associate the peer's response with the
    //# corresponding PATH_CHALLENGE.
    // Verify that the data stored in the challenge is taken from the random generator
    // TODO does the below actually work?? investigate
    let mut test_rnd_generator = random::testing::Generator(123);
    let mut expected_data: [u8; 8] = [0; 8];
    test_rnd_generator.public_random_fill(&mut expected_data);

    //= https://www.rfc-editor.org/rfc/rfc9000#section-9
    //= type=test
    //# An endpoint MUST
    //# perform path validation (Section 8.2) if it detects any change to a
    //# peer's address, unless it has previously validated that address.
    assert!(manager[path_id].on_path_response(&expected_data));
    assert!(manager[path_id].is_validated());
}

#[test]
// Abandon timer should use max PTO of active and new path(new path uses kInitialRtt)
// Setup 1:
// - create manager with path
// - create datagram for packet on second path
//
// Trigger 1:
// - call handle_connection_migration with packet for second path
//
// Expectation 1:
// - assert that new path uses max_ack_delay from the active path
fn connection_migration_use_max_ack_delay_from_active_path() {
    // Setup 1:
    let mut publisher = Publisher::snapshot();
    let new_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    let first_path = ServerPath::new(
        new_addr,
        connection::PeerId::try_from_bytes(&[1]).unwrap(),
        connection::LocalId::TEST_ID,
        RttEstimator::new(Duration::from_millis(30)),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    let mut manager = manager_server(first_path);

    let new_addr: SocketAddr = "127.0.0.2:8001".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    let now = NoopClock {}.get_time();
    let datagram = DatagramInfo {
        timestamp: now,
        payload_len: 0,
        ecn: ExplicitCongestionNotification::default(),
        destination_connection_id: connection::LocalId::TEST_ID,
        destination_connection_id_classification: connection::id::Classification::Local,
        source_connection_id: None,
    };

    // Trigger 1:
    let (second_path_id, _amplification_outcome) = manager
        .handle_connection_migration(
            &new_addr,
            &datagram,
            &mut Default::default(),
            &mut migration::allow_all::Validator,
            &mut mtu::Manager::new(mtu::Config::default()),
            &Limits::default(),
            &mut publisher,
        )
        .unwrap();
    let first_path_id = path_id(0);

    // Expectation 1:
    // inherit max_ack_delay from the active path
    assert_eq!(manager.active, first_path_id.as_u8());
    assert_eq!(
        &manager[first_path_id].rtt_estimator.max_ack_delay(),
        &manager[second_path_id].rtt_estimator.max_ack_delay()
    );
}

#[test]
// Abandon timer should use max PTO of active and new path(new path uses kInitialRtt)
// Setup 1:
// - create manager with path
// - create datagram for packet on second path
// - call handle_connection_migration with packet for second path
//
// Trigger 1:
// - modify rtt for fist path to detect difference in PTO
//
// Expectation 1:
// - verify PTO of second path > PTO of first path
//
// Setup 2:
// - call on_transmit for second path to send challenge and arm abandon timer
//
// Trigger 2:
// - call second_path.on_timeout with abandon_time - 10ms
//
// Expectation 2:
// - verify challenge is NOT abandoned
//
// Trigger 3:
// - call second_path.on_timeout with abandon_time + 10ms
//
// Expectation 3:
// - verify challenge is abandoned
fn connection_migration_new_path_abandon_timer() {
    // Setup 1:
    let mut publisher = Publisher::snapshot();
    let new_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    let first_path = ServerPath::new(
        new_addr,
        connection::PeerId::try_from_bytes(&[1]).unwrap(),
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    let mut manager = manager_server(first_path);

    let new_addr: SocketAddr = "127.0.0.2:8001".parse().unwrap();
    let new_addr = SocketAddress::from(new_addr);
    let new_addr = RemoteAddress::from(new_addr);
    let now = NoopClock {}.get_time();
    let datagram = DatagramInfo {
        timestamp: now,
        payload_len: 0,
        ecn: ExplicitCongestionNotification::default(),
        destination_connection_id: connection::LocalId::TEST_ID,
        destination_connection_id_classification: connection::id::Classification::Local,
        source_connection_id: None,
    };

    let (second_path_id, _amplification_outcome) = manager
        .handle_connection_migration(
            &new_addr,
            &datagram,
            &mut Default::default(),
            &mut migration::allow_all::Validator,
            &mut mtu::Manager::new(mtu::Config::default()),
            &Limits::default(),
            &mut publisher,
        )
        .unwrap();
    let first_path_id = path_id(0);

    // notify the manager that the datagram was authenticated - the path should now issue a challenge
    let _ = manager.on_processed_packet(
        second_path_id,
        None,
        path_validation::Probe::NonProbing,
        &mut random::testing::Generator(123),
        &mut publisher,
    );

    // Trigger 1:
    // modify rtt for first path so we can detect differences
    manager[first_path_id].rtt_estimator.update_rtt(
        Duration::from_millis(0),
        Duration::from_millis(100),
        now,
        true,
        PacketNumberSpace::ApplicationData,
    );

    // Expectation 1:
    // verify the pto_period of the first path is less than the second path
    let first_path_pto = manager[first_path_id].pto_period(PacketNumberSpace::ApplicationData);
    let second_path_pto = manager[second_path_id].pto_period(PacketNumberSpace::ApplicationData);

    assert_eq!(first_path_pto, Duration::from_millis(300));
    assert_eq!(second_path_pto, Duration::from_millis(999));
    assert!(second_path_pto > first_path_pto);

    // Setup 2:
    // send challenge and arm abandon timer
    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut context = MockWriteContext::new(
        now,
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Client,
    );
    manager[second_path_id].on_transmit(&mut context);

    // Trigger 2:
    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.4
    //= type=test
    //# Endpoints SHOULD abandon path validation based on a timer.
    //
    //= https://www.rfc-editor.org/rfc/rfc9000#section-8.2.4
    //= type=test
    //# Endpoints SHOULD abandon path validation based on a timer.  When
    //# setting this timer, implementations are cautioned that the new path
    //# could have a longer round-trip time than the original.  A value of
    //# three times the larger of the current PTO or the PTO for the new path
    //# (using kInitialRtt, as defined in [QUIC-RECOVERY]) is RECOMMENDED.
    // abandon_duration should use max pto_period: second path
    let abandon_time = now + (second_path_pto * 3);
    manager[second_path_id].on_timeout(
        abandon_time - Duration::from_millis(10),
        path::Id::test_id(),
        &mut random::testing::Generator(123),
        &mut publisher,
    );

    // Expectation 2:
    assert!(manager[second_path_id].is_challenge_pending());

    // Trigger 3:
    manager[second_path_id].on_timeout(
        abandon_time + Duration::from_millis(10),
        path::Id::test_id(),
        &mut random::testing::Generator(123),
        &mut publisher,
    );
    // Expectation 3:
    assert!(!manager[second_path_id].is_challenge_pending());
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.2
//= type=test
//# Upon receipt of an increased Retire Prior To field, the peer MUST
//# stop using the corresponding connection IDs and retire them with
//# RETIRE_CONNECTION_ID frames before adding the newly provided
//# connection ID to the set of active connection IDs.
#[test]
fn stop_using_a_retired_connection_id() {
    let mut publisher = Publisher::snapshot();
    let id_1 = connection::PeerId::try_from_bytes(b"id01").unwrap();
    let first_path = ServerPath::new(
        Default::default(),
        id_1,
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    let mut manager = manager_server(first_path);

    let id_2 = connection::PeerId::try_from_bytes(b"id02").unwrap();
    assert!(manager
        .on_new_connection_id(&id_2, 1, 1, &TEST_TOKEN_1, &mut publisher)
        .is_ok());

    assert_eq!(id_2, manager.paths[0].peer_connection_id);
}

#[test]
fn amplification_limited_true_if_all_paths_amplificaiton_limited() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let helper = helper_manager_with_paths_base(true, false, &mut publisher);
    let zp = &helper.manager[helper.zero_path_id];
    assert!(zp.at_amplification_limit());
    let fp = &helper.manager[helper.first_path_id];
    assert!(fp.at_amplification_limit());
    let sp = &helper.manager[helper.second_path_id];
    assert!(sp.at_amplification_limit());

    // Expectation:
    assert!(helper.manager.is_amplification_limited());
}

#[test]
fn amplification_limited_false_if_any_paths_amplificaiton_limited() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    let fp = &helper.manager[helper.first_path_id];
    assert!(fp.at_amplification_limit());
    let sp = &mut helper.manager[helper.second_path_id];
    let amplification_outcome = sp.on_bytes_received(1200);
    assert!(amplification_outcome.is_inactivate_path_unblocked());
    assert!(!sp.at_amplification_limit());

    // Expectation:
    assert!(!helper.manager.is_amplification_limited());
}

#[test]
fn can_transmit_false_if_no_path_can_transmit() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let helper = helper_manager_with_paths_base(true, false, &mut publisher);

    let zp = &helper.manager[helper.zero_path_id];
    assert!(!transmission::Interest::None.can_transmit(zp.transmission_constraint()));

    let interest = transmission::Interest::Forced;
    let fp = &helper.manager[helper.first_path_id];
    assert!(!interest.can_transmit(fp.transmission_constraint()));
    let sp = &helper.manager[helper.second_path_id];
    assert!(!interest.can_transmit(sp.transmission_constraint()));

    // Expectation:
    assert!(!helper.manager.can_transmit(interest));
}

#[test]
fn can_transmit_true_if_any_path_can_transmit() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    let interest = transmission::Interest::Forced;
    let fp = &helper.manager[helper.first_path_id];
    assert!(!interest.can_transmit(fp.transmission_constraint()));

    let sp = &mut helper.manager[helper.second_path_id];
    let amplification_outcome = sp.on_bytes_received(1200);
    assert!(interest.can_transmit(sp.transmission_constraint()));

    // Expectation:
    assert!(amplification_outcome.is_inactivate_path_unblocked());
    assert!(helper.manager.can_transmit(interest));
}

#[test]
// Return all paths that are pending challenge or response.
fn pending_paths_should_return_paths_pending_validation() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let mut helper = helper_manager_with_paths(&mut publisher);
    let third_path_id = path_id(3);
    let third_conn_id = connection::PeerId::try_from_bytes(&[3]).unwrap();
    let mut third_path = ServerPath::new(
        Default::default(),
        third_conn_id,
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    let expected_response_data = [0; 8];
    third_path.on_path_challenge(&expected_response_data);
    helper.manager.paths.push(third_path);

    // not pending challenge or response
    helper.manager[helper.zero_path_id].abandon_challenge(&mut publisher, 0);
    assert!(!helper.manager[helper.zero_path_id].is_challenge_pending());
    assert!(!helper.manager[helper.zero_path_id].is_response_pending());

    // pending challenge
    assert!(helper.manager[helper.first_path_id].is_challenge_pending());
    assert!(!helper.manager[helper.first_path_id].is_response_pending());
    assert!(helper.manager[helper.second_path_id].is_challenge_pending());
    assert!(!helper.manager[helper.second_path_id].is_response_pending());

    // pending response
    assert!(!helper.manager[third_path_id].is_challenge_pending());
    assert!(helper.manager[third_path_id].is_response_pending());

    let mut pending_paths = helper.manager.paths_pending_validation();

    // inclusive range from 1 to 3
    for i in 1..=3 {
        // Trigger:
        let next = pending_paths.next_path();

        // Expectation:
        let (id, _path_manager) = next.unwrap();
        assert_eq!(id, path_id(i));
    }

    // Trigger:
    let next = pending_paths.next_path();

    // Expectation:
    assert!(next.is_none());
}

#[test]
// Ensure paths are temporary until after authenticating a packet on the path
fn temporary_until_authenticated() {
    let mut publisher = Publisher::snapshot();
    let now = NoopClock {}.get_time();
    let datagram = DatagramInfo {
        timestamp: now,
        payload_len: 0,
        ecn: ExplicitCongestionNotification::default(),
        destination_connection_id: connection::LocalId::TEST_ID,
        destination_connection_id_classification: connection::id::Classification::Local,
        source_connection_id: None,
    };

    // create an initial path
    let first_addr: SocketAddr = "127.0.0.1:8001".parse().unwrap();
    let first_addr = SocketAddress::from(first_addr);
    let first_addr = RemoteAddress::from(first_addr);
    let first_path = ServerPath::new(
        first_addr,
        connection::PeerId::try_from_bytes(&[1]).unwrap(),
        connection::LocalId::TEST_ID,
        RttEstimator::default(),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    );
    let mut manager = manager_server(first_path);

    let second_addr: SocketAddr = "127.0.0.2:8001".parse().unwrap();
    let second_addr = SocketAddress::from(second_addr);
    let second_addr = RemoteAddress::from(second_addr);

    // create a second path without calling on_processed_packet
    let (second_path_id, _amplification_outcome) = manager
        .on_datagram_received(
            &second_addr,
            &datagram,
            true,
            &mut Default::default(),
            &mut migration::allow_all::Validator,
            &mut mtu::Manager::new(mtu::Config::default()),
            &Limits::default(),
            &mut publisher,
        )
        .unwrap();

    assert!(
        !manager[second_path_id].is_challenge_pending(),
        "pending paths should not issue a challenge"
    );

    let third_addr: SocketAddr = "127.0.0.3:8001".parse().unwrap();
    let third_addr = SocketAddress::from(third_addr);
    let third_addr = RemoteAddress::from(third_addr);

    // create a third path
    let (third_path_id, _amplification_outcome) = manager
        .on_datagram_received(
            &third_addr,
            &datagram,
            true,
            &mut Default::default(),
            &mut migration::allow_all::Validator,
            &mut mtu::Manager::new(mtu::Config::default()),
            &Limits::default(),
            &mut publisher,
        )
        .unwrap();

    assert_eq!(
        second_path_id, third_path_id,
        "third path should replace the second"
    );

    assert!(
        !manager[third_path_id].is_challenge_pending(),
        "pending paths should not issue a challenge"
    );

    // notify the manager that the packet was processed
    let _ = manager.on_processed_packet(
        third_path_id,
        None,
        path_validation::Probe::NonProbing,
        &mut random::testing::Generator(123),
        &mut publisher,
    );

    assert!(
        manager[third_path_id].is_challenge_pending(),
        "after processing a packet the path should issue a challenge"
    );

    // receive another datagram with the second_addr
    let (fourth_path_id, _unblocked) = manager
        .on_datagram_received(
            &second_addr,
            &datagram,
            true,
            &mut Default::default(),
            &mut migration::allow_all::Validator,
            &mut mtu::Manager::new(mtu::Config::default()),
            &Limits::default(),
            &mut publisher,
        )
        .unwrap();

    assert_ne!(
        fourth_path_id, third_path_id,
        "a new path should be created"
    );
}

// The last_known_active_validated_path needs to be both validated and also
// activated (the active path at some point in the connection).
//
// This test specifically checks that a currently non-active path can become
// the last_known_active_validated_path if it is activated and receives a
// PATH_RESPONSE which matches its PATH_CHALLENGE.
//
// Setup:
// - path 0 validated and active

// Trigger Setup 1:
// - path 1 non-probing packet
// Expectation Setup 1:
// - path 1 active + not valid + challenge pending
// - last_known_active_validated_path = path 0

// Trigger Setup 2:
// - path 2 non-probing packet
// Expectation Setup 2:
// - path 2 active + not valid + challenge pending
// - path 1 not valid + challenge pending
// - last_known_active_validated_path = path 0

// Trigger 1:
// - path response for path 1
// Expectation 1:
// - path 1 valid + no challenge pending
// - last_known_active_validated_path = path 1
//
// - path 2 active + not valid + challenge pending

// Trigger 2:
// - timeout for path 2 challenge
// Expectation 2:
// - path 1 active
// - path 2 not valid + no challenge pending
// - last_known_active_validated_path = None
#[test]
fn last_known_validated_path_should_update_on_path_response() {
    // Setup:
    let mut publisher = Publisher::snapshot();
    let zero_conn_id = connection::PeerId::try_from_bytes(&[0]).unwrap();
    let first_conn_id = connection::PeerId::try_from_bytes(&[1]).unwrap();
    let second_conn_id = connection::PeerId::try_from_bytes(&[2]).unwrap();

    // path zero
    let zero_path_id = path_id(0);
    let mut zero_path = helper_path(zero_conn_id);
    zero_path.on_handshake_packet();

    let mut random_generator = random::testing::Generator(123);
    let mut peer_id_registry =
        ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
            .create_server_peer_id_registry(
                InternalConnectionIdGenerator::new().generate_id(),
                zero_path.peer_connection_id,
                true,
            );
    assert!(peer_id_registry
        .on_new_connection_id(&first_conn_id, 1, 0, &TEST_TOKEN_1)
        .is_ok());

    assert!(peer_id_registry
        .on_new_connection_id(&second_conn_id, 2, 0, &TEST_TOKEN_2)
        .is_ok());

    let mut manager = Manager::new(zero_path, peer_id_registry);

    assert!(!manager[zero_path_id].is_challenge_pending());
    assert!(manager[zero_path_id].is_validated());
    assert_eq!(manager.active_path_id(), zero_path_id);

    // Trigger Setup 1:
    let first_path_id = path_id(1);
    let mut first_path = helper_path(first_conn_id);
    let now = NoopClock {}.get_time();
    let challenge_expiration = Duration::from_millis(10_000);
    let first_expected_data = [0; 8];
    let challenge = challenge::Challenge::new(challenge_expiration, first_expected_data);
    first_path.set_challenge(challenge);
    manager.paths.push(first_path);
    assert!(manager
        .update_active_path(
            first_path_id,
            &mut random::testing::Generator(123),
            &mut publisher
        )
        .is_ok());

    // Expectation Setup 1:
    assert_eq!(manager.active_path_id(), first_path_id);
    // first
    assert!(manager[first_path_id].is_challenge_pending());
    assert!(!manager[first_path_id].is_validated());
    // last_known_active_validated_path
    assert_eq!(
        manager.last_known_active_validated_path.unwrap(),
        zero_path_id.as_u8()
    );

    // Trigger Setup 2:
    let second_path_id = path_id(2);
    let second_expected_data = [1; 8];
    let challenge = challenge::Challenge::new(challenge_expiration, second_expected_data);
    let mut second_path = helper_path(second_conn_id);
    second_path.set_challenge(challenge);
    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut context = MockWriteContext::new(
        now,
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Client,
    );
    second_path.on_transmit(&mut context); // send challenge and arm timer

    manager.paths.push(second_path);
    assert!(manager
        .update_active_path(
            second_path_id,
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .is_ok());

    // Expectation Setup 2:
    assert_eq!(manager.active_path_id(), second_path_id);
    // second
    assert!(manager[second_path_id].is_challenge_pending());
    assert!(!manager[second_path_id].is_validated());
    // first
    assert!(manager[first_path_id].is_challenge_pending());
    assert!(!manager[first_path_id].is_validated());
    // last_known_active_validated_path
    assert_eq!(
        manager.last_known_active_validated_path.unwrap(),
        zero_path_id.as_u8()
    );

    // Trigger 1:
    // - path response for path 1
    let frame = s2n_quic_core::frame::PathResponse {
        data: &first_expected_data,
    };
    let amplification_outcome = manager.on_path_response(&frame, &mut publisher);
    // Expectation 1:
    assert!(amplification_outcome.is_inactivate_path_unblocked());
    assert_eq!(manager.active_path_id(), second_path_id);
    // second
    assert!(manager[second_path_id].is_challenge_pending());
    assert!(!manager[second_path_id].is_validated());
    // first
    assert!(!manager[first_path_id].is_challenge_pending());
    assert!(manager[first_path_id].is_validated());
    // last_known_active_validated_path
    assert_eq!(
        manager.last_known_active_validated_path.unwrap(),
        first_path_id.as_u8()
    );

    // Trigger 2:
    // - timeout for path 2 challenge
    let amplification_outcome = manager
        .on_timeout(
            now + challenge_expiration + Duration::from_millis(100),
            &mut random::testing::Generator(123),
            &mut publisher,
        )
        .unwrap();

    // Expectation 2:
    assert!(amplification_outcome.is_active_path_unblocked());
    assert_eq!(manager.active_path_id(), first_path_id);
    // second
    assert!(!manager[second_path_id].is_challenge_pending());
    assert!(!manager[second_path_id].is_validated());
    // first
    assert!(!manager[first_path_id].is_challenge_pending());
    assert!(manager[first_path_id].is_validated());
    // last_known_active_validated_path
    assert_eq!(manager.last_known_active_validated_path, None);
}

// creates a test path_manager. also check out `helper_manager_with_paths`
// which calls this helper with preset options
pub fn helper_manager_with_paths_base(
    register_second_conn_id: bool,
    validate_path_zero: bool,
    publisher: &mut Publisher,
) -> Helper {
    let zero_conn_id = connection::PeerId::try_from_bytes(&[0]).unwrap();
    let first_conn_id = connection::PeerId::try_from_bytes(&[1]).unwrap();
    let second_conn_id = connection::PeerId::try_from_bytes(&[2]).unwrap();
    let zero_path_id = path_id(0);
    let first_path_id = path_id(1);
    let second_path_id = path_id(2);
    let mut zero_path = helper_path(zero_conn_id);
    if validate_path_zero {
        // simulate receiving a handshake packet to force path validation
        zero_path.on_handshake_packet();
    }
    assert!(!zero_path.is_challenge_pending());

    let now = NoopClock {}.get_time();
    let challenge_expiration = Duration::from_millis(10_000);
    let first_expected_data = [0; 8];
    let challenge = challenge::Challenge::new(challenge_expiration, first_expected_data);

    let mut first_path = helper_path(first_conn_id);
    first_path.set_challenge(challenge);

    // Create a challenge that will expire in 100ms
    let second_expected_data = [1; 8];
    let challenge = challenge::Challenge::new(challenge_expiration, second_expected_data);
    let mut second_path = helper_path(second_conn_id);
    second_path.set_challenge(challenge);

    let mut random_generator = random::testing::Generator(123);
    let mut peer_id_registry =
        ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
            .create_server_peer_id_registry(
                InternalConnectionIdGenerator::new().generate_id(),
                zero_path.peer_connection_id,
                true,
            );
    assert!(peer_id_registry
        .on_new_connection_id(&first_conn_id, 1, 0, &TEST_TOKEN_1)
        .is_ok());

    if register_second_conn_id {
        assert!(peer_id_registry
            .on_new_connection_id(&second_conn_id, 2, 0, &TEST_TOKEN_2)
            .is_ok());
    }

    let mut manager = ServerManager::new(zero_path, peer_id_registry);
    assert!(manager.peer_id_registry.is_active(&first_conn_id));
    manager.paths.push(first_path);
    manager.paths.push(second_path);
    assert_eq!(manager.paths.len(), 3);

    // update active path to first_path
    assert_eq!(manager.active, zero_path_id.as_u8());
    if validate_path_zero {
        assert!(manager.active_path().is_validated());
    }

    assert!(manager
        .update_active_path(
            first_path_id,
            &mut random::testing::Generator(123),
            publisher,
        )
        .is_ok());
    if validate_path_zero {
        assert!(manager[zero_path_id].is_challenge_pending());
    }

    assert!(manager
        .peer_id_registry
        .consume_new_id_for_existing_path(path_id(0), zero_conn_id, publisher)
        .is_some());

    // assert first_path is active and last_known_active_validated_path
    assert!(manager.peer_id_registry.is_active(&first_conn_id));
    assert_eq!(manager.active, first_path_id.as_u8());

    if validate_path_zero {
        assert_eq!(
            manager.last_known_active_validated_path,
            Some(zero_path_id.as_u8())
        );
    } else {
        assert_eq!(manager.last_known_active_validated_path, None);
    }

    Helper {
        now,
        second_expected_data,
        challenge_expiration,
        zero_path_id,
        first_path_id,
        second_path_id,
        manager,
    }
}

pub fn helper_path(peer_id: connection::PeerId) -> ServerPath {
    let local_conn_id = connection::LocalId::TEST_ID;
    ServerPath::new(
        Default::default(),
        peer_id,
        local_conn_id,
        RttEstimator::new(Duration::from_millis(30)),
        Default::default(),
        false,
        mtu::Config::default(),
        ANTI_AMPLIFICATION_MULTIPLIER,
    )
}

fn helper_manager_with_paths(publisher: &mut Publisher) -> Helper {
    helper_manager_with_paths_base(true, true, publisher)
}

pub struct Helper {
    pub now: Timestamp,
    pub second_expected_data: challenge::Data,
    pub challenge_expiration: Duration,
    pub zero_path_id: Id,
    pub first_path_id: Id,
    pub second_path_id: Id,
    pub manager: ServerManager,
}
