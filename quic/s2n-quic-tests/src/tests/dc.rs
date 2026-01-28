// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_codec::DecoderBufferMut;
use s2n_quic::{
    client,
    client::ClientProviders,
    connection,
    provider::{dc, io::testing::Result},
    server,
    server::ServerProviders,
};
use s2n_quic_core::{
    crypto::tls,
    dc::testing::MockDcEndpoint,
    event::{
        api::{
            ConnectionMeta, DatagramDropReason, DcState, EndpointDatagramDropped, EndpointMeta,
            MtuUpdated, Subject,
        },
        metrics::aggregate,
        Timestamp,
    },
    frame::ConnectionClose,
    packet::interceptor::{Datagram, Interceptor},
    stateless_reset::{
        self,
        token::testing::{TEST_TOKEN_1, TEST_TOKEN_2},
    },
    transport,
    varint::VarInt,
};
use std::sync::atomic::Ordering;

const SERVER_TOKENS: [stateless_reset::Token; 1] = [TEST_TOKEN_1];
const CLIENT_TOKENS: [stateless_reset::Token; 1] = [TEST_TOKEN_2];
const SERVER_CLOSE_ERROR_CODE: VarInt = VarInt::from_u8(111);
const CLIENT_CLOSE_ERROR_CODE: VarInt = VarInt::from_u8(222);

// Client                                                                    Server
//
// Initial[0]: CRYPTO[CH (DC_SUPPORTED_VERSIONS[3,2,1])] ->
//
//                                      # dc_state_changed: state=VersionNegotiated
//                                                    Initial[0]: CRYPTO[SH] ACK[0]
//               Handshake[0]: CRYPTO[EE (DC_SUPPORTED_VERSIONS[3]), CERT, CV, FIN]
//
// # dc_state_changed: state=VersionNegotiated
// # handshake_status_updated: status=Complete
// # dc_state_changed: state=PathSecretsReady
// Initial[1]: ACK[0]
// Handshake[0]: CRYPTO[FIN], ACK[0] ->
// 1-RTT[0]: DC_STATELESS_RESET_TOKENS[..]
//
//                                       # dc_state_changed: state=PathSecretsReady
//                                      # handshake_status_updated: status=Complete
//                                     # handshake_status_updated: status=Confirmed
//                                           # key_space_discarded: space=Handshake
//               <- 1-RTT[1]: HANDSHAKE_DONE, ACK[0], DC_STATELESS_RESET_TOKENS[..]
//
// # handshake_status_updated: status=HandshakeDoneAcked
// # handshake_status_updated: status=Confirmed
// # key_space_discarded: space=Handshake
// # dc_state_changed: state=Complete
// 1-RTT[1]: ACK[1] ->
//                            # handshake_status_updated: status=HandshakeDoneAcked
//                                               # dc_state_changed: state=Complete
#[test]
fn dc_handshake_self_test() -> Result<()> {
    let server = Server::builder()
        .with_tls(SERVER_CERTS)?
        .with_dc(MockDcEndpoint::new(&SERVER_TOKENS))?;
    let client = Client::builder()
        .with_tls(certificates::CERT_PEM)?
        .with_dc(MockDcEndpoint::new(&CLIENT_TOKENS))?;

    self_test(server, client, true, None, None, true)?;

    Ok(())
}

// Client                                                                    Server
//
// Initial[0]: CRYPTO[CH (DC_SUPPORTED_VERSIONS[3,2,1])] ->
//
//                                      # dc_state_changed: state=VersionNegotiated
//                                                    Initial[0]: CRYPTO[SH] ACK[0]
//               Handshake[0]: CRYPTO[EE (DC_SUPPORTED_VERSIONS[3]), CERT, CV, FIN]
//
// # dc_state_changed: state=VersionNegotiated
// # handshake_status_updated: status=Complete
// # dc_state_changed: state=PathSecretsReady
// Initial[1]: ACK[0]
// Handshake[0]: CRYPTO[CERT, CV, FIN], ACK[0] ->
// 1-RTT[0]: DC_STATELESS_RESET_TOKENS[..]
//
//                                       # dc_state_changed: state=PathSecretsReady
//                                      # handshake_status_updated: status=Complete
//                                     # handshake_status_updated: status=Confirmed
//                                           # key_space_discarded: space=Handshake
//               <- 1-RTT[1]: HANDSHAKE_DONE, ACK[0], DC_STATELESS_RESET_TOKENS[..]
//
// # handshake_status_updated: status=HandshakeDoneAcked
// # handshake_status_updated: status=Confirmed
// # key_space_discarded: space=Handshake
// # dc_state_changed: state=Complete
// 1-RTT[1]: ACK[1] ->
//                            # handshake_status_updated: status=HandshakeDoneAcked
//                                               # dc_state_changed: state=Complete
#[test]
fn dc_mtls_handshake_self_test() -> Result<()> {
    let server_tls = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
    let server = Server::builder()
        .with_tls(server_tls)?
        .with_dc(MockDcEndpoint::new(&SERVER_TOKENS))?;

    let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
    let client = Client::builder()
        .with_tls(client_tls)?
        .with_dc(MockDcEndpoint::new(&SERVER_TOKENS))?;

    self_test(server, client, true, None, None, true)?;

    Ok(())
}

