// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use crate::{
    connection::{
        peer_id_registry::{
            testing::{id, peer_registry},
            PeerIdRegistrationError,
            PeerIdRegistrationError::{
                ExceededActiveConnectionIdLimit, ExceededRetiredConnectionIdLimit,
                InvalidNewConnectionId,
            },
            PeerIdStatus::{
                InUse, InUsePendingNewConnectionId, New, PendingAcknowledgement, PendingRetirement,
                PendingRetirementRetransmission,
            },
            RETIRED_CONNECTION_ID_LIMIT,
        },
        ConnectionIdMapper, InternalConnectionIdGenerator,
    },
    contexts::{
        testing::{MockWriteContext, OutgoingFrameBuffer},
        WriteContext,
    },
    transmission,
    transmission::interest::Provider,
};
use s2n_quic_core::{
    endpoint,
    frame::{new_connection_id::STATELESS_RESET_TOKEN_LEN, Frame, RetireConnectionId},
    packet::number::PacketNumberRange,
    random,
    stateless_reset::token::testing::*,
    time::clock::testing as time,
    transport,
    varint::VarInt,
};

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.2
//= type=test
//# An endpoint SHOULD limit the number of connection IDs it has retired
//# locally for which RETIRE_CONNECTION_ID frames have not yet been
//# acknowledged.  An endpoint SHOULD allow for sending and tracking a
//# number of RETIRE_CONNECTION_ID frames of at least twice the value of
//# the active_connection_id_limit transport parameter.  An endpoint MUST
//# NOT forget a connection ID without retiring it, though it MAY choose
//# to treat having connection IDs in need of retirement that exceed this
//# limit as a connection error of type CONNECTION_ID_LIMIT_ERROR.
#[test]
fn error_when_exceeding_retired_connection_id_limit() {
    let id_1 = id(b"id01");
    let mut reg = peer_registry(id_1, None);

    // Register 6 more new IDs for a total of 7, with 6 retired
    for i in 2u32..=(RETIRED_CONNECTION_ID_LIMIT + 1).into() {
        assert!(reg
            .on_new_connection_id(
                &id(&i.to_ne_bytes()),
                i,
                i,
                &[i as u8; STATELESS_RESET_TOKEN_LEN].into()
            )
            .is_ok());
    }

    // Retiring one more ID exceeds the limit
    let result = reg.on_new_connection_id(
        &id(b"id08"),
        8,
        8,
        &[8_u8; STATELESS_RESET_TOKEN_LEN].into(),
    );

    assert_eq!(Some(ExceededRetiredConnectionIdLimit), result.err());
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
//= type=test
//# After processing a NEW_CONNECTION_ID frame and
//# adding and retiring active connection IDs, if the number of active
//# connection IDs exceeds the value advertised in its
//# active_connection_id_limit transport parameter, an endpoint MUST
//# close the connection with an error of type CONNECTION_ID_LIMIT_ERROR.
#[test]
fn error_when_exceeding_active_connection_id_limit() {
    let id_1 = id(b"id01");
    let mut reg = peer_registry(id_1, None);

    // Register 5 more new IDs with a retire_prior_to of 4, so there will be a total of
    // 6 connection IDs, with 3 retired and 3 active
    for i in 2u32..=6 {
        assert!(reg
            .on_new_connection_id(
                &id(&i.to_ne_bytes()),
                i,
                4,
                &[i as u8; STATELESS_RESET_TOKEN_LEN].into()
            )
            .is_ok());
    }

    // Adding one more ID exceeds the limit
    let result = reg.on_new_connection_id(
        &id(b"id07"),
        8,
        0,
        &[8_u8; STATELESS_RESET_TOKEN_LEN].into(),
    );

    assert_eq!(Some(ExceededActiveConnectionIdLimit), result.err());
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
//= type=test
//# Receipt of the same frame multiple times MUST NOT be treated as a
//# connection error.
#[test]
fn no_error_when_duplicate() {
    let id_1 = id(b"id01");
    let mut reg = peer_registry(id_1, None);

    let id_2 = id(b"id02");
    assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_1).is_ok());

    assert_eq!(2, reg.registered_ids.len());
    reg.registered_ids[1].status = PendingRetirement;

    assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_1).is_ok());
    assert_eq!(2, reg.registered_ids.len());
    assert_eq!(PendingRetirement, reg.registered_ids[1].status);
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-18.2
//= type=test
//# The value of the active_connection_id_limit parameter MUST be at least 2.
#[test]
fn active_connection_id_limit_must_be_at_least_2() {
    let id_1 = id(b"id01");
    let mut reg = peer_registry(id_1, None);

    let id_2 = id(b"id02");
    assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_1).is_ok());

    let id_3 = id(b"id03");
    assert!(reg.on_new_connection_id(&id_3, 2, 0, &TEST_TOKEN_2).is_ok());

    assert_eq!(
        2,
        reg.registered_ids
            .iter()
            .filter(|id_info| id_info.is_active())
            .count()
    );
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
//= type=test
//# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
//# previously issued connection ID with a different Stateless Reset
//# Token field value or a different Sequence Number field value, or if a
//# sequence number is used for different connection IDs, the endpoint
//# MAY treat that receipt as a connection error of type
//# PROTOCOL_VIOLATION.
#[test]
fn duplicate_new_id_different_token_or_sequence_number() {
    let id_1 = id(b"id01");
    let mut reg = peer_registry(id_1, None);

    let id_2 = id(b"id02");
    assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_1).is_ok());

    // Change the sequence number
    let mut result = reg.on_new_connection_id(&id_2, 2, 0, &TEST_TOKEN_1);
    assert_eq!(Some(InvalidNewConnectionId), result.err());

    // Change the stateless reset token
    result = reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_2);
    assert_eq!(Some(InvalidNewConnectionId), result.err());
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
//= type=test
//# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
//# previously issued connection ID with a different Stateless Reset
//# Token field value or a different Sequence Number field value, or if a
//# sequence number is used for different connection IDs, the endpoint
//# MAY treat that receipt as a connection error of type
//# PROTOCOL_VIOLATION.
#[test]
fn non_duplicate_new_id_same_token_or_sequence_number() {
    let id_1 = id(b"id01");
    let mut reg = peer_registry(id_1, None);

    let id_2 = id(b"id02");
    let id_3 = id(b"id03");
    assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_1).is_ok());

    // Same sequence number
    let mut result = reg.on_new_connection_id(&id_3, 1, 0, &TEST_TOKEN_2);
    assert_eq!(Some(InvalidNewConnectionId), result.err());

    //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.2
    //= type=test
    //# Endpoints are not required to compare new values
    //# against all previous values, but a duplicate value MAY be treated as
    //# a connection error of type PROTOCOL_VIOLATION.
    // Same stateless reset token
    result = reg.on_new_connection_id(&id_3, 2, 0, &TEST_TOKEN_1);
    assert_eq!(Some(InvalidNewConnectionId), result.err());
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
//= type=test
//# A receiver MUST ignore any Retire Prior To fields that do not
//# increase the largest received Retire Prior To value.
#[test]
fn ignore_retire_prior_to_that_does_not_increase() {
    let id_1 = id(b"id01");
    let mut reg = peer_registry(id_1, None);

    let id_2 = id(b"id02");
    let id_3 = id(b"id03");
    let id_4 = id(b"id04");
    assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_2).is_ok());
    assert!(reg.on_new_connection_id(&id_3, 2, 1, &TEST_TOKEN_3).is_ok());
    assert_eq!(1, reg.retire_prior_to);
    assert!(reg.on_new_connection_id(&id_4, 3, 0, &TEST_TOKEN_4).is_ok());
    assert_eq!(1, reg.retire_prior_to);
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.2
//= type=test
//# Upon receipt of an increased Retire Prior To field, the peer MUST
//# stop using the corresponding connection IDs and retire them with
//# RETIRE_CONNECTION_ID frames before adding the newly provided
//# connection ID to the set of active connection IDs.
#[test]
fn retire_connection_id_when_retire_prior_to_increases() {
    let id_1 = id(b"id01");
    let mut random_generator = random::testing::Generator(123);
    let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
    // Even if `rotate_handshake_connection_id` is false, the handshake connection ID should be retired if the peer requests it
    let mut reg = mapper
        .create_client_peer_id_registry(InternalConnectionIdGenerator::new().generate_id(), false);
    reg.register_initial_connection_id(id_1);
    reg.register_initial_stateless_reset_token(TEST_TOKEN_1);

    let id_2 = id(b"id02");
    assert!(reg.on_new_connection_id(&id_2, 1, 1, &TEST_TOKEN_2).is_ok());

    assert_eq!(PendingRetirement, reg.registered_ids[0].status);
    assert_eq!(
        transmission::Interest::NewData,
        reg.get_transmission_interest()
    );

    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut write_context = MockWriteContext::new(
        time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Server,
    );
    let packet_number = write_context.packet_number();
    reg.on_transmit(&mut write_context);

    let expected_frame = Frame::RetireConnectionId(RetireConnectionId {
        sequence_number: VarInt::from_u32(0),
    });

    assert_eq!(
        expected_frame,
        write_context.frame_buffer.pop_front().unwrap().as_frame()
    );
    assert_eq!(
        PendingAcknowledgement(packet_number),
        reg.registered_ids[0].status
    );

    assert_eq!(
        transmission::Interest::None,
        reg.get_transmission_interest()
    );

    reg.on_packet_loss(&PacketNumberRange::new(packet_number, packet_number));

    assert_eq!(
        PendingRetirementRetransmission,
        reg.registered_ids[0].status
    );
    assert_eq!(
        transmission::Interest::LostData,
        reg.get_transmission_interest()
    );

    // Transition ID to PendingAcknowledgement again
    let packet_number = write_context.packet_number();
    reg.on_transmit(&mut write_context);

    assert_eq!(
        Some(reg.internal_id),
        mapper.remove_internal_connection_id_by_stateless_reset_token(&TEST_TOKEN_1)
    );

    reg.on_packet_ack(&PacketNumberRange::new(packet_number, packet_number));

    // ID 1 was removed
    assert_eq!(reg.registered_ids.len(), 1);
    assert_eq!(id_2, reg.registered_ids[0].id);
    //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.1
    //= type=test
    //# An endpoint MUST NOT check for any stateless reset tokens associated
    //# with connection IDs it has not used or for connection IDs that have
    //# been retired.
    assert_eq!(
        None,
        mapper.remove_internal_connection_id_by_stateless_reset_token(&TEST_TOKEN_1)
    );

    assert_eq!(
        transmission::Interest::None,
        reg.get_transmission_interest()
    );
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
//= type=test
//# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
//# previously issued connection ID with a different Stateless Reset
//# Token field value or a different Sequence Number field value, or if a
//# sequence number is used for different connection IDs, the endpoint
//# MAY treat that receipt as a connection error of type
//# PROTOCOL_VIOLATION.
#[test]
fn retire_new_connection_id_if_sequence_number_smaller_than_retire_prior_to() {
    let id_1 = id(b"id01");
    let mut reg = peer_registry(id_1, None);

    let id_2 = id(b"id02");
    assert!(reg
        .on_new_connection_id(&id_2, 10, 10, &TEST_TOKEN_2)
        .is_ok());
    assert_eq!(PendingRetirement, reg.registered_ids[0].status);
    assert_eq!(New, reg.registered_ids[1].status);

    let id_3 = id(b"id03");
    assert!(reg.on_new_connection_id(&id_3, 1, 0, &TEST_TOKEN_3).is_ok());

    assert_eq!(New, reg.registered_ids[1].status);
    assert_eq!(PendingRetirement, reg.registered_ids[2].status);
}

#[test]
fn retire_initial_id_when_new_connection_id_available_rotate_handshake_connection_id_enabled() {
    let id_1 = id(b"id01");
    let mut reg = peer_registry(id_1, None);
    assert!(reg.rotate_handshake_connection_id);

    assert_eq!(InUsePendingNewConnectionId, reg.registered_ids[0].status);

    let id_2 = id(b"id02");
    assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_2).is_ok());

    assert_eq!(PendingRetirement, reg.registered_ids[0].status);
}

