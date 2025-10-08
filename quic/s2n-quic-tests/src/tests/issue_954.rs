// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;

/// Ensures that the client's local path handle is updated after it receives a packet from the
/// server
///
/// See https://github.com/aws/s2n-quic/issues/954
#[test]
fn client_path_handle_update() {
    let model = Model::default();

    let subscriber = recorder::PathUpdated::new();
    let events = subscriber.events();

    test(model.clone(), |handle| {
        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event(tracing_events(true, model.max_udp_payload()))?
            .with_random(Random::with_seed(456))?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event((tracing_events(true, model.max_udp_payload()), subscriber))?
            .with_random(Random::with_seed(456))?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(1000))?;
        Ok(addr)
    })
    .unwrap();

    let events_handle = events.lock().unwrap();

    // initially, the client address should be unknown
    assert_eq!(events_handle[0], "0.0.0.0:0".parse().unwrap());
    // after receiving a packet, the client port should be the first available ephemeral port
    assert_eq!(events_handle[1], "1.0.0.1:49153".parse().unwrap());
    // there should only be a single update to the path handle
    assert_eq!(events_handle.len(), 2);
}