#[test]
fn dc_mtls_handshake_auth_failure_self_test() -> Result<()> {
    let server_tls = build_server_mtls_provider(certificates::UNTRUSTED_CERT_PEM)?;
    let server = Server::builder()
        .with_tls(server_tls)?
        .with_dc(MockDcEndpoint::new(&CLIENT_TOKENS))?;

    let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
    let client = Client::builder()
        .with_tls(client_tls)?
        .with_dc(MockDcEndpoint::new(&SERVER_TOKENS))?;

    // convert from a ConnectionClose frame so the initiator is `Remote`
    let expected_client_error = ConnectionClose {
        error_code: transport::Error::crypto_error(tls::Error::HANDSHAKE_FAILURE.code)
            .code
            .as_u64()
            .try_into()
            .unwrap(),
        frame_type: Some(VarInt::ZERO),
        reason: None,
    }
    .into();

    self_test(
        server,
        client,
        true,
        Some(expected_client_error),
        None,
        true,
    )?;

    Ok(())
}

// Client                                                                    Server
//
// Initial[0]: CRYPTO[CH (DC_SUPPORTED_VERSIONS[3,2,1])] ->
//
//                                                    Initial[0]: CRYPTO[SH] ACK[0]
//                                       <- Handshake[0]: CRYPTO[EE, CERT, CV, FIN]
//
// # dc_state_changed: state=NoVersionNegotiated
// # handshake_status_updated: status=Complete
// Initial[1]: ACK[0]
// 1-RTT[0]: connection_closed: error=Application(222)
#[test]
fn dc_mtls_handshake_server_not_supported_self_test() -> Result<()> {
    // No dc Provider configured on the server
    let server_tls = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
    let server = Server::builder().with_tls(server_tls)?;

    let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
    let client = Client::builder()
        .with_tls(client_tls)?
        .with_dc(MockDcEndpoint::new(&SERVER_TOKENS))?;

    // convert from a ConnectionClose frame so the initiator is `Remote`
    let expected_server_error = ConnectionClose {
        error_code: CLIENT_CLOSE_ERROR_CODE,
        frame_type: None,
        reason: None,
    }
    .into();

    self_test(
        server,
        client,
        true,
        Some(connection::Error::invalid_configuration(
            "peer does not support specified dc versions",
        )),
        Some(expected_server_error),
        true,
    )?;

    Ok(())
}

// Client                                                                    Server
//
// Initial[0]: CRYPTO[CH] ->
//
//                                    # dc_state_changed: state=NoVersionNegotiated
//                                                    Initial[0]: CRYPTO[SH] ACK[0]
//                                          Handshake[0]: CRYPTO[EE, CERT, CV, FIN]
//
// # handshake_status_updated: status=Complete
// Initial[1]: ACK[0]
// Handshake[0]: CRYPTO[CERT, CV, FIN], ACK[0] ->
//
//                                      # handshake_status_updated: status=Complete
//                                     # handshake_status_updated: status=Confirmed
//                                           # key_space_discarded: space=Handshake
//                           <- 1-RTT[0]: connection_closed: error=Application(111)
#[test]
fn dc_mtls_handshake_client_not_supported_self_test() -> Result<()> {
    let server_tls = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
    let server = Server::builder()
        .with_tls(server_tls)?
        .with_dc(MockDcEndpoint::new(&CLIENT_TOKENS))?;

    // No dc Provider configured on the client
    let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
    let client = Client::builder().with_tls(client_tls)?;

    // convert from a ConnectionClose frame so the initiator is `Remote`
    let expected_client_error = ConnectionClose {
        error_code: SERVER_CLOSE_ERROR_CODE,
        frame_type: None,
        reason: None,
    }
    .into();

    self_test(
        server,
        client,
        false,
        Some(expected_client_error),
        Some(connection::Error::invalid_configuration(
            "peer does not support specified dc versions",
        )),
        true,
    )?;

    Ok(())
}

