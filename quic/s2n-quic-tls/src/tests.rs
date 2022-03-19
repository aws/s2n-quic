// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{client, server, verify_host::VerifyHostContext};
use s2n_quic_core::{
    crypto::tls::{
        self,
        testing::certificates::{CERT_PEM, KEY_PEM, UNTRUSTED_CERT_PEM, UNTRUSTED_KEY_PEM},
        Endpoint,
    },
    transport,
};

pub struct VerifyHostNameClientCertVerifier {
    host_name: String,
}

impl VerifyHostNameClientCertVerifier {
    pub fn new(host_name: impl ToString) -> Box<VerifyHostNameClientCertVerifier> {
        Box::new(VerifyHostNameClientCertVerifier {
            host_name: host_name.to_string(),
        })
    }
}

impl VerifyHostContext for VerifyHostNameClientCertVerifier {
    fn verify_certificate_host_name(&self, host_name: &str) -> u8 {
        if self.host_name == host_name {
            1
        } else {
            0
        }
    }
}

pub struct RejectAllClientsCertVerifier {}

impl RejectAllClientsCertVerifier {
    fn new() -> Box<RejectAllClientsCertVerifier> {
        Box::new(RejectAllClientsCertVerifier {})
    }
}

impl VerifyHostContext for RejectAllClientsCertVerifier {
    fn verify_certificate_host_name(&self, _host_name: &str) -> u8 {
        0
    }
}

fn s2n_client() -> client::Client {
    client::Builder::default()
        .with_certificate(CERT_PEM)
        .unwrap()
        .build()
        .unwrap()
}

fn s2n_client_with_client_auth() -> client::Client {
    client::Builder::default()
        .with_clear_trust_store()
        .unwrap()
        .with_certificate(CERT_PEM)
        .unwrap()
        .with_client_identity(CERT_PEM, KEY_PEM)
        .unwrap()
        .build()
        .unwrap()
}

fn s2n_client_with_untrusted_client_auth() -> client::Client {
    client::Builder::default()
        .with_clear_trust_store()
        .unwrap()
        .with_certificate(CERT_PEM)
        .unwrap()
        .with_client_identity(UNTRUSTED_CERT_PEM, UNTRUSTED_KEY_PEM)
        .unwrap()
        .build()
        .unwrap()
}

fn s2n_server() -> server::Server {
    server::Builder::default()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .build()
        .unwrap()
}

fn s2n_server_with_client_auth() -> server::Server {
    server::Builder::default()
        .with_clear_trust_store()
        .unwrap()
        .with_client_authentication()
        .unwrap()
        .with_verify_host_callback(VerifyHostNameClientCertVerifier::new("qlaws.qlaws"))
        .unwrap()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .with_trust_client_certificate_signed_by(CERT_PEM)
        .unwrap()
        .build()
        .unwrap()
}

fn s2n_server_with_client_auth_verifier_rejects_client_certs() -> server::Server {
    server::Builder::default()
        .with_clear_trust_store()
        .unwrap()
        .with_client_authentication()
        .unwrap()
        .with_verify_host_callback(RejectAllClientsCertVerifier::new())
        .unwrap()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .with_trust_client_certificate_signed_by(CERT_PEM)
        .unwrap()
        .build()
        .unwrap()
}

fn rustls_server() -> s2n_quic_rustls::server::Server {
    s2n_quic_rustls::server::Builder::default()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .build()
        .unwrap()
}

fn rustls_client() -> s2n_quic_rustls::client::Client {
    s2n_quic_rustls::client::Builder::default()
        .with_certificate(CERT_PEM)
        .unwrap()
        .build()
        .unwrap()
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_s2n_server_test() {
    let mut client_endpoint = s2n_client();
    let mut server_endpoint = s2n_server();

    run(&mut server_endpoint, &mut client_endpoint);
}

#[test]
#[cfg_attr(miri, ignore)]
fn rustls_client_s2n_server_test() {
    let mut client_endpoint = rustls_client();
    let mut server_endpoint = s2n_server();

    run(&mut server_endpoint, &mut client_endpoint);
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_rustls_server_test() {
    let mut client_endpoint = s2n_client();
    let mut server_endpoint = rustls_server();

    run(&mut server_endpoint, &mut client_endpoint);
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_s2n_server_client_auth_test() {
    let mut client_endpoint = s2n_client_with_client_auth();
    let mut server_endpoint = s2n_server_with_client_auth();

    run(&mut server_endpoint, &mut client_endpoint);
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_no_client_auth_s2n_server_requires_client_auth_test() {
    let mut client_endpoint = s2n_client();
    let mut server_endpoint = s2n_server_with_client_auth();

    let test_result = run_result(&mut server_endpoint, &mut client_endpoint);

    // The handshake should fail because the server requires client auth,
    // but the client does not support it.
    assert!(test_result.is_err());
    let e = test_result.unwrap_err();
    assert_eq!(e.description().unwrap(), "HANDSHAKE_FAILURE");
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_with_client_auth_s2n_server_does_not_require_client_auth_test() {
    let mut client_endpoint = s2n_client_with_client_auth();
    let mut server_endpoint = s2n_server();

    let test_result = run_result(&mut server_endpoint, &mut client_endpoint);

    // The handshake should fail because the client requires client auth,
    // but the server does not support it.
    assert!(test_result.is_err());
    let e = test_result.unwrap_err();
    assert_eq!(e.description().unwrap(), "HANDSHAKE_FAILURE");
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_with_client_auth_s2n_server_does_not_trust_client_certificate() {
    let mut client_endpoint = s2n_client_with_client_auth();
    let mut server_endpoint = s2n_server_with_client_auth_verifier_rejects_client_certs();

    let test_result = run_result(&mut server_endpoint, &mut client_endpoint);

    // The handshake should fail because the certificate presented by the client is rejected by the
    // application level host verification check on the cert.
    assert!(test_result.is_err());
    let e = test_result.unwrap_err();
    assert_eq!(e.description().unwrap(), "HANDSHAKE_FAILURE");
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_with_client_auth_s2n_server_does_not_trust_issuer() {
    let mut client_endpoint = s2n_client_with_untrusted_client_auth();
    let mut server_endpoint = s2n_server_with_client_auth();

    let test_result = run_result(&mut server_endpoint, &mut client_endpoint);

    // The handshake should fail because the certificate presented by the client is issued
    // by a CA that is not in the server trust store, even though the host name is validated.
    assert!(test_result.is_err());
    let e = test_result.unwrap_err();
    assert_eq!(e.description().unwrap(), "HANDSHAKE_FAILURE");
}

/// Executes the handshake to completion
fn run_result<S: Endpoint, C: Endpoint>(
    server: &mut S,
    client: &mut C,
) -> Result<(), transport::Error> {
    let mut pair = tls::testing::Pair::new(server, client, "localhost".into());

    while pair.is_handshaking() {
        pair.poll()?;
    }

    pair.finish();
    Ok(())
}

/// Executes the handshake to completion
fn run<S: Endpoint, C: Endpoint>(server: &mut S, client: &mut C) {
    run_result(server, client).unwrap();
}
