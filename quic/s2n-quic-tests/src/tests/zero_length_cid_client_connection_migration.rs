// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use s2n_quic::provider::{
    io::testing::Result,
    tls::default::{self as tls},
};
use s2n_quic_core::{connection::error::Error, endpoint, inet::ExplicitCongestionNotification::*};
use s2n_quic_platform::io::testing::Socket;
use zerocopy::IntoBytes;

const QUICHE_MAX_DATAGRAM_SIZE: usize = 1350;
const QUICHE_STREAM_ID: u64 = 0;

// Test Description:
// Verifies that an s2n-quic server can handle connection migration from a client using zero-length Connection IDs (CID)
//
// Test Setup:
// - Uses Cloudflare Quiche as the client (since s2n-quic client doesn't support zero-length CIDs)
// - Quiche client is configured to use zero-length CIDs
//
// Test Flow:
// 1. Client initiates handshake with s2n-quic server
// 2. After successful handshake, client performs connection migration to a new address
// 3. Client sends a test string to server post-migration
// 4. Client closes connection after receiving the test string which is echoed back from the server
//
// Verification Points:
// 1. Confirm client is using zero-length CID
// 2. Verify the active path is updated to the correct socket address
// 3. Verify the server close the connection with no error
#[test]
fn zero_length_cid_client_connection_migration_test() {
    let model = Model::default();

    let active_path_update_subscriber = recorder::ActivePathUpdated::new();
    let active_path_update_event = active_path_update_subscriber.events();
    let connection_close_subscriber = recorder::ConnectionClosed::new();
    let connection_close_event = connection_close_subscriber.events();

    // The target address for connection migration
    // It is initially an unspecified address. It will be updated during test to verify connection migration
    let mut migrated_socket_address: SocketAddr = "0.0.0.0:0".parse().unwrap();

    test(model.clone(), |handle| {
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
                tracing_events(true, model.clone()),
                (active_path_update_subscriber, connection_close_subscriber),
            ))?
            .with_random(Random::with_seed(456))?
            .start()?;

        let server_addr = start_server(server)?;

        // Set up a Cloudflare Quiche client
        let mut client_config = quiche::Config::new(quiche::PROTOCOL_VERSION).unwrap();
        client_config
            .set_application_protos(quiche::h3::APPLICATION_PROTOCOL)
            .unwrap();
        client_config.verify_peer(false);

        // The client sends a 14-byte steam data message in this test
        // Set 20 bytes for the maximum amount of data sent on the client created stream is enough
        client_config.set_initial_max_data(20);
        client_config.set_initial_max_stream_data_bidi_local(20);
        client_config.set_disable_active_migration(false);

        // create a zero-length Source CID
        let scid = quiche::ConnectionId::default();

        let socket = handle.builder().build()?.socket();
        let migrated_socket = handle.builder().build()?.socket();

        migrated_socket_address = migrated_socket.local_addr().unwrap();

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

    // Verify the active path is update to the migrated socket address
    let active_path_update_handle = active_path_update_event.lock().unwrap();
    assert_eq!(active_path_update_handle.len(), 1);
    let updated_active_path_remote_addr = active_path_update_handle[0];
    assert_eq!(updated_active_path_remote_addr, migrated_socket_address);

    // Verify the client closes the connection with no error
    let connection_close_status = connection_close_event.lock().unwrap();
    assert_eq!(connection_close_status.len(), 1);
    assert!(matches!(
        connection_close_status[0],
        Error::Closed {
            initiator: endpoint::Location::Remote,
            ..
        }
    ));
}

