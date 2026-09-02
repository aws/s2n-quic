// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use bach::time::timeout;
use s2n_quic::{stream::BidirectionalStream, Connection};

/// How long to wait for the endpoint close to propagate to the streams before asserting
/// on their behavior.
const CLOSE_PROPAGATION_DELAY: Duration = Duration::from_millis(5);

/// How long an rx operation is given to return before it's considered hanging.
const RECEIVE_TIMEOUT: Duration = Duration::from_millis(5);

/// Verifies client endpoint-drop tx stream behavior.
///
/// After endpoint drop, tx stream send should complete with an error.
#[test]
fn client_tx_fails_after_endpoint_drop() {
    let model = Model::default();
    test(model.clone(), |handle| {
        let server = build_server(handle, model.clone())?;
        let server_addr = start_server(server)?;

        let h = handle.clone();

        let client = build_client(handle, model, true).unwrap();

        primary::spawn(async move {
            let (_connection, mut stream) = connect_and_echo(client, server_addr).await;

            // Drop endpoint
            h.close_buffers();

            delay(CLOSE_PROPAGATION_DELAY).await;

            let send_res = stream.send(Bytes::from("world")).await;
            assert!(send_res.is_err());
        });

        Ok(())
    })
    .unwrap();
}

/// Verifies client endpoint-drop rx stream behavior.
///
/// After endpoint drop, rx stream accept should complete with an error instead of hanging.
#[test]
fn client_rx_fails_after_endpoint_drop() {
    let model = Model::default();
    test(model.clone(), |handle| {
        let server = build_server(handle, model.clone())?;
        let server_addr = start_server(server)?;

        let h = handle.clone();

        let client = build_client(handle, model, true).unwrap();

        primary::spawn(async move {
            let (_connection, mut stream) = connect_and_echo(client, server_addr).await;

            // Drop endpoint
            h.close_buffers();

            // The async call should return an error
            let res = timeout(RECEIVE_TIMEOUT, async move { stream.receive().await })
                .await
                .unwrap();
            assert!(res.is_err());
        });

        Ok(())
    })
    .unwrap();
}

/// Verifies server endpoint-drop tx stream behavior.
///
/// After server endpoint drop, tx stream send should complete with an error.
#[test]
fn server_tx_fails_after_endpoint_drop() {
    let model = Model::default();
    test(model.clone(), |handle| {
        let mut server = build_server(handle, model.clone())?;
        let server_addr = server.local_addr()?;

        let h = handle.clone();

        let client = build_client(handle, model, true).unwrap();

        run_echo_client(client, server_addr);

        primary::spawn(async move {
            let mut connection = server.accept().await.unwrap();

            let mut stream = connection
                .accept_bidirectional_stream()
                .await
                .unwrap()
                .unwrap();

            let data = stream.receive().await.unwrap().unwrap();
            stream.send(Bytes::from("hello")).await.unwrap();

            // Drop endpoint
            h.close_buffers();

            delay(CLOSE_PROPAGATION_DELAY).await;

            let send_res = stream.send(data).await;
            assert!(send_res.is_err());
        });

        Ok(())
    })
    .unwrap();
}

/// Verifies server endpoint-drop rx stream behavior.
///
/// After server endpoint drop, rx stream receive should complete with an error instead of hanging.
#[test]
fn server_rx_fails_after_endpoint_drop() {
    let model = Model::default();
    test(model.clone(), |handle| {
        let mut server = build_server(handle, model.clone())?;
        let server_addr = server.local_addr()?;

        let h = handle.clone();

        let client = build_client(handle, model, true).unwrap();

        run_echo_client(client, server_addr);

        primary::spawn(async move {
            let mut connection = server.accept().await.unwrap();

            let mut stream = connection
                .accept_bidirectional_stream()
                .await
                .unwrap()
                .unwrap();

            let _data = stream.receive().await.unwrap().unwrap();
            stream.send(Bytes::from("hello")).await.unwrap();

            // Drop endpoint
            h.close_buffers();

            // The async call should return an error
            let res = timeout(RECEIVE_TIMEOUT, async move { stream.receive().await })
                .await
                .unwrap();
            assert!(res.is_err());
        });

        Ok(())
    })
    .unwrap();
}

/// Connects to `server_addr` and performs an echo round trip on a newly opened
/// bidirectional stream.
///
/// The connection is returned along with the stream, since dropping the last connection
/// handle would close the connection and the streams that belong to it.
async fn connect_and_echo(
    client: Client,
    server_addr: SocketAddr,
) -> (Connection, BidirectionalStream) {
    let connect = Connect::new(server_addr).with_server_name("localhost");
    let mut connection = client.connect(connect).await.unwrap();

    let mut stream = connection.open_bidirectional_stream().await.unwrap();

    let sent = Bytes::from("hello");
    stream.send(sent.clone()).await.unwrap();
    let received = stream.receive().await.unwrap().unwrap();
    assert_eq!(sent, received);

    (connection, stream)
}

fn run_echo_client(client: Client, server_addr: SocketAddr) {
    spawn(async move {
        let connect = Connect::new(server_addr).with_server_name("localhost");
        let Ok(mut client_connection) = client.connect(connect).await else {
            return;
        };

        let Ok(mut stream) = client_connection.open_bidirectional_stream().await else {
            return;
        };

        let sent = Bytes::from("hello");
        let Ok(()) = stream.send(sent.clone()).await else {
            return;
        };
        _ = stream.receive().await;

        // Prevent dropping the connection and the stream
        delay(Duration::from_secs(60)).await;
    });
}
