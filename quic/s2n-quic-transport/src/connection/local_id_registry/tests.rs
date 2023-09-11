// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use super::*;
use s2n_quic_core::{
    connection,
    connection::id::MIN_LIFETIME,
    frame::{Frame, NewConnectionId},
    packet::number::PacketNumberRange,
    random,
    stateless_reset::token::testing::*,
    time::{clock::testing as time, timer::Provider as _},
    varint::VarInt,
};

use crate::{
    connection::{
        connection_id_mapper::*,
        local_id_registry::{
            LocalIdInfo, LocalIdRegistrationError, LocalIdRegistry, EXPIRATION_BUFFER,
            MAX_ACTIVE_CONNECTION_ID_LIMIT, RTT_MULTIPLIER,
        },
        InternalConnectionIdGenerator,
    },
    contexts::testing::{MockWriteContext, OutgoingFrameBuffer},
    endpoint, transmission,
    transmission::interest::Provider,
};
use core::time::Duration;

impl LocalIdRegistry {
    fn get_connection_id_info(&self, id: &connection::LocalId) -> Option<&LocalIdInfo> {
        self.registered_ids.iter().find(|id_info| id_info.id == *id)
    }

    fn get_connection_id_info_mut(&mut self, id: &connection::LocalId) -> Option<&mut LocalIdInfo> {
        self.registered_ids
            .iter_mut()
            .find(|id_info| id_info.id == *id)
    }
}

// Helper function to easily generate a LocalId from bytes
fn id(bytes: &[u8]) -> connection::LocalId {
    connection::LocalId::try_from_bytes(bytes).unwrap()
}

// Helper function to easily create a LocalIdRegistry and Mapper
fn mapper(
    handshake_id: connection::LocalId,
    handshake_id_expiration_time: Option<Timestamp>,
    token: stateless_reset::Token,
) -> (ConnectionIdMapper, LocalIdRegistry) {
    let mut random_generator = random::testing::Generator(123);

    let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);
    let registry = mapper.create_local_id_registry(
        InternalConnectionIdGenerator::new().generate_id(),
        &handshake_id,
        handshake_id_expiration_time,
        token,
    );
    (mapper, registry)
}

// Verify that an expiration with the earliest possible time results in a valid retirement time
#[test]
fn minimum_lifetime() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");

    let expiration = time::now() + MIN_LIFETIME;

    let (_mapper, mut reg1) = mapper(ext_id_1, Some(expiration), TEST_TOKEN_1);
    reg1.set_active_connection_id_limit(3);
    assert!(reg1
        .register_connection_id(&ext_id_2, Some(expiration), TEST_TOKEN_2)
        .is_ok());
    assert_eq!(
        Some(expiration - EXPIRATION_BUFFER),
        reg1.get_connection_id_info(&ext_id_1)
            .unwrap()
            .retirement_time
    );
    assert_eq!(
        Some(expiration - EXPIRATION_BUFFER),
        reg1.get_connection_id_info(&ext_id_2)
            .unwrap()
            .retirement_time
    );
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1
//= type=test
//# As a trivial example, this means the same connection ID
//# MUST NOT be issued more than once on the same connection.
#[test]
fn same_connection_id_must_not_be_issued_for_same_connection() {
    let ext_id = id(b"id01");
    let (_, mut reg) = mapper(ext_id, None, TEST_TOKEN_1);

    assert_eq!(
        Err(LocalIdRegistrationError::ConnectionIdInUse),
        reg.register_connection_id(&ext_id, None, TEST_TOKEN_1)
    );
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
//= type=test
//# The sequence number on
//# each newly issued connection ID MUST increase by 1.
#[test]
fn sequence_number_must_increase_by_one() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");

    let (_, mut reg) = mapper(ext_id_1, None, TEST_TOKEN_1);
    reg.set_active_connection_id_limit(3);
    reg.register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
        .unwrap();

    let seq_num_1 = reg
        .get_connection_id_info(&ext_id_1)
        .unwrap()
        .sequence_number;
    let seq_num_2 = reg
        .get_connection_id_info(&ext_id_2)
        .unwrap()
        .sequence_number;

    assert_eq!(1, seq_num_2 - seq_num_1);
}