// Take reference from https://github.com/cloudflare/quiche/blob/master/quiche/examples/client.rs
// and https://github.com/cloudflare/quiche/blob/master/apps/src/client.rs
pub fn start_quiche_client(
    mut client_conn: quiche::Connection,
    socket: Socket,
    migrated_socket: Socket,
    server_addr: SocketAddr,
) -> Result<()> {
    let mut out = [0; QUICHE_MAX_DATAGRAM_SIZE];
    let mut buf = [0; QUICHE_MAX_DATAGRAM_SIZE];
    let application_data = "Test Migration";

    primary::spawn(async move {
        client_conn.timeout();

        // Write Initial handshake packets
        let (write, send_info) = client_conn.send(&mut out).expect("Initial send failed");
        socket
            .send_to(send_info.to, NotEct, out[..write].to_vec())
            .unwrap();

        let mut path_probed = false;
        let mut req_sent = false;
        let mut stream_data_received = false;
        loop {
            // We need to check if there is a timeout event at the beginning of
            // each loop to make sure that the connection will close properly when
            // the test is done.
            client_conn.on_timeout();
            // Quiche doesn't handle IO. So we need to handle events that
            // happen on both the original socket and the migrated socket
            for active_socket in [&socket, &migrated_socket] {
                let local_addr = active_socket.local_addr().unwrap();
                match active_socket.try_recv_from() {
                    Ok(Some((from, _ecn, payload))) => {
                        // Quiche conn.recv requires a mutable payload array
                        let mut payload_copy = payload.clone();

                        // Feed received data from IO Socket to Quiche
                        let _read = match client_conn.recv(
                            &mut payload_copy,
                            quiche::RecvInfo {
                                from,
                                to: active_socket.local_addr().unwrap(),
                            },
                        ) {
                            Ok(v) => v,
                            Err(quiche::Error::Done) => 0,
                            Err(e) => {
                                panic!("quiche client receive error: {e:?}");
                            }
                        };
                    }
                    Ok(None) => {}
                    Err(e) => {
                        panic!("quiche client socket recv error: {e:?}");
                    }
                }

                for peer_addr in client_conn.paths_iter(local_addr) {
                    loop {
                        let (write, send_info) = match client_conn.send_on_path(
                            &mut out,
                            Some(local_addr),
                            Some(peer_addr),
                        ) {
                            Ok(v) => v,
                            Err(quiche::Error::Done) => {
                                break;
                            }
                            Err(e) => {
                                panic!("quiche client send error: {e:?}")
                            }
                        };

                        active_socket
                            .send_to(send_info.to, NotEct, out[..write].to_vec())
                            .unwrap();
                    }

                    // Send application data using the migrated address
                    // This can only be done once the connection migration is completed
                    if local_addr == migrated_socket.local_addr().unwrap()
                        && client_conn
                            .is_path_validated(local_addr, peer_addr)
                            .unwrap()
                        && !req_sent
                    {
                        client_conn
                            .stream_send(QUICHE_STREAM_ID, application_data.as_bytes(), true)
                            .unwrap();
                        req_sent = true;
                    }
                }

                for stream_id in client_conn.readable() {
                    while let Ok((read, _)) = client_conn.stream_recv(stream_id, &mut buf) {
                        let stream_buf = &buf[..read];
                        // The data that the Quiche client received should be the same that it sent
                        assert_eq!(stream_buf.as_bytes(), application_data.as_bytes());
                        stream_data_received = true;
                        // The test is done once the client receives the data. Hence, close the connection
                        client_conn.close(false, 0x00, b"test finished").unwrap();
                    }
                }
            }

            // Exit the test once the connection is closed and receive no error from the server
            if client_conn.is_closed() {
                assert!(client_conn.peer_error().is_none());
                assert!(stream_data_received);
                break;
            }

            // Probe a new path after the server provides spare CIDs
            if client_conn.available_dcids() > 0 && !path_probed {
                let new_addr = migrated_socket.local_addr().unwrap();
                client_conn.probe_path(new_addr, server_addr).unwrap();
                path_probed = true;
            }

            while let Some(qe) = client_conn.path_event_next() {
                if let quiche::PathEvent::Validated(local_addr, peer_addr) = qe {
                    client_conn.migrate(local_addr, peer_addr).unwrap();
                }
            }

            // Sleep a bit to avoid busy-waiting
            s2n_quic::provider::io::testing::time::delay(std::time::Duration::from_millis(10))
                .await;
        }
    });

    Ok(())
}
