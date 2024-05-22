// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic_core::{
    dc::testing::MockDcEndpoint,
    event::{api::DcState, Timestamp},
    stateless_reset::token::testing::{TEST_TOKEN_1, TEST_TOKEN_2},
};

// Client                                                                    Server
//
// Initial[0]: CRYPTO[CH (DC_SUPPORTED_VERSIONS[3,2,1]] ->
//
//                                      # dc_state_changed: state=VersionNegotiated
//                                                    Initial[0]: CRYPTO[SH] ACK[0]
//                Handshake[0]: CRYPTO[EE (DC_SUPPORTED_VERSIONS[3], CERT, CV, FIN]
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
//               <- 1-RTT[1]: HANDSHAKE_DONE, ACK[0], DC_STATELESS_RESET_TOKENS[..]
//
// # handshake_status_updated: status=HandshakeDoneAcked
// # handshake_status_updated: status=Confirmed
// # key_space_discarded: space=Handshake
// # dc_state_changed: state=Complete
//
// 1-RTT[1]: ACK[1] ->
//                            # handshake_status_updated: status=HandshakeDoneAcked
//                                               # dc_state_changed: state=Complete
#[test]
fn dc_mtls_handshake_self_test() {
    let model = Model::default();
    let rtt = Duration::from_millis(100);
    model.set_delay(rtt / 2);
    const LEN: usize = 1000;

    let server_subscriber = DcStateChanged::new();
    let server_events = server_subscriber.clone();
    let client_subscriber = DcStateChanged::new();
    let client_events = client_subscriber.clone();
    let server_tokens = [TEST_TOKEN_1];
    let client_tokens = [TEST_TOKEN_2];

    test(model, |handle| {
        let server_tls = build_server_mtls_provider(certificates::MTLS_CA_CERT)?;
        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(server_tls)?
            .with_event((tracing_events(), server_subscriber))?
            .with_random(Random::with_seed(456))?
            .with_dc(MockDcEndpoint::new(&server_tokens))?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            let mut conn = server.accept().await.unwrap();
            let mut stream = conn.open_bidirectional_stream().await.unwrap();
            stream.send(vec![42; LEN].into()).await.unwrap();
            stream.flush().await.unwrap();
        });

        let client_tls = build_client_mtls_provider(certificates::MTLS_CA_CERT)?;
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(client_tls)?
            .with_event((tracing_events(), client_subscriber))?
            .with_random(Random::with_seed(456))?
            .with_dc(MockDcEndpoint::new(&client_tokens))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let mut conn = client.connect(connect).await.unwrap();
            let mut stream = conn.accept_bidirectional_stream().await.unwrap().unwrap();

            let mut recv_len = 0;
            while let Some(chunk) = stream.receive().await.unwrap() {
                recv_len += chunk.len();
            }
            assert_eq!(LEN, recv_len);
        });

        Ok(addr)
    })
    .unwrap();

    let server_events = server_events.events().lock().unwrap().clone();
    let client_events = client_events.events().lock().unwrap().clone();

    // 3 state transitions (VersionNegotiated -> PathSecretsReady -> Complete)
    assert_eq!(3, server_events.len());
    assert_eq!(3, client_events.len());

    for events in [server_events.clone(), client_events.clone()] {
        if let DcState::VersionNegotiated { version, .. } = events[0].state {
            assert_eq!(version, s2n_quic_core::dc::SUPPORTED_VERSIONS[0]);
        } else {
            panic!("VersionNegotiated should be the first dc state");
        }

        assert!(matches!(events[1].state, DcState::PathSecretsReady { .. }));
        assert!(matches!(events[2].state, DcState::Complete { .. }));
    }

    // Server completes in 2.5 RTTs measured from the start of the test, since it takes .5 RTT
    // for the Initial from the client to reach the server
    assert_eq!(
        rtt.mul_f32(2.5),
        server_events[2].timestamp.duration_since_start()
    );
    assert_eq!(rtt * 2, client_events[2].timestamp.duration_since_start());
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
