// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{
    connection::Error,
    provider::tls::s2n_tls::{self as tls, security},
};
use s2n_quic_core::{crypto::tls::Error as TlsError, transport};

// It helps to expand the Client Hello size to excced 64 KB, by filling
// the alpn extension in Client Hello with 65310 bytes.
static FAKE_PROTOCOL_COUNT: u16 = 4665;
// Maximum handshake message size is 64KB in S2N-TLS.
static MAXIMUM_HANDSHAKE_MESSAGE_SIZE: usize = 65536;

//= https://www.rfc-editor.org/rfc/rfc9000#section-4
//# To avoid excessive buffering at multiple layers, QUIC implementations
//# SHOULD provide an interface for the cryptographic protocol
//# implementation to communicate its buffering limits.
/// This test shows that the default TLS provider already provides
/// limits for buffering. The server will drop a giant Client Hello.
#[test]
fn buffer_limit_s2n_tls_test() {
    let model = Model::default();
    let policy = &security::Policy::from_version("default_tls13").unwrap();

    let connection_closed_subscriber = recorder::ConnectionClosed::new();
    let connection_closed_event = connection_closed_subscriber.events();
    let client_hello_subscriber = recorder::TlsClientHello::new();
    let client_hello_event = client_hello_subscriber.events();

    test(model, |handle| {
        let server = tls::Server::from_loader({
            let mut builder = tls::config::Config::builder();
            builder
                .enable_quic()?
                .set_application_protocol_preference(["h3"])?
                .set_security_policy(policy)?
                .load_pem(
                    certificates::CERT_PEM.as_bytes(),
                    certificates::KEY_PEM.as_bytes(),
                )?;

            builder.build()?
        });

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(server)?
            .with_event((
                tracing_events(),
                (connection_closed_subscriber, client_hello_subscriber),
            ))?
            .with_random(Random::with_seed(456))?
            .start()?;

        // Fill application_layer_protocol_negotiation extension in ClientHello.
        let mut application_protocols: Vec<String> = Vec::new();
        application_protocols.push("h3".to_string());
        for _ in 0..FAKE_PROTOCOL_COUNT {
            application_protocols.push("fake-protocol".to_string());
        }

        let client = tls::Client::from_loader({
            let mut builder = tls::config::Config::builder();
            builder
                .enable_quic()?
                .set_application_protocol_preference(application_protocols)?
                .set_security_policy(policy)?
                .trust_pem(certificates::CERT_PEM.as_bytes())?;

            builder.build()?
        });

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client)?
            .with_event(tracing_events())?
            .with_random(Random::with_seed(456))?
            .start()?;

        let addr = start_server(server)?;
        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            client.connect(connect).await.unwrap_err();
        });
        Ok(())
    })
    .unwrap();

    // The TlsClientHello payload should be more than the maximum handshake message size.
    let client_hello_handle = client_hello_event.lock().unwrap();
    assert!(client_hello_handle[0] > MAXIMUM_HANDSHAKE_MESSAGE_SIZE);

    let connection_closed_handle = connection_closed_event.lock().unwrap();

    // Expect exactly one connection closed error because the server
    // terminates the connection after receiving a Client Hello message
    // that exceeds the maximum allowed handshake size.
    assert_eq!(connection_closed_handle.len(), 1);

    // The error message for connection closed error should be UNEXPECTED_MESSAGE.
    let Error::Transport { code, .. } = connection_closed_handle[0] else {
        panic!("Unexpected error type")
    };
    assert_eq!(
        code,
        transport::Error::from(TlsError::UNEXPECTED_MESSAGE).code
    );
}
