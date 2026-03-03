// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic::provider::tls::ConnectionInfo;
use s2n_tls::{
    callbacks::{ClientHelloCallback, ConnectionFuture},
    error::Error as S2nError,
};
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
};

struct TestClientHelloHandle {
    // The ClientHelloCallback trait requires `&self` as a immutable reference.
    // We use Arc<Mutex<>> to enable interior mutability - allowing us to mutate the recorded
    // ConnectionInfo through an immutable reference.
    recorded_info: Arc<Mutex<Option<ConnectionInfo>>>,
}

impl TestClientHelloHandle {
    pub fn new(recorded_info: Arc<Mutex<Option<ConnectionInfo>>>) -> Self {
        Self { recorded_info }
    }
}

impl ClientHelloCallback for TestClientHelloHandle {
    fn on_client_hello(
        &self,
        connection: &mut s2n_tls::connection::Connection,
    ) -> Result<Option<Pin<Box<dyn ConnectionFuture>>>, S2nError> {
        let connection_info = connection.application_context::<ConnectionInfo>();

        assert!(connection_info.is_some());
        if let Some(info) = connection_info {
            *self.recorded_info.lock().unwrap() = Some(*info);
        }

        Ok(None)
    }
}

/// Tests that ConnectionInfo is accessible in the client hello callback and contains
/// the correct local (server) and remote (client) socket addresses.
///
/// This test:
/// 1. Creates a server with a client hello callback that records ConnectionInfo
/// 2. Records the actual server and client socket addresses during connection setup
/// 3. Verifies that the ConnectionInfo captured in the callback matches the expected addresses
///
/// Note: Uses interior mutability (Arc<Mutex<>>) to store data from the callback since
/// ClientHelloCallback requires an immutable reference (&self).
#[test]
#[cfg_attr(miri, ignore)]
fn ch_callback_connection_info_test() {
    let model = Model::default();

    let ch_callback_handle_inner = Arc::new(Mutex::new(None));
    let ch_callback_handle_inner_clone = ch_callback_handle_inner.clone();

    let mut server_local_address = None;
    let mut server_remote_address = None;

    test(model.clone(), |handle| {
        let server_tls = tls::s2n_tls::Server::builder()
            .with_certificate(certificates::CERT_PEM, certificates::KEY_PEM)
            .unwrap()
            .with_client_hello_handler(TestClientHelloHandle::new(ch_callback_handle_inner_clone))
            .unwrap()
            .build()
            .unwrap();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(server_tls)?
            .with_event(tracing_events(true, model.clone()))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let server_addr = start_server(server)?;
        server_local_address = Some(server_addr);

        let client = build_client(handle, model.clone(), true)?;
        server_remote_address = Some(client.local_addr().unwrap());

        start_client(client, server_addr, Data::new(1000))?;

        Ok(server_addr)
    })
    .unwrap();

    let connection_info = ch_callback_handle_inner.lock().unwrap().unwrap();

    // Verify that the ConnectionInfo contains the exact server local address
    assert_eq!(connection_info.local_address, server_local_address.unwrap());

    // Verify that the ConnectionInfo contains the exact server's remote address (client's local address)
    assert_eq!(
        connection_info.remote_address,
        server_remote_address.unwrap()
    );
}
