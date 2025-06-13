// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[test]
fn slow_tls() {
    use super::*;
    use crate::provider::tls::default;
    use s2n_quic_core::crypto::tls::testing::certificates::{CERT_PEM, KEY_PEM};

    let model = Model::default();

    let server_endpoint = default::Server::builder()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .build()
        .unwrap();
    let slow_server = SlowTlsProvider {
        endpoint: server_endpoint,
    };

    let client_endpoint = default::Client::builder()
        .with_certificate(CERT_PEM)
        .unwrap()
        .build()
        .unwrap();
    let slow_client = SlowTlsProvider {
        endpoint: client_endpoint,
    };

    test(model, |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(slow_server)?
            .start()?;

        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(slow_client)?
            .with_event((tracing_events(), MyEvents))?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();

    struct MyEvents;
    struct MyContext;
    impl events::Subscriber for MyEvents {
        type ConnectionContext = MyContext;

        fn create_connection_context(
            &mut self,
            _meta: &events::ConnectionMeta,
            _info: &events::ConnectionInfo,
        ) -> Self::ConnectionContext {
            Self::ConnectionContext {}
        }
        fn on_transport_parameters_received(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &s2n_quic_core::event::api::ConnectionMeta,
            _event: &s2n_quic_core::event::api::TransportParametersReceived,
        ) {
            // Slow TLS implementation has no affect on when transport parameters are received
            assert_eq!(meta.timestamp.to_string(), "0:00:00.100000");
        }

        fn on_connection_closed(
            &mut self,
            _context: &mut Self::ConnectionContext,
            meta: &s2n_quic_core::event::api::ConnectionMeta,
            _event: &s2n_quic_core::event::api::ConnectionClosed,
        ) {
            // Slow TLS implementation has no affect on when the connection is shut down
            assert_eq!(meta.timestamp.to_string(), "0:00:00.200000");
        }
    }
}