// Test that verifies MtuProbingComplete frames are exchanged between client and server
// when DC is enabled and MTU probing completes.
//
// Client                                                                    Server
//
// <MTU probing completes locally> ->
// 1-RTT: MTU_PROBING_COMPLETE[mtu=1472]
//                                                  # on_mtu_probing_complete_received
//                                      <- 1-RTT: MTU_PROBING_COMPLETE[mtu=1472]
// # on_mtu_probing_complete_received
#[test]
fn mtu_probing_complete_frame_exchange_test() -> Result<()> {
    let server_tls = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
    let server = Server::builder()
        .with_tls(server_tls)?
        .with_dc(MockDcEndpoint::new(&SERVER_TOKENS))?;

    let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
    let client = Client::builder()
        .with_tls(client_tls)?
        .with_dc(MockDcEndpoint::new(&CLIENT_TOKENS))?;

    let (client_events, server_events) = self_test(server, client, true, None, None, true)?;

    // Verify that client received MtuProbingComplete from server
    let client_mtu_complete_events = client_events
        .mtu_probing_complete_received_events()
        .lock()
        .unwrap()
        .clone();
    assert_eq!(
        1,
        client_mtu_complete_events.len(),
        "Client should receive one MtuProbingComplete frame from server"
    );
    // Verify the MTU value matches the confirmed MTU (1472 = 1500 - headers)
    assert_eq!(
        1472, client_mtu_complete_events[0].mtu,
        "Received MTU should match confirmed MTU"
    );

    // Verify that server received MtuProbingComplete from client
    let server_mtu_complete_events = server_events
        .mtu_probing_complete_received_events()
        .lock()
        .unwrap()
        .clone();
    assert_eq!(
        1,
        server_mtu_complete_events.len(),
        "Server should receive one MtuProbingComplete frame from client"
    );
    assert_eq!(
        1472, server_mtu_complete_events[0].mtu,
        "Received MTU should match confirmed MTU"
    );

    // Verify both sides completed their local MTU probing
    let client_mtu_events = client_events.mtu_updated_events.lock().unwrap().clone();
    let server_mtu_events = server_events.mtu_updated_events.lock().unwrap().clone();

    assert!(
        client_mtu_events
            .iter()
            .any(|event| event.search_complete && event.mtu == 1472),
        "Client should have completed MTU probing at 1472"
    );
    assert!(
        server_mtu_events
            .iter()
            .any(|event| event.search_complete && event.mtu == 1472),
        "Server should have completed MTU probing at 1472"
    );

    // Verify that both sides correctly received mtu_probing_complete_support=true from peer
    // This ensures the transport parameter was correctly encoded and decoded
    let client_received_peer_support = client_events
        .peer_mtu_probing_complete_support()
        .lock()
        .unwrap()
        .expect("Client should have received transport parameters");
    assert!(
        client_received_peer_support,
        "Client should receive mtu_probing_complete_support=true from server"
    );

    let server_received_peer_support = server_events
        .peer_mtu_probing_complete_support()
        .lock()
        .unwrap()
        .expect("Server should have received transport parameters");
    assert!(
        server_received_peer_support,
        "Server should receive mtu_probing_complete_support=true from client"
    );

    Ok(())
}

