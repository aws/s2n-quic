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
    assert!(manager
        .on_path_secrets_ready(&Session, &mut publisher)
        .is_ok());
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

    assert!(manager
        .on_path_secrets_ready(&Session, &mut publisher)
        .is_ok());

    assert_eq!(1, manager.path().on_path_secrets_ready_count);
    assert!(manager.state.is_path_secrets_ready());
    // Server doesn't transmit until it receives tokens from the client
    assert!(!manager.has_transmission_interest());

    let path = MockDcPath::default();
    let mut manager: Manager<Client> = Manager::new(Some(path), 1, &mut publisher);

    assert!(manager
        .on_path_secrets_ready(&Session, &mut publisher)
        .is_ok());

    assert_eq!(1, manager.path().on_path_secrets_ready_count);
    assert!(manager.state.is_path_secrets_ready());
    // Client starts transmitting as soon as path secrets are ready
    assert!(manager.has_transmission_interest());

    // Calling on_path_secrets_ready again results in an internal error
    assert_eq!(
        manager.on_path_secrets_ready(&Session, &mut publisher),
        Err(transport::Error::INTERNAL_ERROR)
    );
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
    assert!(manager.on_path_secrets_ready(&Session, publisher).is_ok());

    manager.on_peer_dc_stateless_reset_tokens(tokens.iter(), publisher);

    assert_eq!(1, manager.path().on_peer_stateless_reset_tokens_count);
    assert_eq!(
        tokens.as_slice(),
        manager.path().peer_stateless_reset_tokens.as_slice()
    );

    if Config::ENDPOINT_TYPE.is_server() {
        assert!(manager.state.is_server_tokens_sent());
        assert_eq!(0, manager.path().on_dc_handshake_complete);
    } else {
        assert_eq!(1, manager.path().on_dc_handshake_complete);
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
    assert_eq!(0, manager.path().on_dc_handshake_complete);
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
    assert_eq!(1, manager.path().on_dc_handshake_complete);
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

    assert!(manager.on_path_secrets_ready(&Session, publisher).is_ok());

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

    assert!(manager
        .on_path_secrets_ready(&Session, &mut publisher)
        .is_ok());
    assert!(manager.has_transmission_interest());

    manager.on_transmit(&mut context);
    assert!(!manager.has_transmission_interest());

    // DC_STATELESS_RESET_TOKENS frame was lost
    manager.on_packet_loss(&PacketNumberRange::new(pn, pn));

    // so now we have transmission interest again
    assert!(manager.has_transmission_interest());
}

#[test]
fn on_mtu_updated() {
    let mut publisher = Publisher::snapshot();
    let path = MockDcPath::default();
    let mut manager: Manager<Server> = Manager::new(Some(path), 1, &mut publisher);
    manager.on_mtu_updated(1500);

    assert_eq!(1500, manager.path().mtu);
}

/// Records the `state` carried by the `DcStateIncomplete` event, so tests can assert the
/// exact state the handshake was stuck in on close.
#[derive(Default)]
struct IncompleteRecorder {
    state: Option<event::api::DcHandshakeState>,
}

impl event::Subscriber for IncompleteRecorder {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &event::api::ConnectionMeta,
        _info: &event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_dc_state_incomplete(
        &mut self,
        _context: &mut Self::ConnectionContext,
        _meta: &event::api::ConnectionMeta,
        event: &event::api::DcStateIncomplete,
    ) {
        assert!(
            self.state.is_none(),
            "at most one DcStateIncomplete event is expected per connection"
        );
        self.state = Some(event.state.clone());
    }
}

fn connection_meta(endpoint_type: s2n_quic_core::endpoint::Type) -> event::builder::ConnectionMeta {
    event::builder::ConnectionMeta {
        endpoint_type,
        id: 0,
        timestamp: now(),
    }
}

