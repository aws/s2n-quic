// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[cfg(unix)]
#[test]
fn tls_connection_has_server_local_address_test() {
    let model = Model::default();

    let tls_server_session_created_subscriber = recorder::TlsServerSessionCreated::new();
    let tls_server_session_created_events = tls_server_session_created_subscriber.clone();
    let actual_server_addr = Arc::new(Mutex::new(None));
    let actual_server_addr_clone = actual_server_addr.clone();

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((
                tracing_events(true, model.clone()),
                tls_server_session_created_subscriber,
            ))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let server_addr = start_server(server).unwrap();

        //record server address for verification purpose
        *actual_server_addr_clone.lock().unwrap() = Some(server_addr);

        let client = build_client(handle, model.clone(), true)?;

        start_client(client, server_addr, Data::new(1000)).unwrap();

        Ok(())
    })
    .unwrap();

    // Verify one address was captured
    let events = tls_server_session_created_events.events.lock().unwrap();
    assert_eq!(events.len(), 1);

    // Verify the server's address set in the TLS connection is correct
    let recorded_addr = events[0].unwrap();
    let actual_addr = actual_server_addr
        .lock()
        .unwrap()
        .expect("Server address should have been recorded");
    assert_eq!(recorded_addr, actual_addr);
}