#[test]
fn retire_initial_id_when_new_connection_id_available_rotate_handshake_connection_id_disabled() {
    let id_1 = id(b"id01");
    let mut random_generator = random::testing::Generator(123);

    let mut reg = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server)
        .create_client_peer_id_registry(InternalConnectionIdGenerator::new().generate_id(), false);
    reg.register_initial_connection_id(id_1);
    assert!(!reg.rotate_handshake_connection_id);

    assert_eq!(InUse, reg.registered_ids[0].status);

    let id_2 = id(b"id02");
    assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_2).is_ok());

    assert_eq!(InUse, reg.registered_ids[0].status);
}

#[test]
pub fn initial_id_is_active() {
    let id_1 = id(b"id01");
    let mut random_generator = random::testing::Generator(123);
    let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
    let reg = mapper.create_server_peer_id_registry(
        InternalConnectionIdGenerator::new().generate_id(),
        id_1,
        true,
    );

    assert!(reg.is_active(&id_1));
}

#[test]
pub fn retired_id_is_not_active() {
    let id_1 = id(b"id01");
    let mut random_generator = random::testing::Generator(123);
    let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
    let mut reg = mapper.create_server_peer_id_registry(
        InternalConnectionIdGenerator::new().generate_id(),
        id_1,
        true,
    );

    assert!(reg.is_active(&id_1));
    reg.registered_ids[0].status = PendingRetirement;
    assert!(!reg.is_active(&id_1));
}