#[test]
fn connection_mapper_test() {
    let mut id_generator = InternalConnectionIdGenerator::new();
    let mut random_generator = random::testing::Generator(123);
    let mut mapper = ConnectionIdMapper::new(&mut random_generator, endpoint::Type::Server);

    let id1 = id_generator.generate_id();
    let id2 = id_generator.generate_id();

    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");
    let ext_id_3 = id(b"id03");
    let ext_id_4 = id(b"id04");

    let now = time::now();
    let handshake_id_expiration_time = now + Duration::from_secs(60);

    let mut reg1 = mapper.create_local_id_registry(
        id1,
        &ext_id_1,
        Some(handshake_id_expiration_time),
        TEST_TOKEN_1,
    );
    let mut reg2 = mapper.create_local_id_registry(
        id2,
        &ext_id_3,
        Some(handshake_id_expiration_time),
        TEST_TOKEN_3,
    );

    reg1.set_active_connection_id_limit(3);
    reg2.set_active_connection_id_limit(3);

    assert_eq!(
        0,
        reg1.get_connection_id_info(&ext_id_1)
            .unwrap()
            .sequence_number
    );
    assert_eq!(
        Some(handshake_id_expiration_time - EXPIRATION_BUFFER),
        reg1.get_connection_id_info(&ext_id_1)
            .unwrap()
            .retirement_time
    );
    assert_eq!(
        Some((id1, connection::id::Classification::Local,)),
        mapper.lookup_internal_connection_id(&ext_id_1)
    );
    assert_eq!(
        TEST_TOKEN_1,
        reg1.get_connection_id_info(&ext_id_1)
            .unwrap()
            .stateless_reset_token
    );

    assert_eq!(
        Err(LocalIdRegistrationError::ConnectionIdInUse),
        reg2.register_connection_id(&ext_id_1, None, TEST_TOKEN_1)
    );

    let exp_2 = now + Duration::from_secs(60);

    assert!(reg1
        .register_connection_id(&ext_id_2, Some(exp_2), TEST_TOKEN_2)
        .is_ok());
    assert_eq!(
        Some(exp_2 - EXPIRATION_BUFFER),
        reg1.get_connection_id_info(&ext_id_2)
            .unwrap()
            .retirement_time
    );
    assert!(reg2
        .register_connection_id(&ext_id_4, None, TEST_TOKEN_4)
        .is_ok());
    assert_eq!(
        Some((id1, connection::id::Classification::Local,)),
        mapper.lookup_internal_connection_id(&ext_id_2)
    );
    assert_eq!(
        Some((id2, connection::id::Classification::Local,)),
        mapper.lookup_internal_connection_id(&ext_id_3)
    );
    assert_eq!(
        Some((id2, connection::id::Classification::Local,)),
        mapper.lookup_internal_connection_id(&ext_id_4)
    );

    // Unregister id 3 (sequence number 0)
    reg2.get_connection_id_info_mut(&ext_id_3).unwrap().status = PendingRemoval(now);
    reg2.unregister_expired_ids(now);
    assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_3));
    assert_eq!(
        Some((id2, connection::id::Classification::Local,)),
        mapper.lookup_internal_connection_id(&ext_id_4)
    );

    reg2.get_connection_id_info_mut(&ext_id_4).unwrap().status =
        PendingRetirementConfirmation(Some(now));
    reg2.unregister_expired_ids(now);
    assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_4));

    // Put back ID3 and ID4 to test drop behavior
    assert!(reg1
        .register_connection_id(&ext_id_3, None, TEST_TOKEN_3)
        .is_ok());
    assert!(reg2
        .register_connection_id(&ext_id_4, None, TEST_TOKEN_4)
        .is_ok());
    assert_eq!(
        Some((id1, connection::id::Classification::Local,)),
        mapper.lookup_internal_connection_id(&ext_id_3)
    );
    assert_eq!(
        Some((id2, connection::id::Classification::Local,)),
        mapper.lookup_internal_connection_id(&ext_id_4)
    );

    // If a registration is dropped all entries are removed
    drop(reg1);
    assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_1));
    assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_2));
    assert_eq!(None, mapper.lookup_internal_connection_id(&ext_id_3));
    assert_eq!(
        Some((id2, connection::id::Classification::Local,)),
        mapper.lookup_internal_connection_id(&ext_id_4)
    );
}

