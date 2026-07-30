// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic::{
    connection::Error,
    provider::tls::default::{self as tls},
};
use s2n_quic_core::{crypto::tls::Error as TlsError, transport};

// It helps to expand the Client Hello size to excced 64 KB, by filling
// the alpn extension in Client Hello with 65310 bytes.
static FAKE_PROTOCOL_COUNT: u16 = 4665;
// Maximum handshake message size is 64KB in S2N-TLS and Rustls.
static MAXIMUM_HANDSHAKE_MESSAGE_SIZE: usize = 65536;

//= https://www.rfc-editor.org/rfc/rfc9000#section-4
//= type=implication
//# To avoid excessive buffering at multiple layers, QUIC implementations
//# SHOULD provide an interface for the cryptographic protocol
//# implementation to communicate its buffering limits.
/// This test shows that the default TLS provider already provides
/// limits for buffering. The server will drop a giant Client Hello.
#[test]
fn buffer_limit_test() {
    let model = Model::default();

    let connection_closed_subscriber = recorder::ConnectionClosed::new();
    let connection_closed_event = connection_closed_subscriber.events();
    let client_hello_subscriber = recorder::TlsClientHello::new();
    let client_hello_event = client_hello_subscriber.events();

    test(model.clone(), |handle| {
        let server = tls::Server::builder()
            .with_application_protocols(["h3"].iter())
            .unwrap()
            .with_certificate(certificates::CERT_PEM, certificates::KEY_PEM)
            .unwrap()
            .build()
            .unwrap();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(server)?
            .with_event((
                tracing_events(true, model.clone()),
                (client_hello_subscriber, connection_closed_subscriber),
            ))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let mut application_protocols: Vec<String> = Vec::new();
        application_protocols.push("h3".to_string());
        for _ in 0..FAKE_PROTOCOL_COUNT {
            application_protocols.push("fake-protocol".to_string());
        }

        let client = tls::Client::builder()
            .with_application_protocols(application_protocols.iter())
            .unwrap()
            .with_certificate(certificates::CERT_PEM)
            .unwrap()
            .build()
            .unwrap();

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client)?
            .with_event(tracing_events(true, model.clone()))?
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

    // The error message for connection closed error should be INTERNAL_ERROR.
    let Error::Transport { code, .. } = connection_closed_handle[0] else {
        panic!("Unexpected error type")
    };

    // Rustls emits INTERNAL_ERROR and S2N-TLS emits UNEXPECTED_MESSAGE error
    // when the server close the connection due to large Client Hello.
    let expected_error = if cfg!(target_os = "windows") {
        TlsError::INTERNAL_ERROR
    } else {
        TlsError::UNEXPECTED_MESSAGE
    };
    assert_eq!(code, transport::Error::from(expected_error).code);
}
