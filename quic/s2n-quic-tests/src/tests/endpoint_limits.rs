// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use ::rand::Rng;
use s2n_quic::provider::{
    connection_id,
    endpoint_limits::{ConnectionAttempt, Limiter, Outcome},
};
use s2n_quic_core::{
    connection::{error::Error, id},
    endpoint,
};

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
            Outcome::close()
        }
    }
}

// We've allocated 150 bytes for the connection close packet.
// Testing with the maximum length of a connection ID ensures that we've allocated enough to store packet.
const MAX_CID_LEN: usize = 20;
struct MaxSizeIdFormat;

impl connection_id::Generator for MaxSizeIdFormat {
    fn generate(
        &mut self,
        _connection_info: &id::ConnectionInfo,
    ) -> s2n_quic_core::connection::LocalId {
        let mut id = [0u8; MAX_CID_LEN];
        ::rand::rng().fill_bytes(&mut id);
        connection_id::LocalId::try_from_bytes(&id[..]).unwrap()
    }
}

impl connection_id::Validator for MaxSizeIdFormat {
    fn validate(&self, _connection_info: &id::ConnectionInfo, _buffer: &[u8]) -> Option<usize> {
        Some(MAX_CID_LEN)
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.2.2
//= type=test
//# If a server refuses to accept a new connection, it SHOULD send an
//# Initial packet containing a CONNECTION_CLOSE frame with error code
//# CONNECTION_REFUSED.
// This test verifies that the server sends a CONNECTION_CLOSE frame with
// error code CONNECTION_REFUSED when the server's limiter returns Outcome::close().
#[test]
fn endpoint_limits_close_test() {
    let model = Model::default();

    let connection_close_subscriber = recorder::ConnectionClosed::new();
    let connection_close_event = connection_close_subscriber.events();

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(tracing_events(false, model.clone()))?
            .with_connection_id(MaxSizeIdFormat)?
            .with_random(Random::with_seed(456))?
            .with_endpoint_limits(AllowFirstThenCloseLimiter::default())?
            .start()?;

        let server_addr = start_server(server)?;

        let client1 = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event(tracing_events(true, model.clone()))?
            .with_connection_id(MaxSizeIdFormat)?
            .with_random(Random::with_seed(456))?
            .start()?;

        let client2 = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(certificates::CERT_PEM)?
            .with_event((tracing_events(true, model.clone()), connection_close_subscriber))?
            .with_connection_id(MaxSizeIdFormat)?
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
            assert!(matches!(result.unwrap_err(), Error::Transport { code, .. } if code == s2n_quic_core::transport::Error::CONNECTION_REFUSED.code));
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
