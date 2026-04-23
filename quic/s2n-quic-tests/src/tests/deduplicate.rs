// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

const LEN: usize = 1_000_000;

compat_test!(deduplicate_successfully {
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));

    let server_subscriber = server_recorder::ConnectionStarted::new();
    let server_events = server_subscriber.events();
    let client_subscriber = client_recorder::ConnectionStarted::new();
    let client_events = client_subscriber.events();
    test(model.clone(), |handle| {
        let mut server = Server::builder()
            .with_io(server_handle(handle).builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((
                server_tracing_events(true, model.clone()),
                server_subscriber.clone(),
            ))?
            .with_random(ServerRandom::with_seed(456))?
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
            .with_io(server_handle(handle).builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((server_tracing_events(true, model.clone()), server_subscriber))?
            .with_random(ServerRandom::with_seed(456))?
            .start()?;

        let addr2 = server2.local_addr()?;
        spawn(async move {
            let mut conn = server2.accept().await.unwrap();
            let mut stream = conn.open_bidirectional_stream().await.unwrap();
            stream.send(vec![42; LEN].into()).await.unwrap();
            stream.flush().await.unwrap();
        });

        let tokens = [client_tokens::TEST_TOKEN_1];
        let client = Client::builder()
            .with_io(client_handle(handle).builder().build().unwrap())?
            .with_tls(client_certificates::CERT_PEM)?
            .with_event((client_tracing_events(true, model.clone()), client_subscriber))?
            .with_random(ClientRandom::with_seed(456))?
            .with_dc(client_dc::MockDcEndpoint::new(&tokens))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr)
                .with_server_name("localhost")
                .with_deduplicate(true);
            let mut conn = client.connect(connect.clone()).await.unwrap();
            let id1 = conn.id();

            {
                let mut stream = conn.accept_bidirectional_stream().await.unwrap().unwrap();
                let mut recv_len = 0;
                while let Some(chunk) = stream.receive().await.unwrap() {
                    recv_len += chunk.len();
                }
                assert_eq!(LEN, recv_len);
            }

            let mut conn2 = client.connect(connect.clone()).await.unwrap();
            assert_eq!(conn2.id(), id1);
            {
                let mut stream = conn2.accept_bidirectional_stream().await.unwrap().unwrap();
                let mut recv_len = 0;
                while let Some(chunk) = stream.receive().await.unwrap() {
                    recv_len += chunk.len();
                }
                assert_eq!(LEN, recv_len);
            }

            let mut conn3 = client
                .connect(
                    Connect::new(addr2)
                        .with_server_name("localhost")
                        .with_deduplicate(true),
                )
                .await
                .unwrap();
            assert_ne!(conn3.id(), conn.id());
            {
                let mut stream = conn3.accept_bidirectional_stream().await.unwrap().unwrap();
                let mut recv_len = 0;
                while let Some(chunk) = stream.receive().await.unwrap() {
                    recv_len += chunk.len();
                }
                assert_eq!(LEN, recv_len);
            }

            drop(conn);
            drop(conn2);

            let mut conn4 = client.connect(connect.clone()).await.unwrap();
            assert_ne!(conn4.id(), id1);
            {
                let mut stream = conn4.accept_bidirectional_stream().await.unwrap().unwrap();
                let mut recv_len = 0;
                while let Some(chunk) = stream.receive().await.unwrap() {
                    recv_len += chunk.len();
                }
                assert_eq!(LEN, recv_len);
            }
        });

        Ok(addr)
    })
    .unwrap();

    let server_started_count = server_events.lock().unwrap().len();
    let client_started_count = client_events.lock().unwrap().len();

    assert_eq!(server_started_count, 3);
    assert_eq!(client_started_count, 3);
});

compat_test!(deduplicate_non_terminal {
    use server_provider::endpoint_limits::Outcome;

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

    impl server_provider::endpoint_limits::Limiter for Toggle {
        fn on_connection_attempt(
            &mut self,
            _info: &server_provider::endpoint_limits::ConnectionAttempt<'_>,
        ) -> Outcome {
            self.0.lock().unwrap().clone()
        }
    }

    let model = Model::default();
    model.set_delay(Duration::from_millis(50));

    let server_subscriber = server_recorder::ConnectionStarted::new();
    let server_events = server_subscriber.events();
    let client_subscriber = client_recorder::ConnectionStarted::new();
    let client_events = client_subscriber.events();
    test(model.clone(), |handle| {
        let toggle = Toggle::new(Outcome::drop());
        let tokens = [server_tokens::TEST_TOKEN_1];
        let mut server = Server::builder()
            .with_io(server_handle(handle).builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((
                server_tracing_events(false, model.clone()),
                server_subscriber.clone(),
            ))?
            .with_random(ServerRandom::with_seed(456))?
            .with_dc(server_dc::MockDcEndpoint::new(&tokens))?
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

        let tokens = [client_tokens::TEST_TOKEN_1];
        let client = Client::builder()
            .with_io(client_handle(handle).builder().build().unwrap())?
            .with_tls(client_certificates::CERT_PEM)?
            .with_event((client_tracing_events(true, model.clone()), client_subscriber))?
            .with_random(ClientRandom::with_seed(456))?
            .with_dc(client_dc::MockDcEndpoint::new(&tokens))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr)
                .with_server_name("localhost")
                .with_deduplicate(true);
            client.connect(connect.clone()).await.unwrap_err();

            // now allow connections
            toggle.set(Outcome::allow());

            let mut conn = client.connect(connect.clone()).await.unwrap();
            {
                let mut stream = conn.accept_bidirectional_stream().await.unwrap().unwrap();
                let mut recv_len = 0;
                while let Some(chunk) = stream.receive().await.unwrap() {
                    recv_len += chunk.len();
                }
                assert_eq!(LEN, recv_len);
            }
        });

        Ok(addr)
    })
    .unwrap();

    let server_started_count = server_events.lock().unwrap().len();
    let client_started_count = client_events.lock().unwrap().len();

    assert_eq!(server_started_count, 1);
    assert_eq!(client_started_count, 2);
});