#[test]
fn on_retire_connection_id() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");

    let now = time::now();
    let (mapper, mut reg1) = mapper(ext_id_1, None, TEST_TOKEN_1);

    reg1.set_active_connection_id_limit(2);

    //= https://www.rfc-editor.org/rfc/rfc9000#section-19.16
    //= type=test
    //# Receipt of a RETIRE_CONNECTION_ID frame containing a sequence number
    //# greater than any previously sent to the peer MUST be treated as a
    //# connection error of type PROTOCOL_VIOLATION.
    assert_eq!(
        Some(LocalIdRegistrationError::InvalidSequenceNumber),
        reg1.on_retire_connection_id(1, &ext_id_1, Duration::default(), now)
            .err()
    );

    assert!(reg1
        .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
        .is_ok());

    let rtt = Duration::from_millis(500);

    //= https://www.rfc-editor.org/rfc/rfc9000#section-19.16
    //= type=test
    //# The sequence number specified in a RETIRE_CONNECTION_ID frame MUST
    //# NOT refer to the Destination Connection ID field of the packet in
    //# which the frame is contained.

    //= https://www.rfc-editor.org/rfc/rfc9000#section-19.16
    //= type=test
    //# The peer MAY treat this as a
    //# connection error of type PROTOCOL_VIOLATION.
    assert_eq!(
        Some(LocalIdRegistrationError::InvalidSequenceNumber),
        reg1.on_retire_connection_id(1, &ext_id_2, Duration::default(), now)
            .err()
    );

    assert!(reg1.on_retire_connection_id(1, &ext_id_1, rtt, now).is_ok());

    assert_eq!(
        PendingRemoval(now + rtt * RTT_MULTIPLIER),
        reg1.get_connection_id_info(&ext_id_2).unwrap().status
    );

    // ID 1 wasn't impacted by the request to retire ID 2
    assert_eq!(
        Active,
        reg1.get_connection_id_info(&ext_id_1).unwrap().status
    );

    //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
    //= type=test
    //# An endpoint SHOULD supply a new connection ID when the peer retires a
    //# connection ID.
    assert_eq!(
        connection::id::Interest::New(1),
        reg1.connection_id_interest()
    );

    //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
    //= type=test
    //# When an endpoint issues a connection ID, it MUST accept packets that
    //# carry this connection ID for the duration of the connection or until
    //# its peer invalidates the connection ID via a RETIRE_CONNECTION_ID
    //# frame (Section 19.16).

    //= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.2
    //= type=test
    //# The endpoint SHOULD continue to
    //# accept the previously issued connection IDs until they are retired by
    //# the peer.
    reg1.unregister_expired_ids(now + rtt * RTT_MULTIPLIER);
    assert!(mapper.lookup_internal_connection_id(&ext_id_2).is_none());
}

#[test]
fn on_retire_connection_id_pending_removal() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");

    let now = time::now() + Duration::from_secs(60);

    let (_, mut reg1) = mapper(ext_id_1, None, TEST_TOKEN_1);
    reg1.set_active_connection_id_limit(2);

    assert!(reg1
        .register_connection_id(&ext_id_2, Some(now), TEST_TOKEN_2)
        .is_ok());

    reg1.retire_handshake_connection_id();
    reg1.on_timeout(now);

    assert_eq!(
        PendingRetirementConfirmation(None),
        reg1.get_connection_id_info(&ext_id_1).unwrap().status
    );
    assert_eq!(
        PendingRetirementConfirmation(Some(now + EXPIRATION_BUFFER)),
        reg1.get_connection_id_info(&ext_id_2).unwrap().status
    );

    let rtt = Duration::from_millis(500);

    assert!(reg1.on_retire_connection_id(1, &ext_id_1, rtt, now).is_ok());

    assert_eq!(
        PendingRetirementConfirmation(None),
        reg1.get_connection_id_info(&ext_id_1).unwrap().status
    );
    // When the ON_RETIRE_CONNECTION_ID frame is received from the peer, the
    // removal time for the retired connection ID is updated
    assert_eq!(
        PendingRemoval(now + rtt * RTT_MULTIPLIER),
        reg1.get_connection_id_info(&ext_id_2).unwrap().status
    );
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
//= type=test
//# An endpoint that initiates migration and requires non-zero-length
//# connection IDs SHOULD ensure that the pool of connection IDs
//# available to its peer allows the peer to use a new connection ID on
//# migration, as the peer will be unable to respond if the pool is
//# exhausted.

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
//= type=test
//# An endpoint SHOULD ensure that its peer has a sufficient number of
//# available and unused connection IDs.

