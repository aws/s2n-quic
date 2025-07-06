// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::provider::{
    limits,
    tls::default::{self as tls},
};

#[test]
fn zero_length_cid_client_connection_migration_test() {
    let model = Model::default();

    // Create event subscribers to track frame received events
    let initial_cid_subscriber = recorder::InitialCryptoFrameReceived::new();
    let initial_cid_event = initial_cid_subscriber.events();
    let path_challenge_subscriber = recorder::PathChallengeUpdated::new();
    let path_challenge_event = path_challenge_subscriber.events();

    test(model, |handle| {
        // Set up a s2n-quic server
        let server = tls::Server::builder()
            .with_application_protocols(["h3"].iter())
            .unwrap()
            .with_certificate(certificates::CERT_PEM, certificates::KEY_PEM)
            .unwrap()
            .build()
            .unwrap();

        let server = Server::builder()
            .with_io(handle.builder().build()?)?
            .with_tls(server)?
            .with_event((
                tracing_events(),
                (initial_cid_subscriber, path_challenge_subscriber),
            ))?
            .with_random(Random::with_seed(456))?
            .with_limits(limits::Limits::new().with_max_active_connection_ids(3)?)?
            .start()?;

        let server_addr = start_server(server)?;

        // Set up a Cloudflare Quiche client
        let mut client_config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
        client_config
            .set_application_protos(quiche::h3::APPLICATION_PROTOCOL)
            .unwrap();
        client_config.verify_peer(false);
        client_config.set_initial_max_data(10_000_000);
        client_config.set_initial_max_stream_data_bidi_local(1_000_000);
        client_config.set_initial_max_stream_data_bidi_remote(1_000_000);
        client_config.set_disable_active_migration(false);
        client_config.set_active_connection_id_limit(5);

        // create a zero-length Source CID
        let scid = quiche::ConnectionId::default();

        let socket = handle.builder().build()?.socket();
        let migrated_socket = handle.builder().build()?.socket();

        // Create a QUIC connection and initiate handshake.
        let conn = quiche::connect(
            Some("localhost"),
            &scid,
            socket.local_addr().unwrap(),
            server_addr,
            &mut client_config,
        )
        .unwrap();

        // Check if the client is using zero-length CID
        assert_eq!(conn.source_id().len(), 0);

        start_quiche_client(conn, socket, migrated_socket, server_addr).unwrap();

        Ok(())
    })
    .unwrap();

    let initial_cid_vec = initial_cid_event.lock().unwrap();
    // The server should only perform one handshake with a successful
    // connection mgiration. Hence, it should only receive one Initial packet
    // with Crypto frame
    assert_eq!(initial_cid_vec.len(), 1);
    // Verify if the client's original CID is zero-length
    assert_eq!(initial_cid_vec[0].len(), 0);

    // Verify if the new path is validated
    let path_challenge_statuses = path_challenge_event.lock().unwrap();
    let path_validated = path_challenge_statuses
        .iter()
        .any(|status| matches!(status, events::PathChallengeStatus::Validated { .. }));
    assert!(path_validated);
}
