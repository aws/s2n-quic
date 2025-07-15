// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic::provider::connection_id;
use s2n_quic_core::event::api::{Frame, FrameSent};

// Configure the server and client with the given `rotate_handshake_connection_id` setting
// and complete a handshake
fn rotate_handshake_test(
    server_rotate_handshake_connection_id: bool,
    client_rotate_handshake_connection_id: bool,
) -> (Vec<events::FrameSent>, Vec<events::FrameSent>) {
    let model = Model::default();

    let server_subscriber = recorder::FrameSent::new();
    let server_events = server_subscriber.events();
    let client_subscriber = recorder::FrameSent::new();
    let client_events = client_subscriber.events();

    test(model, |handle| {
        let server_cid_generator = connection_id::default::Format::builder()
            .with_handshake_connection_id_rotation(server_rotate_handshake_connection_id)?
            .build()?;

        let client_cid_generator = connection_id::default::Format::builder()
            .with_handshake_connection_id_rotation(client_rotate_handshake_connection_id)?
            .build()?;

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(SERVER_CERTS)?
            .with_event((tracing_events(), server_subscriber))?
            .with_random(Random::with_seed(456))?
            .with_connection_id(server_cid_generator)?
            .start()?;
        let client = Client::builder()
            .with_io(handle.builder().build().unwrap())?
            .with_tls(certificates::CERT_PEM)?
            .with_event((tracing_events(), client_subscriber))?
            .with_random(Random::with_seed(456))?
            .with_connection_id(client_cid_generator)?
            .start()?;
        let addr = start_server(server)?;
        start_client(client, addr, Data::new(10000))?;
        Ok(addr)
    })
    .unwrap();

    let server_handle = server_events.lock().unwrap();
    let client_handle = client_events.lock().unwrap();
    (server_handle.clone(), client_handle.clone())
}

#[test]
fn server_enabled_client_enabled() {
    let (server_events, client_events) = rotate_handshake_test(true, true);

    // Both server and client request to retire the handshake CID
    assert_retire_prior_to(&server_events, 1);
    assert_retire_prior_to(&client_events, 1);

    // Both server and client retire the handshake CID as requested
    assert_retire_connection_id_count(&server_events, 1);
    assert_retire_connection_id_count(&client_events, 1);
}

#[test]
fn server_enabled_client_disabled() {
    let (server_events, client_events) = rotate_handshake_test(true, false);

    // Only the server requests to retire the handshake CID
    assert_retire_prior_to(&server_events, 1);
    assert_retire_prior_to(&client_events, 0);

    // The client retires the handshake CID as requested.
    // The server still retires the handshake CID because it had rotate_handshake_connection_id enabled
    assert_retire_connection_id_count(&server_events, 1);
    assert_retire_connection_id_count(&client_events, 1);
}

#[test]
fn server_disabled_client_enabled() {
    let (server_events, client_events) = rotate_handshake_test(false, true);

    // Only the client requests to retire the handshake CID
    assert_retire_prior_to(&server_events, 0);
    assert_retire_prior_to(&client_events, 1);

    // The server retires the handshake CID as requested.
    // The client still retires the handshake CID because it had rotate_handshake_connection_id enabled
    assert_retire_connection_id_count(&server_events, 1);
    assert_retire_connection_id_count(&client_events, 1);
}

#[test]
fn server_disabled_client_disabled() {
    let (server_events, client_events) = rotate_handshake_test(false, false);

    // Neither server nor client requests to retire the handshake CID
    assert_retire_prior_to(&server_events, 0);
    assert_retire_prior_to(&client_events, 0);

    // No connection IDs are retired
    assert_retire_connection_id_count(&server_events, 0);
    assert_retire_connection_id_count(&client_events, 0);
}

fn assert_retire_prior_to(events: &[FrameSent], expected: u64) {
    for frame_sent in events {
        if let Frame::NewConnectionId {
            retire_prior_to, ..
        } = frame_sent.frame
        {
            assert_eq!(retire_prior_to, expected);
        }
    }
}

fn assert_retire_connection_id_count(events: &[FrameSent], expected: usize) {
    let retire_connection_id_count = events
        .iter()
        .filter(|frame_sent| matches!(frame_sent.frame, Frame::RetireConnectionId { .. }))
        .count();
    assert_eq!(retire_connection_id_count, expected);
}
