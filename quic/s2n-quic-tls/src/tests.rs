// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{certificate, client, server};
use core::{
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
    task::Poll,
};
use openssl::{ec::EcKey, ecdsa::EcdsaSig};
use pin_project::pin_project;
use s2n_quic_core::{
    crypto::tls::{
        self,
        testing::certificates::{CERT_PEM, KEY_PEM, UNTRUSTED_CERT_PEM, UNTRUSTED_KEY_PEM},
        Endpoint,
    },
    transport,
};
#[cfg(any(test, all(s2n_quic_unstable, feature = "unstable_client_hello")))]
use s2n_tls::callbacks::ClientHelloCallback;
#[cfg(any(test, all(s2n_quic_unstable, feature = "unstable_client_hello")))]
use s2n_tls::callbacks::{PrivateKeyCallback, PrivateKeyOperation};
use s2n_tls::{
    callbacks::{ConnectionFuture, VerifyHostNameCallback},
    connection::Connection,
    enums::ClientAuthType,
    error::Error,
};
use std::sync::Arc;

pub struct MyCallbackHandler {
    done: Arc<AtomicBool>,
    wait_counter: Arc<AtomicU8>,
}

impl MyCallbackHandler {
    fn new(wait_counter: u8) -> Self {
        MyCallbackHandler {
            done: Arc::new(AtomicBool::new(false)),
            wait_counter: Arc::new(AtomicU8::new(wait_counter)),
        }
    }
}

#[cfg(any(test, all(s2n_quic_unstable, feature = "unstable_client_hello")))]
impl ClientHelloCallback for MyCallbackHandler {
    fn on_client_hello(
        &self,
        _connection: &mut Connection,
    ) -> Result<Option<std::pin::Pin<Box<dyn s2n_tls::callbacks::ConnectionFuture>>>, Error> {
        let fut = MyConnectionFuture {
            done: self.done.clone(),
            wait_counter: self.wait_counter.clone(),
        };
        Ok(Some(Box::pin(fut)))
    }
}

struct MyConnectionFuture {
    done: Arc<AtomicBool>,
    wait_counter: Arc<AtomicU8>,
}

impl ConnectionFuture for MyConnectionFuture {
    fn poll(
        self: std::pin::Pin<&mut Self>,
        _connection: &mut Connection,
        _ctx: &mut core::task::Context,
    ) -> Poll<Result<(), Error>> {
        if self.wait_counter.fetch_sub(1, Ordering::SeqCst) == 0 {
            self.done.store(true, Ordering::SeqCst);
            return Poll::Ready(Ok(()));
        }

        Poll::Pending
    }
}

#[cfg(any(test, all(s2n_quic_unstable, feature = "unstable_private_key")))]
impl PrivateKeyCallback for MyCallbackHandler {
    fn handle_operation(
        &self,
        _connection: &mut Connection,
        op: PrivateKeyOperation,
    ) -> Result<Option<std::pin::Pin<Box<dyn s2n_tls::callbacks::ConnectionFuture>>>, Error> {
        let future = MyConnectionFuture {
            done: self.done.clone(),
            wait_counter: self.wait_counter.clone(),
        };
        let op = Some(op);
        let pkey_future = MyPrivateKeyFuture { future, op };
        Ok(Some(Box::pin(pkey_future)))
    }
}

#[pin_project]
struct MyPrivateKeyFuture {
    #[pin]
    future: MyConnectionFuture,
    #[pin]
    op: Option<PrivateKeyOperation>,
}

