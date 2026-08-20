// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::recorder;

/// The signature scheme negotiated with the default test certificates (ECDSA P-256).
const EXPECTED_SCHEME: &str = "ecdsa_secp256r1_sha256";

/// Pins the signature scheme a connection is expected to negotiate.
struct Case {
    /// Used in assertion messages to identify which test case failed.
    name: &'static str,
    server_cert: &'static str,
    server_key: &'static str,
    client_trust: &'static str,
    expected: &'static str,
}

/// Self-signed ECDSA P-256, the certificate most of this test suite uses.
const ECDSA_P256: Case = Case {
    name: "ECDSA P-256",
    server_cert: certificates::CERT_PEM,
    server_key: certificates::KEY_PEM,
    client_trust: certificates::CERT_PEM,
    expected: "ecdsa_secp256r1_sha256",
};

/// Self-signed RSA 2048. TLS 1.3 forbids PKCS#1 v1.5 signatures, so an `rsaEncryption`
/// key must sign with RSA-PSS using an RSAE-encoded key.
const RSA_2048: Case = Case {
    name: "RSA 2048",
    server_cert: certificates::CERT_PKCS1_PEM,
    server_key: certificates::KEY_PKCS1_PEM,
    client_trust: certificates::CERT_PKCS1_PEM,
    expected: "rsa_pss_rsae_sha256",
};

/// ECDSA P-384, issued by the mTLS CA. Client authentication is intentionally not
/// enabled; only the server's key type is being varied here.
const ECDSA_P384: Case = Case {
    name: "ECDSA P-384",
    server_cert: certificates::MTLS_SERVER_CERT,
    server_key: certificates::MTLS_SERVER_KEY,
    client_trust: certificates::MTLS_CA_CERT,
    expected: "ecdsa_secp384r1_sha384",
};

/// The event recorders attached to a single endpoint.
#[derive(Clone, Default)]
struct Recorders {
    event: recorder::SignatureScheme,
    exporter: recorder::TlsExporterSignatureScheme,
}

impl Recorders {
    fn subscriber(&self, model: Model) -> impl event::Subscriber {
        (
            (self.event.clone(), self.exporter.clone()),
            tracing_events(true, model),
        )
    }

    /// Asserts that both recorders saw exactly one handshake reporting `scheme`.
    fn assert_observed(&self, scheme: Option<&str>, endpoint: &str) {
        assert_eq!(
            *self.event.events().lock().unwrap(),
            scheme.map(str::to_owned).into_iter().collect::<Vec<_>>(),
            "unexpected SignatureScheme events on the {endpoint}"
        );
        assert_eq!(
            *self.exporter.events().lock().unwrap(),
            vec![scheme.map(str::to_owned)],
            "unexpected TlsExporterReady signature scheme on the {endpoint}"
        );
    }
}

/// Runs an h3 handshake with the case's server key type and asserts that every surface
/// reports the signature scheme the case expects.
fn signature_scheme_test(case: &'static Case) {
    let model = Model::default();
    model.set_delay(Duration::from_millis(50));

    let server_recorders = Recorders::default();
    let client_recorders = Recorders::default();

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls((case.server_cert, case.server_key))?
            .with_event(server_recorders.subscriber(model.clone()))?
            .start()?;
        let addr = start_server(server)?;

        let client = Client::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(case.client_trust)?
            .with_event(client_recorders.subscriber(model.clone()))?
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

    // The scheme is reported through events only, so assert both event surfaces on both
    // endpoints.
    client_recorders.assert_observed(Some(case.expected), &format!("{} client", case.name));
    server_recorders.assert_observed(Some(case.expected), &format!("{} server", case.name));
}

#[test]
fn signature_scheme_is_available_on_h3_connections() {
    signature_scheme_test(&ECDSA_P256);
}

/// The event must report a different scheme when the server signs with a different key
/// type, which is what proves it reflects the negotiated handshake.
#[test]
fn signature_scheme_reports_ecdsa_p384() {
    signature_scheme_test(&ECDSA_P384);
}

#[test]
fn signature_scheme_reports_rsa_pss() {
    signature_scheme_test(&RSA_2048);
}

/// A resumed handshake produces no server signature, so no signature scheme is reported and no event is emitted.
#[test]
fn signature_scheme_is_absent_on_resumed_h3_connections() {
    use crate::resumption::*;

    // Resumption set up similar to the one in resumption.rs
    let handler = SessionTicketHandler::default();
    let full = Recorders::default();
    let resumed = Recorders::default();

    // First connection: an ordinary full handshake. Its purpose is to obtain the ticket,
    // and it doubles as the control case showing a scheme *is* reported here.
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
            .with_event(full.subscriber(model.clone()))?
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
    // signed or presented a verifiable certificate -- which is exactly why there is no
    // signature scheme to report.
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
            .with_event(resumed.subscriber(model.clone()))?
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

    full.assert_observed(Some(EXPECTED_SCHEME), "client after a full handshake");
    resumed.assert_observed(None, "client after a resumed handshake");
}