// Test that verifies asymmetric MtuProbingComplete frame exchange when only the server
// has mtu_probing_complete_support enabled but the client has it disabled.
//
// Client (disabled)                                                    Server (enabled)
//
// <MTU probing completes locally>
// (no frame sent) ->
//                                                                      <- 1-RTT: MTU_PROBING_COMPLETE[mtu=1472]
// # on_mtu_probing_complete_received
//                                                 (no frame received on server)
#[test]
fn mtu_probing_complete_server_only_test() -> Result<()> {
    // Server with mtu_probing_complete_support enabled (default)
    let server_tls = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
    let server = Server::builder()
        .with_tls(server_tls)?
        .with_dc(MockDcEndpoint::new(&SERVER_TOKENS))?;

    // Client with mtu_probing_complete_support DISABLED
    let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
    let client = Client::builder()
        .with_tls(client_tls)?
        .with_dc(MockDcEndpoint::new(&CLIENT_TOKENS).with_mtu_probing_complete_support(false))?;

    let (client_events, server_events) =
        self_test_with_mtu(server, client, true, None, None, false, None)?;

    // Verify that client received MtuProbingComplete from server
    let client_mtu_complete_events = client_events
        .mtu_probing_complete_received_events()
        .lock()
        .unwrap()
        .clone();
    assert_eq!(
        1,
        client_mtu_complete_events.len(),
        "Client should receive one MtuProbingComplete frame from server"
    );
    assert_eq!(
        1472, client_mtu_complete_events[0].mtu,
        "Received MTU should match confirmed MTU"
    );

    // Verify that server did NOT receive MtuProbingComplete from client
    let server_mtu_complete_events = server_events
        .mtu_probing_complete_received_events()
        .lock()
        .unwrap()
        .clone();
    assert_eq!(
        0,
        server_mtu_complete_events.len(),
        "Server should NOT receive any MtuProbingComplete frame from client (client has it disabled)"
    );

    // Verify both sides still completed their local MTU probing
    let client_mtu_events = client_events.mtu_updated_events.lock().unwrap().clone();
    let server_mtu_events = server_events.mtu_updated_events.lock().unwrap().clone();

    // Check that server's wait_ready didn't wait for client's frame
    // Since client has mtu_probing_complete_support disabled, the server should complete
    // wait_ready immediately after its own local MTU probing completes, not wait for a
    // frame from the client (which will never arrive).

    // If the server were incorrectly waiting for the client's frame,
    // the test would take 10 seconds to timeout.

    let server_local_complete_time = server_mtu_events
        .iter()
        .find(|event| event.search_complete && event.mtu == 1472)
        .unwrap()
        .timestamp;

    let client_local_complete_time = client_mtu_events
        .iter()
        .find(|event| event.search_complete && event.mtu == 1472)
        .unwrap()
        .timestamp;

    let server_duration = server_local_complete_time.duration_since_start();
    let client_duration = client_local_complete_time.duration_since_start();

    let time_difference = if server_duration > client_duration {
        server_duration.saturating_sub(client_duration)
    } else {
        client_duration.saturating_sub(server_duration)
    };

    // Server should complete MTU probing independently of the client.
    // Both should complete around the same time.
    // If server were waiting for client's frame, there would be a significant additional delay.
    assert!(time_difference < Duration::from_millis(100));

    // Verify that the transport parameter was correctly encoded and decoded
    let server_received_peer_support = server_events
        .peer_mtu_probing_complete_support()
        .lock()
        .unwrap()
        .unwrap();
    assert!(!server_received_peer_support);

    // Client should receive mtu_probing_complete_support=true from server (server has it enabled)
    let client_received_peer_support = client_events
        .peer_mtu_probing_complete_support()
        .lock()
        .unwrap()
        .expect("Client should have received transport parameters");
    assert!(
        client_received_peer_support,
        "Client should receive mtu_probing_complete_support=true from server"
    );

    Ok(())
}