impl ConnectionFuture for MyPrivateKeyFuture {
    fn poll(
        self: std::pin::Pin<&mut Self>,
        conn: &mut Connection,
        ctx: &mut core::task::Context,
    ) -> Poll<Result<(), Error>> {
        let mut this = self.project();
        if this.future.poll(conn, ctx).is_pending() {
            return Poll::Pending;
        }

        let op = this.op.take().expect("Missing pkey operation");
        let in_buf_size = op.input_size()?;
        let mut in_buf = vec![0; in_buf_size];
        op.input(&mut in_buf)?;

        let key = EcKey::private_key_from_pem(KEY_PEM.as_bytes())
            .expect("Failed to create EcKey from pem");
        let sig = EcdsaSig::sign(&in_buf, &key).expect("Failed to sign input");
        let out = sig.to_der().expect("Failed to convert signature to der");

        op.set_output(conn, &out)?;
        Poll::Ready(Ok(()))
    }
}

pub struct VerifyHostNameClientCertVerifier {
    host_name: String,
}

impl VerifyHostNameCallback for VerifyHostNameClientCertVerifier {
    fn verify_host_name(&self, host_name: &str) -> bool {
        self.host_name == host_name
    }
}

impl VerifyHostNameClientCertVerifier {
    pub fn new(host_name: impl ToString) -> VerifyHostNameClientCertVerifier {
        VerifyHostNameClientCertVerifier {
            host_name: host_name.to_string(),
        }
    }
}

#[derive(Default)]
pub struct RejectAllClientCertificatesHandler {}
impl VerifyHostNameCallback for RejectAllClientCertificatesHandler {
    fn verify_host_name(&self, _host_name: &str) -> bool {
        false
    }
}

fn s2n_client() -> client::Client {
    client::Builder::default()
        .with_certificate(CERT_PEM)
        .unwrap()
        .build()
        .unwrap()
}

fn s2n_client_with_client_auth() -> Result<client::Client, Error> {
    client::Builder::default()
        .with_empty_trust_store()?
        .with_certificate(CERT_PEM)?
        .with_client_identity(CERT_PEM, KEY_PEM)?
        .build()
}

fn s2n_client_with_untrusted_client_auth() -> Result<client::Client, Error> {
    client::Builder::default()
        .with_empty_trust_store()?
        .with_certificate(CERT_PEM)?
        .with_client_identity(UNTRUSTED_CERT_PEM, UNTRUSTED_KEY_PEM)?
        .build()
}

fn s2n_client_with_fixed_hostname_auth(host_name: &str) -> Result<client::Client, Error> {
    client::Builder::default()
        .with_certificate(CERT_PEM)
        .unwrap()
        .with_verify_host_name_callback(VerifyHostNameClientCertVerifier::new(host_name))
        .unwrap()
        .build()
}

fn s2n_server() -> server::Server {
    server::Builder::default()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .build()
        .unwrap()
}

fn s2n_server_with_client_auth() -> Result<server::Server, Error> {
    server::Builder::default()
        .with_empty_trust_store()?
        .with_client_authentication()?
        .with_verify_host_name_callback(VerifyHostNameClientCertVerifier::new("qlaws.qlaws"))?
        .with_certificate(CERT_PEM, KEY_PEM)?
        .with_trusted_certificate(CERT_PEM)?
        .build()
}

fn s2n_server_with_optional_client_auth() -> Result<server::Server, Error> {
    server::Builder::default()
        .with_empty_trust_store()?
        .with_client_auth_type(ClientAuthType::Optional)?
        .with_verify_host_name_callback(VerifyHostNameClientCertVerifier::new("qlaws.qlaws"))?
        .with_certificate(CERT_PEM, KEY_PEM)?
        .with_trusted_certificate(CERT_PEM)?
        .build()
}

fn s2n_server_with_none_client_auth() -> Result<server::Server, Error> {
    server::Builder::default()
        .with_empty_trust_store()?
        .with_client_auth_type(ClientAuthType::None)?
        .with_verify_host_name_callback(VerifyHostNameClientCertVerifier::new("qlaws.qlaws"))?
        .with_certificate(CERT_PEM, KEY_PEM)?
        .with_trusted_certificate(CERT_PEM)?
        .build()
}

