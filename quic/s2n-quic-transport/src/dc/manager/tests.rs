// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    contexts::testing::MockWriteContext,
    endpoint::testing::{Client, Server},
};
use insta::{assert_debug_snapshot, assert_snapshot};
use s2n_quic_core::{
    crypto::tls::testing::Session,
    dc::testing::MockDcPath,
    event::testing::Publisher,
    frame::Frame,
    packet::number::{PacketNumberRange, PacketNumberSpace},
    stateless_reset::token::testing::{TEST_TOKEN_1, TEST_TOKEN_2, TEST_TOKEN_3},
    time::clock::testing::now,
    transmission::{interest::Provider as _, writer::testing::OutgoingFrameBuffer, Provider as _},
    varint::VarInt,
};

#[test]
fn new() {
    let mut publisher = Publisher::snapshot();
    let manager: Manager<Server> = Manager::new(Some(MockDcPath::default()), 1, &mut publisher);

    assert!(matches!(manager.state, State::InitServer));
    assert_eq!(Some(1), manager.version);

    let manager: Manager<Client> = Manager::new(Some(MockDcPath::default()), 1, &mut publisher);

    assert!(matches!(manager.state, State::InitClient));
    assert_eq!(Some(1), manager.version);
}

#[test]
fn disabled() {
    let mut publisher = Publisher::snapshot();
    let manager: Manager<Server> = Manager::disabled();
    assert_eq!(None, manager.version());
    assert!(manager.state.is_complete());
    assert!(!manager.has_transmission_interest());

    let mut manager: Manager<Server> = Manager::new(None, 1, &mut publisher);
    let ack_set = &PacketNumberRange::new(pn(1), pn(2));
    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut context = MockWriteContext::new(
        now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Client,
    );

    assert_eq!(None, manager.version());
    assert!(manager.state.is_complete());
    assert!(!manager.has_transmission_interest());

    // verify calling all the methods doesn't panic
    manager.on_peer_dc_stateless_reset_tokens([TEST_TOKEN_1].iter(), &mut publisher);
    manager.on_path_secrets_ready(&Session, &mut publisher);
    manager.on_packet_ack(ack_set, &mut publisher);
    manager.on_packet_loss(ack_set);
    manager.on_transmit(&mut context);

    // state is still complete
    assert!(manager.state.is_complete());
}

#[test]
fn on_path_secrets_ready() {
    let mut publisher = Publisher::snapshot();
    let path = MockDcPath::default();
    let mut manager: Manager<Server> = Manager::new(Some(path), 1, &mut publisher);

    manager.on_path_secrets_ready(&Session, &mut publisher);

    assert_eq!(1, manager.path().on_path_secrets_ready_count);
    assert!(manager.state.is_path_secrets_ready());
    // Server doesn't transmit until it receives tokens from the client
    assert!(!manager.has_transmission_interest());

    let path = MockDcPath::default();
    let mut manager: Manager<Client> = Manager::new(Some(path), 1, &mut publisher);

    manager.on_path_secrets_ready(&Session, &mut publisher);

    assert_eq!(1, manager.path().on_path_secrets_ready_count);
    assert!(manager.state.is_path_secrets_ready());
    // Client starts transmitting as soon as path secrets are ready
    assert!(manager.has_transmission_interest());
}

#[test]
fn on_peer_dc_stateless_reset_tokens_server() {
    let mut publisher = Publisher::snapshot();
    let path = MockDcPath::default();
    let mut manager: Manager<Server> = Manager::new(Some(path), 1, &mut publisher);

    on_peer_dc_stateless_reset_tokens(&mut manager, &mut publisher);
}

#[test]
fn on_peer_dc_stateless_reset_tokens_client() {
    let mut publisher = Publisher::snapshot();
    let path = MockDcPath::default();
    let mut manager: Manager<Client> = Manager::new(Some(path), 1, &mut publisher);

    on_peer_dc_stateless_reset_tokens(&mut manager, &mut publisher);
}

fn on_peer_dc_stateless_reset_tokens<Config, Endpoint>(
    manager: &mut Manager<Config>,
    publisher: &mut Publisher,
) where
    Config: endpoint::Config<DcEndpoint = Endpoint>,
    Endpoint: dc::Endpoint<Path = MockDcPath>,
{
    let tokens = [TEST_TOKEN_1, TEST_TOKEN_2, TEST_TOKEN_3];

    manager.on_peer_dc_stateless_reset_tokens(tokens.iter(), publisher);

    // peer tokens were delivered too early
    assert_eq!(0, manager.path().on_peer_stateless_reset_tokens_count);
    assert!(manager.path().peer_stateless_reset_tokens.is_empty());

    // Now path secrets are ready, so the peer tokens are received
    manager.on_path_secrets_ready(&Session, publisher);

    manager.on_peer_dc_stateless_reset_tokens(tokens.iter(), publisher);

    assert_eq!(1, manager.path().on_peer_stateless_reset_tokens_count);
    assert_eq!(
        tokens.as_slice(),
        manager.path().peer_stateless_reset_tokens.as_slice()
    );

    if Config::ENDPOINT_TYPE.is_server() {
        assert!(manager.state.is_server_tokens_sent());
    } else {
        assert!(manager.state.is_complete());
    }

    // Receiving the peer tokens again doesn't call the provider again
    manager.on_peer_dc_stateless_reset_tokens(tokens.iter(), publisher);
    assert_eq!(1, manager.path().on_peer_stateless_reset_tokens_count);
}