// Test that verifies MtuProbingComplete frames are exchanged between client and server
// when DC is enabled and MTU probing completes with jumbo frames (9000 byte MTU).
// This specifically tests the scenario where the first probe at max MTU succeeds.
//
// Client                                                                    Server
//
// <MTU probing completes locally with first probe success at 9000 bytes> ->
// 1-RTT: MTU_PROBING_COMPLETE[mtu=8972]
//                                                  # on_mtu_probing_complete_received
//                                      <- 1-RTT: MTU_PROBING_COMPLETE[mtu=8972]
// # on_mtu_probing_complete_received
#[test]
fn mtu_probing_complete_frame_exchange_jumbo_mtu_test() -> Result<()> {
    let server_tls = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
    let server = Server::builder()
        .with_tls(server_tls)?
        .with_dc(MockDcEndpoint::new(&SERVER_TOKENS))?;

    let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
    let client = Client::builder()
        .with_tls(client_tls)?
        .with_dc(MockDcEndpoint::new(&CLIENT_TOKENS))?;

    // Use 9000 byte MTU for jumbo frames
    let (client_events, server_events) =
        self_test_with_mtu(server, client, true, None, None, true, Some(9000))?;

    let expected_mtu = 8972;

    // Verify that client received MtuProbingComplete from server
    let client_mtu_complete_events = client_events
        .mtu_probing_complete_received_events()
        .lock()
        .unwrap()
        .clone();
    assert_eq!(
        1,
        client_mtu_complete_events.len(),
        "Client should receive one MtuProbingComplete frame from server"
    );
    // Verify the MTU value matches the confirmed MTU (8972 = 9000 - headers)
    assert_eq!(
        expected_mtu, client_mtu_complete_events[0].mtu,
        "Received MTU should match confirmed MTU for jumbo frames"
    );

    // Verify that server received MtuProbingComplete from client
    let server_mtu_complete_events = server_events
        .mtu_probing_complete_received_events()
        .lock()
        .unwrap()
        .clone();
    assert_eq!(
        1,
        server_mtu_complete_events.len(),
        "Server should receive one MtuProbingComplete frame from client"
    );
    assert_eq!(
        expected_mtu, server_mtu_complete_events[0].mtu,
        "Received MTU should match confirmed MTU for jumbo frames"
    );

    // Verify both sides completed their local MTU probing
    let client_mtu_events = client_events.mtu_updated_events.lock().unwrap().clone();
    let server_mtu_events = server_events.mtu_updated_events.lock().unwrap().clone();

    assert!(
        client_mtu_events
            .iter()
            .any(|event| event.search_complete && event.mtu == expected_mtu),
        "Client should have completed MTU probing at {expected_mtu}"
    );
    assert!(
        server_mtu_events
            .iter()
            .any(|event| event.search_complete && event.mtu == expected_mtu),
        "Server should have completed MTU probing at {expected_mtu}"
    );

    // Verify that both sides correctly received mtu_probing_complete_support=true from peer
    let client_received_peer_support = client_events
        .peer_mtu_probing_complete_support()
        .lock()
        .unwrap()
        .expect("Client should have received transport parameters");
    assert!(
        client_received_peer_support,
        "Client should receive mtu_probing_complete_support=true from server"
    );

    let server_received_peer_support = server_events
        .peer_mtu_probing_complete_support()
        .lock()
        .unwrap()
        .expect("Server should have received transport parameters");
    assert!(
        server_received_peer_support,
        "Server should receive mtu_probing_complete_support=true from client"
    );

    Ok(())
}

#[test]
fn dc_secret_control_packet() -> Result<()> {
    dc_possible_secret_control_packet(|| true)
}

#[test]
fn dc_not_secret_control_packet() -> Result<()> {
    dc_possible_secret_control_packet(|| false)
}

fn dc_possible_secret_control_packet(
    on_possible_secret_control_packet: fn() -> bool,
) -> Result<()> {
    let server_tls = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
    let server = Server::builder()
        .with_tls(server_tls)?
        .with_dc(MockDcEndpoint::new(&SERVER_TOKENS))?;

    let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
    let mut dc_endpoint = MockDcEndpoint::new(&CLIENT_TOKENS);
    dc_endpoint.on_possible_secret_control_packet = on_possible_secret_control_packet;
    let on_possible_secret_control_packet_count =
        dc_endpoint.on_possible_secret_control_packet_count.clone();

    let client = Client::builder()
        .with_tls(client_tls)?
        .with_dc(dc_endpoint)?
        .with_packet_interceptor(RandomShort::default())?;

    let (client_events, _server_events) = self_test(server, client, true, None, None, false)?;

    assert_eq!(
        1,
        on_possible_secret_control_packet_count.load(Ordering::Relaxed)
    );

    let client_datagram_drops = client_events
        .endpoint_datagram_dropped_events
        .lock()
        .unwrap();

    if on_possible_secret_control_packet() {
        // No datagrams should be recorded as dropped because MockDcEndpoint::on_possible_secret_control_packet
        // returned true, indicating the given datagram was a secret control packet
        assert_eq!(0, client_datagram_drops.len());
    } else {
        // The datagram was not a secret control packet, so it is dropped
        assert_eq!(1, client_datagram_drops.len());
        assert!(matches!(
            client_datagram_drops[0].reason,
            DatagramDropReason::UnknownDestinationConnectionId { .. }
        ));
    }

    Ok(())
}