/// A graceful close while the server is stuck in `ServerTokensSent` emits `DcStateIncomplete`
/// reporting that exact state.
#[test]
fn on_close_server_incomplete_graceful() {
    let mut recorder = IncompleteRecorder::default();
    let mut context = ();
    let mut publisher = event::ConnectionPublisherSubscriber::new(
        connection_meta(s2n_quic_core::endpoint::Type::Server),
        1,
        &mut recorder,
        &mut context,
    );

    let mut manager: Manager<Server> = Manager::new(Some(MockDcPath::default()), 1, &mut publisher);
    assert!(manager
        .on_path_secrets_ready(&Session, &mut publisher)
        .is_ok());
    manager.on_peer_dc_stateless_reset_tokens([TEST_TOKEN_1].iter(), &mut publisher);
    assert!(manager.state.is_server_tokens_sent());

    manager.on_close(true, &mut publisher);
    drop(publisher);

    assert!(matches!(
        recorder.state,
        Some(event::api::DcHandshakeState::ServerTokensSent { .. })
    ));
}

/// A graceful close while the client is stuck in `ClientPathSecretsReady` emits `DcStateIncomplete`
/// reporting that exact state.
#[test]
fn on_close_client_incomplete_graceful() {
    let mut recorder = IncompleteRecorder::default();
    let mut context = ();
    let mut publisher = event::ConnectionPublisherSubscriber::new(
        connection_meta(s2n_quic_core::endpoint::Type::Client),
        1,
        &mut recorder,
        &mut context,
    );

    let mut manager: Manager<Client> = Manager::new(Some(MockDcPath::default()), 1, &mut publisher);
    assert!(manager
        .on_path_secrets_ready(&Session, &mut publisher)
        .is_ok());
    assert!(manager.state.is_path_secrets_ready());

    manager.on_close(true, &mut publisher);
    drop(publisher);

    assert!(matches!(
        recorder.state,
        Some(event::api::DcHandshakeState::ClientPathSecretsReady { .. })
    ));
}

/// An error close does not emit `DcStateIncomplete`, even when the dc handshake is incomplete.
#[test]
fn on_close_error_does_not_emit() {
    let mut recorder = IncompleteRecorder::default();
    let mut context = ();
    let mut publisher = event::ConnectionPublisherSubscriber::new(
        connection_meta(s2n_quic_core::endpoint::Type::Server),
        1,
        &mut recorder,
        &mut context,
    );

    let mut manager: Manager<Server> = Manager::new(Some(MockDcPath::default()), 1, &mut publisher);
    assert!(manager
        .on_path_secrets_ready(&Session, &mut publisher)
        .is_ok());
    manager.on_peer_dc_stateless_reset_tokens([TEST_TOKEN_1].iter(), &mut publisher);
    assert!(manager.state.is_server_tokens_sent());

    manager.on_close(false, &mut publisher);
    drop(publisher);

    assert!(recorder.state.is_none());
}

/// A completed dc handshake does not emit `DcStateIncomplete` on close.
#[test]
fn on_close_complete_does_not_emit() {
    let mut recorder = IncompleteRecorder::default();
    let mut context = ();
    let mut publisher = event::ConnectionPublisherSubscriber::new(
        connection_meta(s2n_quic_core::endpoint::Type::Client),
        1,
        &mut recorder,
        &mut context,
    );

    let mut manager: Manager<Client> = Manager::new(Some(MockDcPath::default()), 1, &mut publisher);
    assert!(manager
        .on_path_secrets_ready(&Session, &mut publisher)
        .is_ok());
    // the client completes as soon as it receives the peer's tokens
    manager.on_peer_dc_stateless_reset_tokens([TEST_TOKEN_1].iter(), &mut publisher);
    assert!(manager.state.is_complete());

    manager.on_close(true, &mut publisher);
    drop(publisher);

    assert!(recorder.state.is_none());
}

/// A disabled `dc::Manager` does not emit `DcStateIncomplete` on close.
#[test]
fn on_close_disabled_does_not_emit() {
    let mut recorder = IncompleteRecorder::default();
    let mut context = ();
    let mut publisher = event::ConnectionPublisherSubscriber::new(
        connection_meta(s2n_quic_core::endpoint::Type::Server),
        1,
        &mut recorder,
        &mut context,
    );

    let mut manager: Manager<Server> = Manager::disabled();
    assert!(manager.state.is_complete());

    manager.on_close(true, &mut publisher);
    drop(publisher);

    assert!(recorder.state.is_none());
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