fn s2n_server_with_client_auth_verifier_rejects_client_certs() -> Result<server::Server, Error> {
    server::Builder::default()
        .with_empty_trust_store()?
        .with_client_authentication()?
        .with_verify_host_name_callback(RejectAllClientCertificatesHandler::default())?
        .with_certificate(CERT_PEM, KEY_PEM)?
        .with_trusted_certificate(CERT_PEM)?
        .build()
}

#[cfg(any(test, all(s2n_quic_unstable, feature = "unstable_client_hello")))]
fn s2n_server_with_client_hello_callback(wait_counter: u8) -> (server::Server, Arc<AtomicBool>) {
    let handle = MyCallbackHandler::new(wait_counter);
    let done = handle.done.clone();
    let tls = server::Builder::default()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .with_client_hello_handler(handle)
        .unwrap()
        .build()
        .unwrap();
    (tls, done)
}

#[cfg(any(test, all(s2n_quic_unstable, feature = "unstable_private_key")))]
fn s2n_server_with_private_key_callback(wait_counter: u8) -> (server::Server, Arc<AtomicBool>) {
    let handle = MyCallbackHandler::new(wait_counter);
    let done = handle.done.clone();
    let tls = server::Builder::default()
        .with_certificate(CERT_PEM, certificate::OFFLOAD_PRIVATE_KEY)
        .unwrap()
        .with_private_key_handler(handle)
        .unwrap()
        .build()
        .unwrap();
    (tls, done)
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
fn s2n_client_s2n_server_ch_callback_test() {
    for wait_counter in 0..=10 {
        let mut client_endpoint = s2n_client();
        let (mut server_endpoint, done) = s2n_server_with_client_hello_callback(wait_counter);

        run(&mut server_endpoint, &mut client_endpoint, Some(done));
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_s2n_server_pkey_callback_test() {
    for wait_counter in 0..=10 {
        let mut client_endpoint = s2n_client();
        let (mut server_endpoint, done) = s2n_server_with_private_key_callback(wait_counter);

        run(&mut server_endpoint, &mut client_endpoint, Some(done));
    }
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_s2n_server_test() {
    let mut client_endpoint = s2n_client();
    let mut server_endpoint = s2n_server();

    run(&mut server_endpoint, &mut client_endpoint, None);
}

#[test]
#[cfg_attr(miri, ignore)]
fn rustls_client_s2n_server_test() {
    let mut client_endpoint = rustls_client();
    let mut server_endpoint = s2n_server();

    run(&mut server_endpoint, &mut client_endpoint, None);
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_rustls_server_test() {
    let mut client_endpoint = s2n_client();
    let mut server_endpoint = rustls_server();

    run(&mut server_endpoint, &mut client_endpoint, None);
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_s2n_server_client_auth_test() {
    let mut client_endpoint = s2n_client_with_client_auth().unwrap();
    let mut server_endpoint = s2n_server_with_client_auth().unwrap();

    run(&mut server_endpoint, &mut client_endpoint, None);
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_no_client_auth_s2n_server_requires_client_auth_test() {
    let mut client_endpoint = s2n_client();
    let mut server_endpoint = s2n_server_with_client_auth().unwrap();

    let test_result = run_result(&mut server_endpoint, &mut client_endpoint, None);

    // The handshake should fail because the server requires client auth,
    // but the client does not support it.
    assert!(test_result.is_err());
    let e = test_result.unwrap_err();
    assert_eq!(e.description().unwrap(), "UNEXPECTED_MESSAGE");
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_no_client_auth_s2n_server_optional_client_auth_test() {
    let mut client_endpoint = s2n_client();
    let mut server_endpoint = s2n_server_with_optional_client_auth().unwrap();

    let test_result = run_result(&mut server_endpoint, &mut client_endpoint, None);

    assert!(test_result.is_err());
    let e = test_result.unwrap_err();
    assert_eq!(e.description().unwrap(), "UNEXPECTED_MESSAGE");
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_no_client_auth_s2n_server_none_client_auth_test() {
    let mut client_endpoint = s2n_client();
    let mut server_endpoint = s2n_server_with_none_client_auth().unwrap();

    run(&mut server_endpoint, &mut client_endpoint, None);
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_with_client_auth_s2n_server_does_not_require_client_auth_test() {
    let mut client_endpoint = s2n_client_with_client_auth().unwrap();
    let mut server_endpoint = s2n_server();

    let test_result = run_result(&mut server_endpoint, &mut client_endpoint, None);

    // The handshake should fail because the client requires client auth,
    // but the server does not support it.
    assert!(test_result.is_err());
    let e = test_result.unwrap_err();
    assert_eq!(e.description().unwrap(), "UNEXPECTED_MESSAGE");
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_with_client_auth_s2n_server_does_not_trust_client_certificate() {
    let mut client_endpoint = s2n_client_with_client_auth().unwrap();
    let mut server_endpoint = s2n_server_with_client_auth_verifier_rejects_client_certs().unwrap();

    let test_result = run_result(&mut server_endpoint, &mut client_endpoint, None);

    // The handshake should fail because the certificate presented by the client is rejected by the
    // application level host verification check on the cert.
    assert!(test_result.is_err());
    let e = test_result.unwrap_err();
    assert_eq!(e.description().unwrap(), "HANDSHAKE_FAILURE");
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_with_client_auth_s2n_server_does_not_trust_issuer() {
    let mut client_endpoint = s2n_client_with_untrusted_client_auth().unwrap();
    let mut server_endpoint = s2n_server_with_client_auth().unwrap();

    let test_result = run_result(&mut server_endpoint, &mut client_endpoint, None);

    // The handshake should fail because the certificate presented by the client is issued
    // by a CA that is not in the server trust store, even though the host name is validated.
    assert!(test_result.is_err());
    let e = test_result.unwrap_err();
    assert_eq!(e.description().unwrap(), "HANDSHAKE_FAILURE");
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_with_custom_hostname_auth_rejects_server_name() {
    let mut client_endpoint = s2n_client_with_fixed_hostname_auth("not-localhost").unwrap();
    let mut server_endpoint = s2n_server();

    let test_result = run_result(&mut server_endpoint, &mut client_endpoint, None);

    // The handshake should fail because the hostname ("localhost") is not validated
    assert!(test_result.is_err());
    let e = test_result.unwrap_err();
    assert_eq!(e.description().unwrap(), "HANDSHAKE_FAILURE");
}

#[test]
#[cfg_attr(miri, ignore)]
fn s2n_client_with_custom_hostname_auth_accepts_server_name() {
    let mut client_endpoint = s2n_client_with_fixed_hostname_auth("localhost").unwrap();
    let mut server_endpoint = s2n_server();

    run_result(&mut server_endpoint, &mut client_endpoint, None).unwrap();
}

/// Executes the handshake to completion
fn run_result<S: Endpoint, C: Endpoint>(
    server: &mut S,
    client: &mut C,
    client_hello_cb_done: Option<Arc<AtomicBool>>,
) -> Result<(), transport::Error> {
    let mut pair = tls::testing::Pair::new(server, client, "localhost".into());

    while pair.is_handshaking() {
        pair.poll(client_hello_cb_done.clone())?;
    }

    pair.finish();
    Ok(())
}

/// Executes the handshake to completion
fn run<S: Endpoint, C: Endpoint>(
    server: &mut S,
    client: &mut C,
    client_hello_cb_done: Option<Arc<AtomicBool>>,
) {
    run_result(server, client, client_hello_cb_done).unwrap();
}

#[test]
fn config_loader() {
    use crate::{ConfigLoader, Server};

    let server = Server::default();

    // make sure the loader can be a static type
    let server: Server<Server> = Server::from_loader(server);

    // make sure the loader can be a dynamic type
    let server: Box<dyn ConfigLoader> = Box::new(server);
    let mut server: Server<Box<dyn ConfigLoader>> = Server::from_loader(server);

    // make sure the server can actually create a session
    let _ = server.new_server_session(&1);
}
