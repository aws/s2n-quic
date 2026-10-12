// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{recorder, recorder::PublicKeyTypes};
use s2n_quic::provider::tls;

/// The public key type of the default test certificates (ECDSA P-256).
const EXPECTED_KEY_TYPE: &str = "ecdsa_secp256r1";

struct Case {
    /// Used in assertion messages to identify which test case failed.
    name: &'static str,
    server_cert: &'static str,
    server_key: &'static str,
    client_trust: &'static str,
    /// The certificate the client authenticates with, if client authentication is
    /// enabled for this case.
    client_identity: Option<(&'static str, &'static str)>,
    /// The expected server and client public key types, in that order.
    expected: (Option<&'static str>, Option<&'static str>),
}

const ECDSA_P256: Case = Case {
    name: "ECDSA P-256",
    server_cert: certificates::CERT_PEM,
    server_key: certificates::KEY_PEM,
    client_trust: certificates::CERT_PEM,
    client_identity: None,
    expected: (Some("ecdsa_secp256r1"), None),
};

const RSA_2048: Case = Case {
    name: "RSA 2048",
    server_cert: certificates::CERT_PKCS1_PEM,
    server_key: certificates::KEY_PKCS1_PEM,
    client_trust: certificates::CERT_PKCS1_PEM,
    client_identity: None,
    expected: (Some("rsa2048"), None),
};

const ECDSA_P384: Case = Case {
    name: "ECDSA P-384",
    server_cert: certificates::MTLS_SERVER_CERT,
    server_key: certificates::MTLS_SERVER_KEY,
    client_trust: certificates::MTLS_CA_CERT,
    client_identity: None,
    expected: (Some("ecdsa_secp384r1"), None),
};

const MTLS: Case = Case {
    name: "mTLS",
    server_cert: certificates::MTLS_SERVER_CERT,
    server_key: certificates::MTLS_SERVER_KEY,
    client_trust: certificates::MTLS_CA_CERT,
    client_identity: Some((
        certificates::MTLS_CLIENT_CERT,
        certificates::MTLS_CLIENT_KEY,
    )),
    expected: (Some("ecdsa_secp384r1"), Some("ecdsa_secp384r1")),
};

/// Builds the TLS providers described by `case`.
fn providers(case: &Case) -> Result<(tls::default::Server, tls::default::Client)> {
    let mut server =
        tls::default::Server::builder().with_certificate(case.server_cert, case.server_key)?;
    let mut client = tls::default::Client::builder().with_certificate(case.client_trust)?;

    if let Some((cert, key)) = case.client_identity {
        server = server
            .with_client_authentication()?
            .with_trusted_certificate(case.client_trust)?;
        client = client.with_client_identity(cert, key)?;
    }

    Ok((server.build()?, client.build()?))
}

/// Asserts that `recorder` saw exactly one handshake reporting `expected`.
///
/// The event is only emitted when at least one key type is available, so an `expected`
/// of `(None, None)` asserts that no event was emitted at all.
fn assert_observed(
    recorder: &recorder::SignaturePublicKeyType,
    expected: (Option<&str>, Option<&str>),
    endpoint: &str,
) {
    let expected: PublicKeyTypes = (expected.0.map(str::to_owned), expected.1.map(str::to_owned));

    let expected_events = if expected == (None, None) {
        vec![]
    } else {
        vec![expected]
    };

    assert_eq!(
        *recorder.events().lock().unwrap(),
        expected_events,
        "unexpected SignaturePublicKeyType events on the {endpoint}"
    );
}

fn subscriber(recorder: &recorder::SignaturePublicKeyType, model: Model) -> impl event::Subscriber {
    (recorder.clone(), tracing_events(true, model))
}

