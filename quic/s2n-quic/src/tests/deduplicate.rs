// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::provider::endpoint_limits::Outcome;

use super::*;
use s2n_quic_core::{dc::testing::MockDcEndpoint, stateless_reset::token::testing::TEST_TOKEN_1};

const LEN: usize = 1_000_000;

#[test]
fn deduplicate_successfully() {
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));

    let server_subscriber = recorder::ConnectionStarted::new();
    let server_events = server_subscriber.events();
    let client_subscriber = recorder::ConnectionStarted::new();
    let client_events = client_subscriber.events();
    test(model, |handle| {
        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(), server_subscriber.clone()))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            let mut conn = server.accept().await.unwrap();
            for _ in 0..2 {
                let mut stream = conn.open_bidirectional_stream().await.unwrap();
                stream.send(vec![42; LEN].into()).await.unwrap();
                stream.flush().await.unwrap();
            }
            let mut conn = server.accept().await.unwrap();
            let mut stream = conn.open_bidirectional_stream().await.unwrap();
            stream.send(vec![42; LEN].into()).await.unwrap();
            stream.flush().await.unwrap();
        });

        let mut server2 = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(), server_subscriber))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr2 = server2.local_addr()?;
        spawn(async move {
            let mut conn = server2.accept().await.unwrap();
            let mut stream = conn.open_bidirectional_stream().await.unwrap();
            stream.send(vec![42; LEN].into()).await.unwrap();
            stream.flush().await.unwrap();
        });

        let tokens = [TEST_TOKEN_1];
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event((tracing_events(), client_subscriber))?
            .with_random(Random::with_seed(456))?
            .with_dc(MockDcEndpoint::new(&tokens))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr)
                .with_server_name("localhost")
                .with_deduplicate(true);
            let mut conn = client.connect(connect.clone()).await.unwrap();
            let id1 = conn.id();
            confirm_conn_works(&mut conn).await;

            // "Open" a second connection -- this should not actually open a new connection.
            let mut conn2 = client.connect(connect.clone()).await.unwrap();
            assert_eq!(conn2.id(), id1);
            confirm_conn_works(&mut conn2).await;

            // New address means new connection. (FIXME: Also test SNI differences, need to figure
            // out certificates for that...)
            let mut conn3 = client
                .connect(
                    Connect::new(addr2)
                        .with_server_name("localhost")
                        .with_deduplicate(true),
                )
                .await
                .unwrap();
            assert_ne!(conn3.id(), conn.id());
            confirm_conn_works(&mut conn3).await;

            drop(conn);
            drop(conn2);

            // "Open" a connection -- since the conn/conn2 handles have dropped, this should be a
            // new connection.
            let mut conn4 = client.connect(connect.clone()).await.unwrap();
            assert_ne!(conn4.id(), id1);
            confirm_conn_works(&mut conn4).await;
        });

        Ok(addr)
    })
    .unwrap();

    let server_started_count = server_events.lock().unwrap().len();
    let client_started_count = client_events.lock().unwrap().len();

    assert_eq!(server_started_count, 3);
    assert_eq!(client_started_count, 3);
}

#[track_caller]
fn confirm_conn_works(
    conn: &mut crate::connection::Connection,
) -> impl std::future::Future<Output = ()> + '_ {
    let caller = std::panic::Location::caller();
    async move {
        let mut stream = conn
            .accept_bidirectional_stream()
            .await
            .unwrap()
            .unwrap_or_else(|| panic!("from {caller:?}"));

        let mut recv_len = 0;
        while let Some(chunk) = stream.receive().await.unwrap() {
            recv_len += chunk.len();
        }
        assert_eq!(LEN, recv_len);
    }
}

#[derive(Clone)]
struct Toggle(Arc<Mutex<Outcome>>);

impl Toggle {
    fn new(outcome: Outcome) -> Self {
        Self(Arc::new(Mutex::new(outcome)))
    }

    fn set(&self, outcome: Outcome) {
        *self.0.lock().unwrap() = outcome;
    }
}

impl crate::provider::endpoint_limits::Limiter for Toggle {
    fn on_connection_attempt(
        &mut self,
        _info: &crate::provider::endpoint_limits::ConnectionAttempt<'_>,
    ) -> Outcome {
        self.0.lock().unwrap().clone()
    }
}

#[test]
fn deduplicate_non_terminal() {
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));

    let server_subscriber = recorder::ConnectionStarted::new();
    let server_events = server_subscriber.events();
    let client_subscriber = recorder::ConnectionStarted::new();
    let client_events = client_subscriber.events();
    test(model, |handle| {
        let toggle = Toggle::new(Outcome::drop());
        let tokens = [TEST_TOKEN_1];
        let mut server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(), server_subscriber.clone()))?
            .with_random(Random::with_seed(456))?
            .with_dc(MockDcEndpoint::new(&tokens))?
            .with_endpoint_limits(toggle.clone())?
            .start()?;

        let addr = server.local_addr()?;
        spawn(async move {
            let mut conn = server.accept().await.unwrap();
            for _ in 0..2 {
                let mut stream = conn.open_bidirectional_stream().await.unwrap();
                stream.send(vec![42; LEN].into()).await.unwrap();
                stream.flush().await.unwrap();
            }
            let mut conn = server.accept().await.unwrap();
            let mut stream = conn.open_bidirectional_stream().await.unwrap();
            stream.send(vec![42; LEN].into()).await.unwrap();
            stream.flush().await.unwrap();
        });

        let tokens = [TEST_TOKEN_1];
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event((tracing_events(), client_subscriber))?
            .with_random(Random::with_seed(456))?
            .with_dc(MockDcEndpoint::new(&tokens))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr)
                .with_server_name("localhost")
                .with_deduplicate(true);
            client.connect(connect.clone()).await.unwrap_err();

            // now allow connections
            toggle.set(Outcome::allow());

            let mut conn = client.connect(connect.clone()).await.unwrap();
            confirm_conn_works(&mut conn).await;
        });

        Ok(addr)
    })
    .unwrap();

    let server_started_count = server_events.lock().unwrap().len();
    let client_started_count = client_events.lock().unwrap().len();

    assert_eq!(server_started_count, 1);
    assert_eq!(client_started_count, 2);
}
