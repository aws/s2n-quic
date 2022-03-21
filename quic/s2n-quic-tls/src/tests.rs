// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{client, server};
use core::{
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
    task::Poll,
};
use s2n_quic_core::crypto::tls::{
    self,
    testing::certificates::{CERT_PEM, KEY_PEM},
    Endpoint,
};
#[cfg(any(test, all(s2n_quic_unstable, feature = "unstable_client_hello")))]
use s2n_tls::raw::{config::ClientHelloHandler, connection::Connection};
use std::sync::Arc;

pub struct MyClientHelloHandler {
    done: Arc<AtomicBool>,
    wait_counter: Arc<AtomicU8>,
}

impl MyClientHelloHandler {
    fn new(wait_counter: u8) -> Self {
        MyClientHelloHandler {
            done: Arc::new(AtomicBool::new(false)),
            wait_counter: Arc::new(AtomicU8::new(wait_counter)),
        }
    }
}

#[cfg(any(test, all(s2n_quic_unstable, feature = "unstable_client_hello")))]
impl ClientHelloHandler for MyClientHelloHandler {
    fn poll_client_hello(&self, _connection: &mut Connection) -> core::task::Poll<Result<(), ()>> {
        if self.wait_counter.fetch_sub(1, Ordering::SeqCst) == 0 {
            self.done.store(true, Ordering::SeqCst);
            return Poll::Ready(Ok(()));
        }

        Poll::Pending
    }
}

fn s2n_client() -> client::Client {
    client::Builder::default()
        .with_certificate(CERT_PEM)
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

#[cfg(any(test, all(s2n_quic_unstable, feature = "unstable_client_hello")))]
fn s2n_server_with_client_hello_callback(wait_counter: u8) -> (server::Server, Arc<AtomicBool>) {
    let handle = MyClientHelloHandler::new(wait_counter);
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

/// Executes the handshake to completion
fn run<S: Endpoint, C: Endpoint>(
    server: &mut S,
    client: &mut C,
    client_hello_cb_done: Option<Arc<AtomicBool>>,
) {
    let mut pair = tls::testing::Pair::new(server, client, "localhost".into());

    while pair.is_handshaking() {
        pair.poll(client_hello_cb_done.clone()).unwrap();
    }

    pair.finish();
}