//= https://www.rfc-editor.org/rfc/rfc9000#section-9.5
//= type=test
//# To ensure that migration is possible and packets sent on different
//# paths cannot be correlated, endpoints SHOULD provide new connection
//# IDs before peers migrate; see Section 5.1.1.
#[test]
fn connection_id_interest() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");
    let ext_id_3 = id(b"id03");

    let (_, mut reg1) = mapper(ext_id_1, None, TEST_TOKEN_1);

    // Active connection ID limit starts at 1, so there is no interest initially
    assert_eq!(
        connection::id::Interest::None,
        reg1.connection_id_interest()
    );

    reg1.set_active_connection_id_limit(5);
    assert_eq!(
        MAX_ACTIVE_CONNECTION_ID_LIMIT,
        reg1.active_connection_id_limit as u64
    );

    assert_eq!(
        connection::id::Interest::New(reg1.active_connection_id_limit - 1),
        reg1.connection_id_interest()
    );

    assert!(reg1
        .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
        .is_ok());

    assert_eq!(
        connection::id::Interest::New(reg1.active_connection_id_limit - 2),
        reg1.connection_id_interest()
    );

    assert!(reg1
        .register_connection_id(&ext_id_3, None, TEST_TOKEN_3)
        .is_ok());

    assert_eq!(
        connection::id::Interest::None,
        reg1.connection_id_interest()
    );
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
//= type=test
//# An endpoint MUST NOT
//# provide more connection IDs than the peer's limit.
#[test]
#[should_panic]
fn endpoint_must_not_provide_more_ids_than_peer_limit() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");
    let ext_id_3 = id(b"id03");

    let (_, mut reg1) = mapper(ext_id_1, None, TEST_TOKEN_1);

    reg1.set_active_connection_id_limit(2);

    assert_eq!(
        connection::id::Interest::New(1),
        reg1.connection_id_interest()
    );

    assert!(reg1
        .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
        .is_ok());

    // Panics because we are inserting more than the limit
    let _ = reg1.register_connection_id(&ext_id_3, None, TEST_TOKEN_3);
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
//= type=test
//# An endpoint MAY
//# send connection IDs that temporarily exceed a peer's limit if the
//# NEW_CONNECTION_ID frame also requires the retirement of any excess,
//# by including a sufficiently large value in the Retire Prior To field.
#[test]
fn endpoint_may_exceed_limit_temporarily() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");
    let ext_id_3 = id(b"id03");

    let now = time::now();

    let (_, mut reg1) = mapper(ext_id_1, None, TEST_TOKEN_1);
    reg1.set_active_connection_id_limit(2);

    assert_eq!(
        connection::id::Interest::New(1),
        reg1.connection_id_interest()
    );

    assert!(reg1
        .register_connection_id(&ext_id_2, Some(now + EXPIRATION_BUFFER), TEST_TOKEN_2)
        .is_ok());
    reg1.retire_handshake_connection_id();
    reg1.on_timeout(now + EXPIRATION_BUFFER);

    // We can register another ID because the retire_prior_to field retires old IDs
    assert_eq!(
        connection::id::Interest::New(2),
        reg1.connection_id_interest()
    );
    assert!(reg1
        .register_connection_id(&ext_id_3, None, TEST_TOKEN_3)
        .is_ok());
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1.1
//= type=test
//# An endpoint MAY also limit the issuance of
//# connection IDs to reduce the amount of per-path state it maintains,
//# such as path validation status, as its peer might interact with it
//# over as many paths as there are issued connection IDs.
#[test]
fn endpoint_may_limit_connection_ids() {
    let ext_id_1 = id(b"id01");
    let (_, mut reg1) = mapper(ext_id_1, None, TEST_TOKEN_1);
    reg1.set_active_connection_id_limit(100);

    assert_eq!(
        MAX_ACTIVE_CONNECTION_ID_LIMIT,
        reg1.active_connection_id_limit as u64
    );
}