#[test]
fn on_packet_ack_client() {
    let mut publisher = Publisher::snapshot();
    let mut path = MockDcPath::default();
    let tokens = [TEST_TOKEN_1, TEST_TOKEN_2];
    path.stateless_reset_tokens.extend(tokens);
    let mut manager: Manager<Client> = Manager::new(Some(path), 1, &mut publisher);
    on_packet_ack(&mut manager, tokens.as_slice(), &mut publisher);

    // Client completes when it has received stateless reset tokens from the peer
    assert!(!manager.state.is_complete());
}

#[test]
fn on_packet_ack_server() {
    let mut publisher = Publisher::snapshot();
    let mut path = MockDcPath::default();
    let tokens = [TEST_TOKEN_1, TEST_TOKEN_2];
    path.stateless_reset_tokens.extend(tokens);
    let mut manager: Manager<Server> = Manager::new(Some(path), 1, &mut publisher);
    on_packet_ack(&mut manager, tokens.as_slice(), &mut publisher);

    // Server completes when its stateless reset tokens are acked
    assert!(manager.state.is_complete());
}

fn on_packet_ack<Config, Endpoint>(
    manager: &mut Manager<Config>,
    tokens: &[stateless_reset::Token],
    publisher: &mut Publisher,
) where
    Config: endpoint::Config<DcEndpoint = Endpoint>,
    Endpoint: dc::Endpoint<Path = MockDcPath>,
{
    let expected_frame =
        Frame::DcStatelessResetTokens(DcStatelessResetTokens::new(tokens).unwrap());

    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut context = MockWriteContext::new(
        now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Client,
    );
    let pn = context.packet_number();

    manager.on_path_secrets_ready(&Session, publisher);

    if Config::ENDPOINT_TYPE.is_server() {
        // Receive tokens on the server to trigger sending
        manager.on_peer_dc_stateless_reset_tokens([TEST_TOKEN_3].iter(), publisher);
    }
    assert!(manager.has_transmission_interest());

    manager.on_transmit(&mut context);
    // We no longer have transmission interest, but DC_STATELESS_RESET_TOKENS will still
    // be transmitted passively until one is acked
    assert!(!manager.has_transmission_interest());
    assert_eq!(
        expected_frame,
        context.frame_buffer.pop_front().unwrap().as_frame()
    );

    manager.on_transmit(&mut context);

    // Same DC_STATELESS_RESET_TOKENS frame is written
    assert_eq!(
        expected_frame,
        context.frame_buffer.pop_front().unwrap().as_frame()
    );

    // Ack the first one
    manager.on_packet_ack(&PacketNumberRange::new(pn, pn), publisher);

    assert!(!manager.has_transmission_interest());
}

#[test]
fn on_packet_loss() {
    let mut publisher = Publisher::snapshot();
    let mut path = MockDcPath::default();
    let tokens = [TEST_TOKEN_1, TEST_TOKEN_2];
    path.stateless_reset_tokens.extend(tokens);
    let mut manager: Manager<Client> = Manager::new(Some(path), 1, &mut publisher);
    let mut frame_buffer = OutgoingFrameBuffer::new();
    let mut context = MockWriteContext::new(
        now(),
        &mut frame_buffer,
        transmission::Constraint::None,
        transmission::Mode::Normal,
        endpoint::Type::Client,
    );
    let pn = context.packet_number();

    manager.on_path_secrets_ready(&Session, &mut publisher);
    assert!(manager.has_transmission_interest());

    manager.on_transmit(&mut context);
    assert!(!manager.has_transmission_interest());

    // DC_STATELESS_RESET_TOKENS frame was lost
    manager.on_packet_loss(&PacketNumberRange::new(pn, pn));

    // so now we have transmission interest again
    assert!(manager.has_transmission_interest());
}

#[test]
#[cfg_attr(miri, ignore)]
fn snapshots() {
    assert_debug_snapshot!(State::test_transitions());
}

#[test]
#[cfg_attr(miri, ignore)]
fn dot_test() {
    assert_snapshot!(State::dot());
}

/// Creates an application space packet number with the given value
fn pn(nr: usize) -> PacketNumber {
    PacketNumberSpace::ApplicationData.new_packet_number(VarInt::new(nr as u64).unwrap())
}