#[track_caller]
fn self_test<S: ServerProviders, C: ClientProviders>(
    server: server::Builder<S>,
    client: client::Builder<C>,
    client_has_dc: bool,
    expected_client_error: Option<connection::Error>,
    expected_server_error: Option<connection::Error>,
    with_blocklist: bool,
) -> Result<(DcRecorder, DcRecorder)> {
    self_test_with_mtu(
        server,
        client,
        client_has_dc,
        expected_client_error,
        expected_server_error,
        with_blocklist,
        None,
    )
}

#[track_caller]
fn self_test_with_mtu<S: ServerProviders, C: ClientProviders>(
    server: server::Builder<S>,
    client: client::Builder<C>,
    client_has_dc: bool,
    expected_client_error: Option<connection::Error>,
    expected_server_error: Option<connection::Error>,
    with_blocklist: bool,
    max_mtu: Option<u16>,
) -> Result<(DcRecorder, DcRecorder)> {
    let model = Model::default();
    let rtt = Duration::from_millis(100);
    model.set_delay(rtt / 2);

    if let Some(max_mtu) = max_mtu {
        model.set_max_udp_payload(max_mtu);
    }

    let server_subscriber = DcRecorder::new();
    let server_events = server_subscriber.clone();
    let client_subscriber = DcRecorder::new();
    let client_events = client_subscriber.clone();

    test(model.clone(), |handle| {
        let metrics = aggregate::testing::Registry::snapshot();

        let server_event = (
            (
                (dc::ConfirmComplete, dc::MtuConfirmComplete),
                metrics.subscriber("server"),
            ),
            (
                tracing_events(with_blocklist, model.clone()),
                server_subscriber,
            ),
        );

        let server_io_builder = handle.builder();
        let server_io_builder = if let Some(mtu) = max_mtu {
            server_io_builder
                .with_max_mtu(mtu)
                .with_base_mtu(mtu)
                .with_initial_mtu(mtu)
        } else {
            server_io_builder
        };

        let mut server = server
            .with_io(server_io_builder.build()?)?
            .with_event(server_event)?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = server.local_addr()?;

        let expected_count = 1 + client_has_dc as usize;
        spawn(async move {
            for _ in 0..expected_count {
                if let Some(mut conn) = server.accept().await {
                    let result = dc::ConfirmComplete::wait_ready(&mut conn).await;

                    if let Some(error) = expected_server_error {
                        assert_eq!(error, convert_io_result(result).unwrap());

                        if expected_client_error.is_some() {
                            conn.close(SERVER_CLOSE_ERROR_CODE.into());
                        }
                    } else {
                        assert!(result.is_ok());
                        dc::MtuConfirmComplete::wait_ready(&mut conn).await;
                    }
                }
            }
        });

        let client_event = (
            (
                (dc::ConfirmComplete, dc::MtuConfirmComplete),
                metrics.subscriber("client"),
            ),
            (
                tracing_events(with_blocklist, model.clone()),
                client_subscriber,
            ),
        );

        let client_io_builder = handle.builder();
        let client_io_builder = if let Some(mtu) = max_mtu {
            client_io_builder
                .with_max_mtu(mtu)
                .with_base_mtu(mtu)
                .with_initial_mtu(mtu)
        } else {
            client_io_builder
        };

        let client = client
            .with_io(client_io_builder.build().unwrap())?
            .with_event(client_event)?
            .with_random(Random::with_seed(456))?
            .start()?;

        for _ in 0..expected_count {
            primary::spawn({
                let client = client.clone();
                let client_events = client_events.clone();
                async move {
                    let connect = Connect::new(addr)
                        .with_server_name("localhost")
                        .with_deduplicate(client_has_dc);
                    let mut conn = client.connect(connect).await.unwrap();
                    let result = dc::ConfirmComplete::wait_ready(&mut conn).await;

                    if let Some(error) = expected_client_error {
                        assert_eq!(error, convert_io_result(result).unwrap());

                        if expected_server_error.is_some() {
                            conn.close(CLIENT_CLOSE_ERROR_CODE.into());
                            // wait for the server to assert the expected error before dropping
                            delay(Duration::from_millis(100)).await;
                        }
                    } else {
                        assert!(result.is_ok());
                        let client_events = client_events
                            .dc_state_changed_events()
                            .lock()
                            .unwrap()
                            .clone();
                        assert_dc_complete(&client_events);

                        dc::MtuConfirmComplete::wait_ready(&mut conn).await;

                        // wait briefly for MTU probing to complete on the server
                        delay(Duration::from_millis(100)).await;
                    }
                }
            });
        }

        Ok(addr)
    })
    .unwrap();

    if expected_client_error.is_some() || expected_server_error.is_some() {
        return Ok((client_events, server_events));
    }

    let server_dc_state_changed_events = server_events
        .dc_state_changed_events()
        .lock()
        .unwrap()
        .clone();
    let client_dc_state_changed_events = client_events
        .dc_state_changed_events()
        .lock()
        .unwrap()
        .clone();

    assert_dc_complete(&server_dc_state_changed_events);
    assert_dc_complete(&client_dc_state_changed_events);

    // Server path secrets are ready in 1.5 RTTs measured from the start of the test, since it takes
    // .5 RTT for the Initial from the client to reach the server
    assert_eq!(
        // remove floating point division error
        Duration::from_millis(rtt.mul_f32(1.5).as_millis() as u64),
        server_dc_state_changed_events[1]
            .timestamp
            .duration_since_start()
    );
    assert_eq!(
        rtt,
        client_dc_state_changed_events[1]
            .timestamp
            .duration_since_start()
    );

    // Server completes in 2.5 RTTs measured from the start of the test, since it takes .5 RTT
    // for the Initial from the client to reach the server
    assert_eq!(
        rtt.mul_f32(2.5),
        server_dc_state_changed_events[2]
            .timestamp
            .duration_since_start()
    );
    assert_eq!(
        rtt * 2,
        client_dc_state_changed_events[2]
            .timestamp
            .duration_since_start()
    );

    let client_mtu_events = client_events.mtu_updated_events.lock().unwrap().clone();
    let server_mtu_events = server_events.mtu_updated_events.lock().unwrap().clone();

    // Expected MTU is max_mtu minus IP/UDP headers (28 bytes for IPv4)
    // Default max_mtu is 1500 bytes
    let expected_mtu = max_mtu.unwrap_or(1500) - 28;
    assert_mtu_probing_completed(&client_mtu_events, expected_mtu);
    assert_mtu_probing_completed(&server_mtu_events, expected_mtu);

    Ok((client_events, server_events))
}