#[test]
fn on_transmit() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");
    let ext_id_3 = id(b"id03");

    let now = time::now() + Duration::from_secs(60);

    let (_, mut reg1) = mapper(ext_id_1, None, TEST_TOKEN_1);

    reg1.set_active_connection_id_limit(3);

    assert_eq!(
        transmission::Interest::None,
        reg1.get_transmission_interest()
    );

    assert!(reg1
        .register_connection_id(&ext_id_2, Some(now), TEST_TOKEN_2)
        .is_ok());

    assert_eq!(
        transmission::Interest::NewData,
        reg1.get_transmission_interest()
    );

    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut write_context = MockWriteContext::new(
        time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Server,
    );
    reg1.on_transmit(&mut write_context);

    let expected_frame = Frame::NewConnectionId(NewConnectionId {
        sequence_number: VarInt::from_u32(1),
        retire_prior_to: VarInt::from_u32(0),
        connection_id: ext_id_2.as_bytes(),
        stateless_reset_token: TEST_TOKEN_2.as_ref().try_into().unwrap(),
    });

    assert_eq!(
        expected_frame,
        write_context.frame_buffer.pop_front().unwrap().as_frame()
    );

    assert_eq!(
        transmission::Interest::None,
        reg1.get_transmission_interest()
    );

    // Retire everything
    reg1.retire_handshake_connection_id();
    reg1.on_timeout(now);
    assert!(reg1
        .register_connection_id(&ext_id_3, None, TEST_TOKEN_3)
        .is_ok());

    assert_eq!(
        transmission::Interest::NewData,
        reg1.get_transmission_interest()
    );

    // Switch ID 3 to PendingReissue
    reg1.get_connection_id_info_mut(&ext_id_3).unwrap().status = PendingReissue;
    reg1.transmission_interest.clear();

    assert_eq!(
        transmission::Interest::LostData,
        reg1.get_transmission_interest()
    );

    reg1.on_transmit(&mut write_context);

    let expected_frame = Frame::NewConnectionId(NewConnectionId {
        sequence_number: VarInt::from_u32(2),
        retire_prior_to: VarInt::from_u32(2),
        connection_id: ext_id_3.as_bytes(),
        stateless_reset_token: TEST_TOKEN_3.as_ref().try_into().unwrap(),
    });

    assert_eq!(
        expected_frame,
        write_context.frame_buffer.pop_front().unwrap().as_frame()
    );

    assert_eq!(
        transmission::Interest::None,
        reg1.get_transmission_interest()
    );
}

