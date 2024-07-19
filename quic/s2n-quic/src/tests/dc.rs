// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
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
    event::{api::DcState, Timestamp},
    frame::ConnectionClose,
    stateless_reset,
    stateless_reset::token::testing::{TEST_TOKEN_1, TEST_TOKEN_2},
    transport,
    varint::VarInt,
};

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

    self_test(server, client, None, None)
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

    self_test(server, client, None, None)
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

    self_test(server, client, Some(expected_client_error), None)
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
        Some(connection::Error::invalid_configuration(
            "peer does not support specified dc versions",
        )),
        Some(expected_server_error),
    )
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
        Some(expected_client_error),
        Some(connection::Error::invalid_configuration(
            "peer does not support specified dc versions",
        )),
    )
}

fn self_test<S: ServerProviders, C: ClientProviders>(
    server: server::Builder<S>,
    client: client::Builder<C>,
    expected_client_error: Option<connection::Error>,
    expected_server_error: Option<connection::Error>,
) -> Result<()> {
    let model = Model::default();
    let rtt = Duration::from_millis(100);
    model.set_delay(rtt / 2);

    let server_subscriber = DcStateChanged::new();
    let server_events = server_subscriber.clone();
    let client_subscriber = DcStateChanged::new();
    let client_events = client_subscriber.clone();

    test(model, |handle| {
        let mut server = server
            .with_io(handle.builder().build()?)?
            .with_event((dc::ConfirmComplete, (tracing_events(), server_subscriber)))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = server.local_addr()?;

        spawn(async move {
            if let Some(mut conn) = server.accept().await {
                let result = dc::ConfirmComplete::wait_ready(&mut conn).await;

                if let Some(error) = expected_server_error {
                    assert_eq!(error, convert_io_result(result).unwrap());

                    if expected_client_error.is_some() {
                        conn.close(SERVER_CLOSE_ERROR_CODE.into());
                    }
                } else {
                    assert!(result.is_ok());
                }
            }
        });

        let client = client
            .with_io(handle.builder().build().unwrap())?
            .with_event((dc::ConfirmComplete, (tracing_events(), client_subscriber)))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let client_events = client_events.clone();

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
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
                let client_events = client_events.events().lock().unwrap().clone();
                assert_dc_complete(&client_events);
                // wait briefly so the ack for the `DC_STATELESS_RESET_TOKENS` frame from the server is sent
                // before the client closes the connection. This is only necessary to confirm the `dc::State`
                // on the server moves to `DcState::Complete`
                delay(Duration::from_millis(100)).await;
            }
        });

        Ok(addr)
    })
    .unwrap();

    if expected_client_error.is_some() || expected_server_error.is_some() {
        return Ok(());
    }

    let server_events = server_events.events().lock().unwrap().clone();
    let client_events = client_events.events().lock().unwrap().clone();

    assert_dc_complete(&server_events);
    assert_dc_complete(&client_events);

    // Server path secrets are ready in 1.5 RTTs measured from the start of the test, since it takes
    // .5 RTT for the Initial from the client to reach the server
    assert_eq!(
        // remove floating point division error
        Duration::from_millis(rtt.mul_f32(1.5).as_millis() as u64),
        server_events[1].timestamp.duration_since_start()
    );
    assert_eq!(rtt, client_events[1].timestamp.duration_since_start());

    // Server completes in 2.5 RTTs measured from the start of the test, since it takes .5 RTT
    // for the Initial from the client to reach the server
    assert_eq!(
        rtt.mul_f32(2.5),
        server_events[2].timestamp.duration_since_start()
    );
    assert_eq!(rtt * 2, client_events[2].timestamp.duration_since_start());

    Ok(())
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

#[derive(Clone, Default)]
struct DcStateChanged {
    pub events: Arc<Mutex<Vec<DcStateChangedEvent>>>,
}
impl DcStateChanged {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn events(&self) -> Arc<Mutex<Vec<DcStateChangedEvent>>> {
        self.events.clone()
    }
}

impl events::Subscriber for DcStateChanged {
    type ConnectionContext = DcStateChanged;

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
        let mut buffer = context.events.lock().unwrap();
        store(event, &mut buffer);
    }
}