/// Runs a handshake with the case's certificates and asserts that both endpoints
/// report the public key types the case expects.
fn public_key_type_test(case: &'static Case) {
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));

    let server_events = recorder::SignaturePublicKeyType::new();
    let client_events = recorder::SignaturePublicKeyType::new();

    test(model.clone(), |handle| {
        let (server_tls, client_tls) = providers(case)?;

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(server_tls)?
            .with_event(subscriber(&server_events, model.clone()))?
            .start()?;
        let addr = start_server(server)?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(client_tls)?
            .with_event(subscriber(&client_events, model.clone()))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let mut conn = client.connect(connect).await.unwrap();

            // confirm this really is an h3 connection
            assert_eq!(&conn.application_protocol().unwrap()[..], b"h3");

            // round trip through the echo server so the server side of the handshake
            // completes before the simulation ends
            let mut stream = conn.open_bidirectional_stream().await.unwrap();
            stream.send(Bytes::from_static(b"h3")).await.unwrap();
            stream.finish().unwrap();
            while stream.receive().await.unwrap().is_some() {}
        });

        Ok(addr)
    })
    .unwrap();

    // Both endpoints describe the same two certificates: one of them is their own and
    // the other is the peer's validated leaf.
    assert_observed(
        &client_events,
        case.expected,
        &format!("{} client", case.name),
    );
    assert_observed(
        &server_events,
        case.expected,
        &format!("{} server", case.name),
    );
}

#[test]
fn public_key_type_is_available_on_h3_connections() {
    public_key_type_test(&ECDSA_P256);
}

/// The event must report a different key type when the server presents a different
/// certificate, which is what proves it reflects the handshake.
#[test]
fn public_key_type_reports_ecdsa_p384() {
    public_key_type_test(&ECDSA_P384);
}

#[test]
fn public_key_type_reports_rsa_key_size() {
    public_key_type_test(&RSA_2048);
}

/// With client authentication both certificates are described.
#[test]
fn public_key_type_reports_client_certificate() {
    public_key_type_test(&MTLS);
}

/// A resumed handshake presents no certificate, so there is nothing to describe and no
/// event is emitted.
#[test]
fn public_key_type_is_absent_on_resumed_h3_connections() {
    use crate::resumption::*;

    // Resumption set up similar to the one in resumption.rs
    let handler = SessionTicketHandler::default();
    let full = recorder::SignaturePublicKeyType::new();
    let resumed = recorder::SignaturePublicKeyType::new();

    // First connection: an ordinary full handshake. Its purpose is to obtain the ticket,
    // and it doubles as the control case showing a key type *is* reported here.
    let model = Model::default();
    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(build_server_resumption_provider(
                certificates::CERT_PEM,
                certificates::KEY_PEM,
            )?)?
            .with_event(tracing_events(true, model.clone()))?
            .start()?;
        let addr = start_server(server)?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(build_client_resumption_provider(
                certificates::CERT_PEM,
                &handler,
            )?)?
            .with_event(subscriber(&full, model.clone()))?
            .start()?;

        // exchange data so the post-handshake session ticket reaches the client
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();

    // Second connection: the resumed handshake.
    //
    // This server intentionally presents a certificate the client does not trust. A full
    // handshake would therefore fail on certificate verification, so simply getting past
    // `connect()` is the proof that the handshake was resumed and that the server never
    // presented a verifiable certificate, which is exactly why there is no public key
    // type to report.
    let model = Model::default();
    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(build_server_resumption_provider(
                certificates::UNTRUSTED_CERT_PEM,
                certificates::UNTRUSTED_KEY_PEM,
            )?)?
            .with_event(tracing_events(true, model.clone()))?
            .start()?;
        let addr = start_server(server)?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(build_client_resumption_provider(
                certificates::CERT_PEM,
                &handler,
            )?)?
            .with_event(subscriber(&resumed, model.clone()))?
            .start()?;

        primary::spawn(async move {
            let connect = Connect::new(addr).with_server_name("localhost");
            let conn = client.connect(connect).await.expect(
                "the handshake should resume using the ticket from the first connection; \
                 a certificate error here means resumption did not happen",
            );

            assert_eq!(&conn.application_protocol().unwrap()[..], b"h3");
        });

        Ok(addr)
    })
    .unwrap();

    assert_observed(
        &full,
        (Some(EXPECTED_KEY_TYPE), None),
        "client after a full handshake",
    );
    assert_observed(&resumed, (None, None), "client after a resumed handshake");
}
