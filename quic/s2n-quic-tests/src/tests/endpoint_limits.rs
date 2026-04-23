// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use ::rand::Rng;

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.2.2
//= type=test
//# If a server refuses to accept a new connection, it SHOULD send an
//# Initial packet containing a CONNECTION_CLOSE frame with error code
//# CONNECTION_REFUSED.
compat_test!(endpoint_limits_close_test {
    const MAX_CID_LEN: usize = 20;

    // Define server-side types using server_provider/server_core
    struct ServerLimiter {
        connection_count: usize,
    }
    impl Default for ServerLimiter {
        fn default() -> Self { Self { connection_count: 0 } }
    }
    impl server_provider::endpoint_limits::Limiter for ServerLimiter {
        fn on_connection_attempt(
            &mut self,
            _info: &server_provider::endpoint_limits::ConnectionAttempt,
        ) -> server_provider::endpoint_limits::Outcome {
            if self.connection_count == 0 {
                self.connection_count += 1;
                server_provider::endpoint_limits::Outcome::allow()
            } else {
                server_provider::endpoint_limits::Outcome::close()
            }
        }
    }

    struct ServerCidFormat;
    impl server_provider::connection_id::Generator for ServerCidFormat {
        fn generate(
            &mut self,
            _info: &server_core::connection::id::ConnectionInfo,
        ) -> server_core::connection::LocalId {
            let mut id = [0u8; MAX_CID_LEN];
            ::rand::rng().fill_bytes(&mut id);
            server_provider::connection_id::LocalId::try_from_bytes(&id[..]).unwrap()
        }
    }
    impl server_provider::connection_id::Validator for ServerCidFormat {
        fn validate(&self, _info: &server_core::connection::id::ConnectionInfo, _buffer: &[u8]) -> Option<usize> {
            Some(MAX_CID_LEN)
        }
    }

    struct ClientCidFormat;
    impl client_provider::connection_id::Generator for ClientCidFormat {
        fn generate(
            &mut self,
            _info: &client_core::connection::id::ConnectionInfo,
        ) -> client_core::connection::LocalId {
            let mut id = [0u8; MAX_CID_LEN];
            ::rand::rng().fill_bytes(&mut id);
            client_provider::connection_id::LocalId::try_from_bytes(&id[..]).unwrap()
        }
    }
    impl client_provider::connection_id::Validator for ClientCidFormat {
        fn validate(&self, _info: &client_core::connection::id::ConnectionInfo, _buffer: &[u8]) -> Option<usize> {
            Some(MAX_CID_LEN)
        }
    }

    let model = Model::default();

    let connection_close_subscriber = client_recorder::ConnectionClosed::new();
    let connection_close_event = connection_close_subscriber.events();

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(server_handle(handle).builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(server_tracing_events(false, model.clone()))?
            .with_connection_id(ServerCidFormat)?
            .with_random(ServerRandom::with_seed(456))?
            .with_endpoint_limits(ServerLimiter::default())?
            .start()?;

        let server_addr = start_server(server)?;

        let client1 = Client::builder()
            .with_io(client_handle(handle).builder().build()?)?
            .with_tls(client_certificates::CERT_PEM)?
            .with_event(client_tracing_events(true, model.clone()))?
            .with_connection_id(ClientCidFormat)?
            .with_random(ClientRandom::with_seed(456))?
            .start()?;

        let client2 = Client::builder()
            .with_io(client_handle(handle).builder().build()?)?
            .with_tls(client_certificates::CERT_PEM)?
            .with_event((client_tracing_events(true, model.clone()), connection_close_subscriber))?
            .with_connection_id(ClientCidFormat)?
            .with_random(ClientRandom::with_seed(789))?
            .start()?;

        primary::spawn(async move {
            let connect1 = Connect::new(server_addr).with_server_name("localhost");
            client1.connect(connect1).await.unwrap();

            let connect2 = Connect::new(server_addr).with_server_name("localhost");
            let result = client2.connect(connect2).await;
            assert!(matches!(
                result.unwrap_err(),
                client_core::connection::error::Error::Transport { code, .. }
                    if code == client_core::transport::Error::CONNECTION_REFUSED.code
            ));
        });

        Ok(())
    })
    .unwrap();

    let connection_close_status = connection_close_event.lock().unwrap();
    assert_eq!(connection_close_status.len(), 1);
    assert!(matches!(
        connection_close_status[0],
        client_core::connection::error::Error::Transport {
            code,
            initiator,
            ..
        } if (code == client_core::transport::Error::CONNECTION_REFUSED.code
              && initiator == client_core::endpoint::Location::Remote)
    ));
});