fn assert_dc_complete(events: &[DcStateChangedEvent]) {
    // 3 state transitions (VersionNegotiated -> PathSecretsReady -> Complete)
    assert_eq!(3, events.len());

    if let DcState::VersionNegotiated { version, .. } = events[0].state {
        assert_eq!(version, s2n_quic_core::dc::SUPPORTED_VERSIONS[0]);
    } else {
        panic!("VersionNegotiated should be the first dc state");
    }

    assert!(matches!(events[1].state, DcState::PathSecretsReady { .. }));
    assert!(matches!(events[2].state, DcState::Complete { .. }));
}

fn assert_mtu_probing_completed(events: &[MtuUpdatedEvent], mtu: u16) {
    assert!(!events.is_empty());
    let last_event = events.last().unwrap();
    assert!(last_event.search_complete);
    assert_eq!(mtu, last_event.mtu);
}

fn convert_io_result(io_result: std::io::Result<()>) -> Option<connection::Error> {
    io_result
        .err()?
        .into_inner()?
        .downcast::<connection::Error>()
        .ok()
        .as_deref()
        .copied()
}

#[derive(Clone)]
struct DcStateChangedEvent {
    timestamp: Timestamp,
    state: DcState,
}

#[derive(Clone)]
struct MtuUpdatedEvent {
    timestamp: Timestamp,
    mtu: u16,
    search_complete: bool,
}

#[derive(Clone)]
struct MtuProbingCompleteReceivedEvent {
    mtu: u16,
}