#[test]
pub fn unknown_id_is_not_active() {
    let id_1 = id(b"id01");
    let mut random_generator = random::testing::Generator(123);
    let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
    let reg = mapper.create_server_peer_id_registry(
        InternalConnectionIdGenerator::new().generate_id(),
        id_1,
        true,
    );

    assert!(reg.is_active(&id_1));
    let id_unknown = id(b"unknown");
    assert!(!reg.is_active(&id_unknown));
}

#[test]
pub fn consume_new_id_should_return_id() {
    let id_1 = id(b"id01");
    let mut random_generator = random::testing::Generator(123);
    let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
    let mut reg = mapper.create_server_peer_id_registry(
        InternalConnectionIdGenerator::new().generate_id(),
        id_1,
        true,
    );

    let id_2 = id(b"id02");
    assert!(reg.on_new_connection_id(&id_2, 1, 0, &TEST_TOKEN_2).is_ok());
    reg.registered_ids[1].status = New;

    assert!(reg
        .state
        .lock()
        .unwrap()
        .stateless_reset_map
        .remove(&TEST_TOKEN_2)
        .is_none());
    assert_eq!(Some(id_2), reg.consume_new_id_inner());
    reg.registered_ids[1].status = InUse;
    // this is an indirect way to test that we inserted a reset token when we consumed id_2
    assert!(reg
        .state
        .lock()
        .unwrap()
        .stateless_reset_map
        .remove(&TEST_TOKEN_2)
        .is_some());
}

