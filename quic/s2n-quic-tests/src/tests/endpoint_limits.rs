// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic::provider::endpoint_limits::{ConnectionAttempt, Limiter, Outcome};
use s2n_quic_core::{connection::error::Error, endpoint};

/// A custom limiter that always returns Outcome::close()
struct AlwaysCloseLimiter;

impl Limiter for AlwaysCloseLimiter {
    fn on_connection_attempt(&mut self, _info: &ConnectionAttempt) -> Outcome {
        Outcome::close()
    }
}

// This test verifies that when the server would send a CONNECTION_CLOSE frame with
// error code CONNECTION_REFUSED when the server's limiter returns Outcome::close().
#[test]
fn endpoint_limits_test() {
    let model = Model::default();

    let connection_close_subscriber = recorder::ConnectionClosed::new();
    let connection_close_event = connection_close_subscriber.events();

    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .with_endpoint_limits(AlwaysCloseLimiter)?
            .start()?;

        let server_addr = start_server(server)?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event((tracing_events(), connection_close_subscriber))?
            .with_random(Random::with_seed(456))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(server_addr).with_server_name("localhost");

            // The server should immediately close the connection, so that the handshake will fail
            matches!(
                client.connect(connect).await.unwrap_err(),
                Error::Transport { .. }
            );
        });

        Ok(())
    })
    .unwrap();

    // Verify that the client received a CONNECTION_CLOSE frame with error code CONNECTION_REFUSED,
    // and the CONNECTION_CLOSE frame is sent by the server (remote from client's perspectives).
    let connection_close_status = connection_close_event.lock().unwrap();
    assert_eq!(connection_close_status.len(), 1);
    assert!(matches!(
        connection_close_status[0],
        Error::Transport {
            code,
            initiator,
            ..
        } if (code == s2n_quic_core::transport::Error::CONNECTION_REFUSED.code && initiator == endpoint::Location::Remote)
    ));
}