#[derive(Clone, Default)]
struct DcRecorder {
    pub dc_state_changed_events: Arc<Mutex<Vec<DcStateChangedEvent>>>,
    pub mtu_updated_events: Arc<Mutex<Vec<MtuUpdatedEvent>>>,
    pub mtu_probing_complete_received_events: Arc<Mutex<Vec<MtuProbingCompleteReceivedEvent>>>,
    pub endpoint_datagram_dropped_events: Arc<Mutex<Vec<EndpointDatagramDropped>>>,
    pub peer_mtu_probing_complete_support: Arc<Mutex<Option<bool>>>,
}
impl DcRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn dc_state_changed_events(&self) -> Arc<Mutex<Vec<DcStateChangedEvent>>> {
        self.dc_state_changed_events.clone()
    }

    pub fn mtu_probing_complete_received_events(
        &self,
    ) -> Arc<Mutex<Vec<MtuProbingCompleteReceivedEvent>>> {
        self.mtu_probing_complete_received_events.clone()
    }

    pub fn peer_mtu_probing_complete_support(&self) -> Arc<Mutex<Option<bool>>> {
        self.peer_mtu_probing_complete_support.clone()
    }
}

impl events::Subscriber for DcRecorder {
    type ConnectionContext = DcRecorder;

    fn create_connection_context(
        &mut self,
        _meta: &events::ConnectionMeta,
        _info: &events::ConnectionInfo,
    ) -> Self::ConnectionContext {
        self.clone()
    }

    fn on_dc_state_changed(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &events::ConnectionMeta,
        event: &events::DcStateChanged,
    ) {
        let store = |event: &events::DcStateChanged, storage: &mut Vec<DcStateChangedEvent>| {
            storage.push(DcStateChangedEvent {
                timestamp: meta.timestamp,
                state: event.state.clone(),
            });
        };
        let mut buffer = context.dc_state_changed_events.lock().unwrap();
        store(event, &mut buffer);
    }

    fn on_mtu_updated(
        &mut self,
        context: &mut Self::ConnectionContext,
        meta: &ConnectionMeta,
        event: &MtuUpdated,
    ) {
        let store = |meta: &ConnectionMeta,
                     event: &events::MtuUpdated,
                     storage: &mut Vec<MtuUpdatedEvent>| {
            storage.push(MtuUpdatedEvent {
                timestamp: meta.timestamp,
                mtu: event.mtu,
                search_complete: event.search_complete,
            });
        };
        let mut buffer = context.mtu_updated_events.lock().unwrap();
        store(meta, event, &mut buffer);
    }

    fn on_mtu_probing_complete_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        event: &events::MtuProbingCompleteReceived,
    ) {
        let store = |event: &events::MtuProbingCompleteReceived,
                     storage: &mut Vec<MtuProbingCompleteReceivedEvent>| {
            storage.push(MtuProbingCompleteReceivedEvent { mtu: event.mtu });
        };
        let mut buffer = context.mtu_probing_complete_received_events.lock().unwrap();
        store(event, &mut buffer);
    }

    fn on_endpoint_datagram_dropped(
        &mut self,
        _meta: &EndpointMeta,
        event: &EndpointDatagramDropped,
    ) {
        self.endpoint_datagram_dropped_events
            .lock()
            .unwrap()
            .push(event.clone());
    }

    fn on_transport_parameters_received(
        &mut self,
        context: &mut Self::ConnectionContext,
        _meta: &ConnectionMeta,
        event: &events::TransportParametersReceived,
    ) {
        let mut buffer = context.peer_mtu_probing_complete_support.lock().unwrap();
        *buffer = Some(event.transport_parameters.mtu_probing_complete_support);
    }
}

/// Replace the first short packet payload with a randomized short packet
#[derive(Default)]
struct RandomShort(bool);

impl Interceptor for RandomShort {
    #[inline]
    fn intercept_rx_datagram<'a>(
        &mut self,
        _subject: &Subject,
        _datagram: &Datagram,
        payload: DecoderBufferMut<'a>,
    ) -> DecoderBufferMut<'a> {
        let payload = payload.into_less_safe_slice();

        if let 0b0100u8..=0b0111u8 = payload[0] >> 4 {
            if !self.0 {
                // randomize everything after the short header tag
                rand::fill_bytes(&mut payload[1..]);
                // only change the first short packet
                self.0 = true;
            }
        }

        DecoderBufferMut::new(payload)
    }
}