#[test]
fn on_transmit_constrained() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");
    let ext_id_3 = id(b"id03");

    let (_, mut reg1) = mapper(ext_id_1, None, TEST_TOKEN_1);

    reg1.set_active_connection_id_limit(3);

    assert_eq!(
        transmission::Interest::None,
        reg1.get_transmission_interest()
    );

    assert!(reg1
        .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
        .is_ok());
    assert!(reg1
        .register_connection_id(&ext_id_3, None, TEST_TOKEN_3)
        .is_ok());

    assert_eq!(
        transmission::Interest::NewData,
        reg1.get_transmission_interest()
    );

    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut write_context = MockWriteContext::new(
        time::now(),
        &mut frame_buffer,
        transmission::Constraint::RetransmissionOnly,
        transmission::Mode::Normal,
        endpoint::Type::Server,
    );
    reg1.on_transmit(&mut write_context);

    // No frame written because only retransmissions are allowed
    assert!(write_context.frame_buffer.is_empty());

    reg1.get_connection_id_info_mut(&ext_id_2).unwrap().status = PendingReissue;
    reg1.transmission_interest.clear();

    assert_eq!(
        transmission::Interest::LostData,
        reg1.get_transmission_interest()
    );

    reg1.on_transmit(&mut write_context);

    // Only the ID pending reissue should be written
    assert_eq!(1, write_context.frame_buffer.len());

    let expected_frame = Frame::NewConnectionId(NewConnectionId {
        sequence_number: VarInt::from_u32(1),
        retire_prior_to: VarInt::from_u32(0),
        connection_id: ext_id_2.as_bytes(),
        stateless_reset_token: TEST_TOKEN_2.as_ref().try_into().unwrap(),
    });

    assert_eq!(
        expected_frame,
        write_context.frame_buffer.pop_front().unwrap().as_frame()
    );

    assert_eq!(
        transmission::Interest::NewData,
        reg1.get_transmission_interest()
    );
}

#[test]
fn on_packet_ack_and_loss() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");

    let (_, mut reg1) = mapper(ext_id_1, None, TEST_TOKEN_1);

    reg1.set_active_connection_id_limit(3);

    assert!(reg1
        .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
        .is_ok());

    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut write_context = MockWriteContext::new(
        time::now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Server,
    );

    // Transition ID to PendingAcknowledgement
    let packet_number = write_context.packet_number();
    reg1.on_transmit(&mut write_context);

    // Packet was lost
    reg1.on_packet_loss(&PacketNumberRange::new(packet_number, packet_number));

    assert_eq!(
        PendingReissue,
        reg1.get_connection_id_info(&ext_id_2).unwrap().status
    );

    // Transition ID to PendingAcknowledgement again
    let packet_number = write_context.packet_number();
    reg1.on_transmit(&mut write_context);

    reg1.on_packet_ack(&PacketNumberRange::new(packet_number, packet_number));

    assert_eq!(
        Active,
        reg1.get_connection_id_info(&ext_id_2).unwrap().status
    );
    assert_eq!(
        stateless_reset::Token::ZEROED,
        reg1.get_connection_id_info(&ext_id_2)
            .unwrap()
            .stateless_reset_token
    );
}

#[test]
fn timers() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");

    let (_, mut reg1) = mapper(ext_id_1, None, TEST_TOKEN_1);
    reg1.set_active_connection_id_limit(3);

    // No timer set for the handshake connection ID
    assert_eq!(0, reg1.armed_timer_count());

    let now = time::now();
    let expiration = now + Duration::from_secs(60);

    assert!(reg1
        .register_connection_id(&ext_id_2, Some(expiration), TEST_TOKEN_2)
        .is_ok());

    // Expiration timer is armed based on retire time
    assert_eq!(1, reg1.armed_timer_count());
    assert_eq!(Some(expiration - EXPIRATION_BUFFER), reg1.next_expiration());

    reg1.get_connection_id_info_mut(&ext_id_1)
        .unwrap()
        .retire(Some(now));
    reg1.next_expiration.clear();

    // Expiration timer is armed based on removal time
    assert_eq!(1, reg1.armed_timer_count());
    assert_eq!(Some(now + EXPIRATION_BUFFER), reg1.next_expiration());

    reg1.get_connection_id_info_mut(&ext_id_2)
        .unwrap()
        .retire(Some(now));
    reg1.next_expiration.clear();

    // Expiration timer is armed based on removal time
    assert_eq!(1, reg1.armed_timer_count());
    assert_eq!(Some(now + EXPIRATION_BUFFER), reg1.next_expiration());

    // Unregister CIDs 1 and 2 (sequence numbers 0 and 1)
    reg1.unregister_expired_ids(now + Duration::from_secs(120));

    // No more timers are set
    assert_eq!(0, reg1.armed_timer_count());
}

