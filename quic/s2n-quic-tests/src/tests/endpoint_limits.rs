// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic::provider::endpoint_limits::{ConnectionAttempt, Limiter, Outcome};
use s2n_quic_core::{connection::error::Error, endpoint};

/// A custom limiter that allows the first connection but closes subsequent ones
#[derive(Default)]
struct AllowFirstThenCloseLimiter {
    connection_count: usize,
}

impl Limiter for AllowFirstThenCloseLimiter {
    fn on_connection_attempt(&mut self, _info: &ConnectionAttempt) -> Outcome {
        if self.connection_count == 0 {
            self.connection_count += 1;
            // Allow the first connection
            Outcome::allow()
        } else {
            // Close subsequent connections
            Outcome::throttle()
        }
    }
}

// This test verifies that when the server would send a CONNECTION_CLOSE frame with
// error code CONNECTION_REFUSED when the server's limiter returns Outcome::throttle().
#[test]
fn endpoint_limits_close_test() {
    let model = Model::default();

    // ConnectionClose recorder can't track the connection close reason.
    // Hence, we need another subscriber to track the reaason when the CONNECTION_CLOSE frame is received.
    let connection_close_subscriber = recorder::ConnectionClosed::new();
    let connection_close_event = connection_close_subscriber.events();

    let connection_close_reason_subscriber = recorder::ConnectionCloseReason::new();
    let connection_close_reason_event = connection_close_reason_subscriber.events();

    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .with_endpoint_limits(AllowFirstThenCloseLimiter::default())?
            .start()?;

        let server_addr = start_server(server)?;

        let client1 = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;

        let client2 = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event((
                tracing_events(),
                (
                    connection_close_reason_subscriber,
                    connection_close_subscriber,
                ),
            ))?
            .with_random(Random::with_seed(789))?
            .start()?;

        primary::spawn(async move {
            // First client should connect successfully
            let connect1 = Connect::new(server_addr).with_server_name("localhost");
            client1.connect(connect1).await.unwrap();

            // Second client should fail to connect, since the server's endpoint limiter
            // will refuse all connections after the first one.
            let connect2 = Connect::new(server_addr).with_server_name("localhost");
            let result = client2.connect(connect2).await;
            assert!(matches!(result.unwrap_err(), Error::Transport { .. }));
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

    // Verify that the reason in the CONNECTION_CLOSE frame is THROTTLE_REASON
    let connection_close_reason_status = connection_close_reason_event.lock().unwrap();
    assert_eq!(connection_close_reason_status.len(), 1);
    assert_eq!(
        connection_close_reason_status[0],
        endpoint::limits::THROTTLE_REASON.as_bytes()
    );
}