#[test]
pub fn consume_new_id_should_error_if_no_ids_are_available() {
    let id_1 = id(b"id01");
    let mut random_generator = random::testing::Generator(123);
    let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
    let mut reg = mapper.create_server_peer_id_registry(
        InternalConnectionIdGenerator::new().generate_id(),
        id_1,
        true,
    );

    assert_eq!(None, reg.consume_new_id_inner());
}

#[test]
fn error_conversion() {
    //= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
    //= type=test
    //# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
    //# previously issued connection ID with a different Stateless Reset
    //# Token field value or a different Sequence Number field value, or if a
    //# sequence number is used for different connection IDs, the endpoint
    //# MAY treat that receipt as a connection error of type
    //# PROTOCOL_VIOLATION.
    let mut transport_error: transport::Error =
        PeerIdRegistrationError::InvalidNewConnectionId.into();
    assert_eq!(
        transport::Error::PROTOCOL_VIOLATION.code,
        transport_error.code
    );

    //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
    //= type=test
    //# After processing a NEW_CONNECTION_ID frame and
    //# adding and retiring active connection IDs, if the number of active
    //# connection IDs exceeds the value advertised in its
    //# active_connection_id_limit transport parameter, an endpoint MUST
    //# close the connection with an error of type CONNECTION_ID_LIMIT_ERROR.
    transport_error = PeerIdRegistrationError::ExceededActiveConnectionIdLimit.into();
    assert_eq!(
        transport::Error::CONNECTION_ID_LIMIT_ERROR.code,
        transport_error.code
    );

    //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.2
    //= type=test
    //# An endpoint MUST NOT forget a connection ID without retiring it,
    //# though it MAY choose to treat having connection IDs in need of
    //# retirement that exceed this limit as a connection error of type
    //# CONNECTION_ID_LIMIT_ERROR.
    transport_error = PeerIdRegistrationError::ExceededRetiredConnectionIdLimit.into();
    assert_eq!(
        transport::Error::CONNECTION_ID_LIMIT_ERROR.code,
        transport_error.code
    );
}

#[test]
pub fn client_peer_id_registry_should_not_register_cid() {
    let mut random_generator = random::testing::Generator(123);
    let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
    let reg = mapper
        .create_client_peer_id_registry(InternalConnectionIdGenerator::new().generate_id(), true);

    assert!(reg.registered_ids.is_empty());
    assert!(reg.is_empty());
}