#[test]
fn on_timeout() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");
    let ext_id_3 = id(b"id03");

    let now = time::now();
    let handshake_expiration = now + Duration::from_secs(60);

    let (_, mut reg1) = mapper(ext_id_1, Some(handshake_expiration), TEST_TOKEN_1);
    reg1.set_active_connection_id_limit(3);

    // Timer set for the handshake connection ID
    assert_eq!(1, reg1.armed_timer_count());

    reg1.retire_handshake_connection_id();

    // Too early, no timer is ready
    reg1.on_timeout(now);

    assert_eq!(Some(handshake_expiration), reg1.next_expiration());
    assert!(reg1.get_connection_id_info(&ext_id_1).is_some());

    // Now the expiration timer is ready
    reg1.on_timeout(handshake_expiration);
    // ID 1 was removed since it expired
    assert!(reg1.get_connection_id_info(&ext_id_1).is_none());
    assert!(reg1.next_expiration().is_none());

    let expiration_2 = now + Duration::from_secs(60);
    let expiration_3 = now + Duration::from_secs(120);

    assert!(reg1
        .register_connection_id(&ext_id_2, Some(expiration_2), TEST_TOKEN_2)
        .is_ok());
    assert!(reg1
        .register_connection_id(&ext_id_3, Some(expiration_3), TEST_TOKEN_3)
        .is_ok());

    // Expiration timer is set based on the retirement time of ID 2
    assert_eq!(
        Some(expiration_2 - EXPIRATION_BUFFER),
        reg1.next_expiration()
    );

    reg1.on_timeout(expiration_2 - EXPIRATION_BUFFER);

    // ID 2 is moved into pending retirement confirmation
    assert_eq!(
        PendingRetirementConfirmation(Some(expiration_2)),
        reg1.get_connection_id_info(&ext_id_2).unwrap().status
    );
    // Expiration timer is set to the expiration time of ID 2
    assert_eq!(Some(expiration_2), reg1.next_expiration());

    reg1.on_timeout(expiration_2);

    assert!(reg1.get_connection_id_info(&ext_id_2).is_none());

    // Expiration timer is set to the retirement time of ID 3
    assert_eq!(
        Some(expiration_3 - EXPIRATION_BUFFER),
        reg1.next_expiration()
    );
}

#[test]
fn retire_handshake_connection_id() {
    let ext_id_1 = id(b"id01");
    let ext_id_2 = id(b"id02");
    let ext_id_3 = id(b"id03");

    let now = time::now();
    let handshake_expiration = now + Duration::from_secs(60);
    let (_, mut reg1) = mapper(ext_id_1, Some(handshake_expiration), TEST_TOKEN_1);

    reg1.set_active_connection_id_limit(3);

    assert!(reg1
        .register_connection_id(&ext_id_2, None, TEST_TOKEN_2)
        .is_ok());
    assert!(reg1
        .register_connection_id(&ext_id_3, None, TEST_TOKEN_3)
        .is_ok());

    reg1.retire_handshake_connection_id();

    assert_eq!(3, reg1.registered_ids.iter().count());

    for id_info in reg1.registered_ids.iter() {
        if id_info.sequence_number == 0 {
            assert!(id_info.is_retired());
            assert_eq!(
                Some(handshake_expiration - EXPIRATION_BUFFER),
                id_info.retirement_time
            );
        } else {
            assert!(!id_info.is_retired())
        }
    }

    // Calling retire_handshake_connection_id again does nothing
    reg1.retire_handshake_connection_id();

    assert_eq!(3, reg1.registered_ids.iter().count());

    for id_info in reg1.registered_ids.iter() {
        if id_info.sequence_number == 0 {
            assert!(id_info.is_retired());
            assert_eq!(
                Some(handshake_expiration - EXPIRATION_BUFFER),
                id_info.retirement_time
            );
        } else {
            assert!(!id_info.is_retired())
        }
    }

    assert!(reg1
        .on_retire_connection_id(0, &ext_id_2, Duration::from_millis(100), now)
        .is_ok());

    // ON_RETIRE_CONNECTION_ID received for the handshake CID should change the status to PendingRemoval
    for id_info in reg1.registered_ids.iter() {
        if id_info.sequence_number == 0 {
            assert!(matches!(id_info.status, PendingRemoval(_)));
        } else {
            assert!(!id_info.is_retired())
        }
    }
}
