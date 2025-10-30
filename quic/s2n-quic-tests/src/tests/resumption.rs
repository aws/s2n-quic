// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Tests resumption handshake
#[cfg(unix)]
#[test]
fn resumption_handshake() {
    use super::*;
    use crate::resumption::*;

    let model = Model::default();
    let handler = SessionTicketHandler::default();

    // The client and server do a single handshake in order to
    // negotiate a session ticket.
    test(model.clone(), |handle| {
        let server_tls =
            build_server_resumption_provider(certificates::CERT_PEM, certificates::KEY_PEM)?;
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_event(tracing_events(true, model.clone()))?
            .with_tls(server_tls)?
            .start()?;

        let client_tls = build_client_resumption_provider(certificates::CERT_PEM, &handler)?;
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(client_tls)?
            .with_event(tracing_events(true, model.clone()))?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;

        Ok(addr)
    })
    .unwrap();

    // The second handshake will be a resumption handshake now that the client has a session ticket
    // available. The handshake succeeds even though the client doesn't have the correct certificate
    // to authenticate the server.
    let model = Model::default();
    test(model.clone(), |handle| {
        let client_tls = build_client_resumption_provider(certificates::CERT_PEM, &handler)?;
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(client_tls)?
            .with_event(tracing_events(true, model.clone()))?
            .start()?;

        let server_tls = build_server_resumption_provider(
            certificates::UNTRUSTED_CERT_PEM,
            certificates::UNTRUSTED_KEY_PEM,
        )?;
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(server_tls)?
            .with_event(tracing_events(true, model.clone()))?
            .start()?;
        let addr = start_server(server)?;

        start_client(client, addr, Data::new(1000))?;
        Ok(addr)
    })
    .unwrap();
}
