// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// use crate::{handshake, stream::Protocol, testing::init_tracing};
use crate::{
    path::secret::{stateless_reset::Signer, Map},
    psk::{client::Provider as ClientProvider, server::Provider as ServerProvider},
    stream::{
        client::tokio::Client as ClientTokio, server::manager::Server as ServerTokio, Protocol,
    },
    testing::{init_tracing, query_event, server_name, NoopSubscriber, TestTlsProvider},
};
use s2n_quic_core::time::StdClock;
use std::{
    num::NonZeroUsize,
    path::{Path, PathBuf},
    time::Duration,
};
use tracing::info;

#[tokio::test]
async fn setup_servers() {
    init_tracing();

    let tls_materials_provider = TestTlsProvider {};
    let test_event_subscriber = NoopSubscriber {};
    let unix_socket_path1 = PathBuf::from("/tmp/shared1.sock");
    let unix_socket_path2 = PathBuf::from("/tmp/shared2.sock");

    // Create client
    let handshake_client = ClientProvider::builder()
        .start(
            "127.0.0.1:0".parse().unwrap(),
            Map::new(
                Signer::new(b"default"),
                100,
                StdClock::default(),
                test_event_subscriber.clone(),
            ),
            tls_materials_provider.clone(),
            test_event_subscriber.clone(),
            query_event,
            server_name(),
        )
        .unwrap();

    info!("Handshake client: {:?}", handshake_client.local_addr());

    let stream_client = ClientTokio::<ClientProvider, NoopSubscriber>::builder()
        .with_tcp(true)
        .with_default_protocol(Protocol::Tcp)
        .build(handshake_client, test_event_subscriber.clone())
        .unwrap();

    info!("Client created");

    // Create manager handshake server
    let manager_handshake_map = Map::new(
        Signer::new(b"default"),
        1,
        StdClock::default(),
        test_event_subscriber.clone(),
    );

    let handshake_server = ServerProvider::builder()
        .start(
            "127.0.0.1:0".parse().unwrap(),
            tls_materials_provider.clone(),
            test_event_subscriber.clone(),
            manager_handshake_map,
        )
        .await
        .unwrap();

    info!(
        "Manager handshake server: {}",
        handshake_server.local_addr()
    );

    let handshake_addr = handshake_server.local_addr();
    stream_client
        .handshake_with(handshake_addr, server_name())
        .await
        .unwrap();
    info!("Handshake completed");

    info!("Setting up first manager+application servers");
    test_connection(
        &unix_socket_path1,
        &handshake_server,
        test_event_subscriber.clone(),
        &stream_client,
    )
    .await;

    info!("Setting up second manager+application servers");
    test_connection(
        &unix_socket_path2,
        &handshake_server,
        test_event_subscriber,
        &stream_client,
    )
    .await;
}

async fn test_connection(
    unix_socket_path: &Path,
    handshake_server: &ServerProvider,
    test_event_subscriber: NoopSubscriber,
    stream_client: &ClientTokio<ClientProvider, NoopSubscriber>,
) {
    let manager_server = ServerTokio::<ServerProvider, NoopSubscriber>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_workers(NonZeroUsize::new(1).unwrap())
        .with_socket_path(unix_socket_path)
        .build(handshake_server.clone(), test_event_subscriber.clone())
        .unwrap();

    info!(
        "Manager server created at: {:?}",
        manager_server.acceptor_addr()
    );

    let app_server = crate::stream::server::application::Server::<NoopSubscriber>::builder()
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_socket_path(unix_socket_path)
        .build(test_event_subscriber.clone())
        .unwrap();
    info!("Application server created");

    info!("All servers setup completed successfully");

    let handshake_addr = handshake_server.local_addr();
    let acceptor_addr = manager_server.acceptor_addr().unwrap();
    let connection_result = tokio::try_join!(
        stream_client.connect(handshake_addr, acceptor_addr, server_name()),
        async {
            tokio::time::timeout(Duration::from_secs(5), app_server.accept())
                .await
                .unwrap()
        }
    );

    assert!(
        connection_result.is_ok(),
        "Connection should be established successfully"
    );
    let (mut client_stream, server_result) = connection_result.unwrap();
    let (mut server_stream, _addr) = server_result;

    info!("Connection established successfully between client and application server");

    let test_message = b"Hello from server!";

    let data_exchange_result = tokio::try_join!(
        async {
            let mut buffer = Vec::new();
            let bytes_read = client_stream.read_into(&mut buffer).await?;
            info!(
                "Client received {} bytes: {}",
                bytes_read,
                String::from_utf8_lossy(&buffer[..bytes_read])
            );
            assert_eq!(
                &buffer[..bytes_read],
                test_message,
                "Client should receive the correct message"
            );
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        },
        async {
            let mut message_slice = &test_message[..];
            let bytes_written = server_stream.write_from(&mut message_slice).await?;
            info!(
                "Server sent {} bytes: {}",
                bytes_written,
                String::from_utf8_lossy(test_message)
            );
            Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
        }
    );

    assert!(
        data_exchange_result.is_ok(),
        "Data exchange should be successful"
    );
    info!("Data exchange completed successfully");
}
