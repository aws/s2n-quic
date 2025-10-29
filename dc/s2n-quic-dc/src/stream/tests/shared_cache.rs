// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// use crate::{handshake, stream::Protocol, testing::init_tracing};
use crate::{
    path::secret::{stateless_reset::Signer, Map},
    psk::{client::Provider as ClientProvider, server::Provider as ServerProvider},
    stream::{
        client::tokio::Client as ClientTokio,
        server::{application, manager},
        Protocol,
    },
    testing::{init_tracing, query_event, server_name, NoopSubscriber, TestTlsProvider},
};
use s2n_quic_core::time::StdClock;
use std::{
    num::{NonZero, NonZeroUsize},
    path::{Path, PathBuf},
    time::Duration,
};
use tracing::info;

fn create_stream_client() -> ClientTokio<ClientProvider, NoopSubscriber> {
    let tls_materials_provider = TestTlsProvider {};
    let test_event_subscriber = NoopSubscriber {};

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
    stream_client
}

async fn create_handshake_server() -> ServerProvider {
    let tls_materials_provider = TestTlsProvider {};
    let test_event_subscriber = NoopSubscriber {};

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
    handshake_server
}

fn create_application_server(
    unix_socket_path: &Path,
    test_event_subscriber: NoopSubscriber,
) -> application::Server<NoopSubscriber> {
    let app_server = application::Server::<NoopSubscriber>::builder()
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_socket_path(unix_socket_path)
        .build(test_event_subscriber.clone())
        .unwrap();
    info!("Application server created");
    app_server
}

#[tokio::test]
async fn setup_servers() {
    init_tracing();

    let test_event_subscriber = NoopSubscriber {};
    let unix_socket_path1 = PathBuf::from("/tmp/shared1.sock");
    let unix_socket_path2 = PathBuf::from("/tmp/shared2.sock");

    let stream_client = create_stream_client();
    let handshake_server = create_handshake_server().await;

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
    let manager_server = manager::Server::<ServerProvider, NoopSubscriber>::builder()
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

    let app_server = create_application_server(unix_socket_path, test_event_subscriber);

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

#[tokio::test]
async fn test_kernel_queue_full() {
    init_tracing();
    let test_event_subscriber = NoopSubscriber {};
    let unix_socket_path = PathBuf::from("/tmp/kernel_queue_test.sock");

    let stream_client = create_stream_client();
    let handshake_server = create_handshake_server().await;

    let handshake_addr = handshake_server.local_addr();
    stream_client
        .handshake_with(handshake_addr, server_name())
        .await
        .unwrap();
    info!("Handshake completed");

    let manager_server = manager::Server::<ServerProvider, NoopSubscriber>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_workers(NonZeroUsize::new(1).unwrap())
        .with_socket_path(&unix_socket_path)
        .with_backlog(NonZero::new(10000).unwrap()) // configuring backlog so that streams are not dropped
        .build(handshake_server.clone(), test_event_subscriber.clone())
        .unwrap();

    info!(
        "Manager server created at: {:?}",
        manager_server.acceptor_addr()
    );
    let acceptor_addr = manager_server.acceptor_addr().unwrap();

    let app_server = create_application_server(&unix_socket_path, test_event_subscriber);

    let mut clients = Vec::new();
    let stream_count = 10000;
    let mut buffer: Vec<u8> = Vec::new();

    for _ in 0..stream_count {
        let mut client_stream = stream_client
            .connect(handshake_addr, acceptor_addr, server_name())
            .await
            .unwrap();

        // read from stream times out
        let read_result = tokio::time::timeout(
            Duration::from_millis(2),
            client_stream.read_into(&mut buffer),
        )
        .await;
        assert!(read_result.is_err());
        assert!(matches!(
            read_result.unwrap_err(),
            tokio::time::error::Elapsed { .. }
        ));

        clients.push(client_stream);
    }

    let mut servers = Vec::new();
    for _ in 0..stream_count {
        let (stream, _addr) = app_server.accept().await.unwrap();
        servers.push(stream);
    }

    let test_message = b"Hello from server!";
    for mut stream in servers {
        let mut message_slice = &test_message[..];
        stream.write_from(&mut message_slice).await.unwrap();
    }

    for mut stream in clients {
        let mut buffer: Vec<u8> = Vec::new();
        let bytes_read = stream.read_into(&mut buffer).await.unwrap();
        assert_eq!(
            &buffer[..bytes_read],
            test_message,
            "Client should receive the correct message"
        );
    }
}

#[tokio::test]
async fn test_application_crash() {
    init_tracing();
    let test_event_subscriber = NoopSubscriber {};
    let unix_socket_path = PathBuf::from("/tmp/shared_crash.sock");

    let stream_client = create_stream_client();
    let handshake_server = create_handshake_server().await;

    let handshake_addr = handshake_server.local_addr();
    stream_client
        .handshake_with(handshake_addr, server_name())
        .await
        .unwrap();
    info!("Handshake completed");

    let manager_server = manager::Server::<ServerProvider, NoopSubscriber>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_workers(NonZeroUsize::new(1).unwrap())
        .with_socket_path(&unix_socket_path)
        .build(handshake_server.clone(), test_event_subscriber.clone())
        .unwrap();

    info!(
        "Manager server created at: {:?}",
        manager_server.acceptor_addr()
    );

    let app_server = create_application_server(&unix_socket_path, test_event_subscriber);

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

    // Application crash
    drop(server_result);
    drop(app_server);

    // Reading from existing stream fails
    let mut buffer: Vec<u8> = Vec::new();
    let read_result = client_stream.read_into(&mut buffer).await;
    assert!(read_result.is_err());
    let error = read_result.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    info!("Stream error: {}", error.to_string()); // TruncatedTransport

    // Create new stream
    info!("Trying to open a new stream to app server");
    let result = stream_client
        .connect(handshake_addr, acceptor_addr, server_name())
        .await;
    assert!(
        result.is_ok(),
        "Connection should be established successfully"
    );

    // Reading from new stream fails
    let mut client_stream = result.unwrap();
    let read_result = client_stream.read_into(&mut buffer).await;
    assert!(read_result.is_err());
    let error = read_result.unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    info!("Stream error: {}", error.to_string()); // TruncatedTransport
}

#[tokio::test]
async fn test_kernel_queue_full_application_crash() {
    init_tracing();
    let test_event_subscriber = NoopSubscriber {};
    let unix_socket_path = PathBuf::from("/tmp/kernel_queue_crash.sock");

    let stream_client = create_stream_client();
    let handshake_server = create_handshake_server().await;

    let handshake_addr = handshake_server.local_addr();
    stream_client
        .handshake_with(handshake_addr, server_name())
        .await
        .unwrap();
    info!("Handshake completed");

    let manager_server = manager::Server::<ServerProvider, NoopSubscriber>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_protocol(Protocol::Tcp)
        .with_udp(false)
        .with_workers(NonZeroUsize::new(1).unwrap())
        .with_socket_path(&unix_socket_path)
        .with_backlog(NonZero::new(5000).unwrap())
        .build(handshake_server.clone(), test_event_subscriber.clone())
        .unwrap();

    info!(
        "Manager server created at: {:?}",
        manager_server.acceptor_addr()
    );
    let acceptor_addr = manager_server.acceptor_addr().unwrap();

    let app_server = create_application_server(&unix_socket_path, test_event_subscriber);

    let mut clients = Vec::new();
    let stream_count = 5000;

    for _ in 0..stream_count {
        let mut client_stream = stream_client
            .connect(handshake_addr, acceptor_addr, server_name())
            .await
            .unwrap();

        let mut buffer: Vec<u8> = Vec::new();
        let read_result = tokio::time::timeout(
            Duration::from_millis(5),
            client_stream.read_into(&mut buffer),
        )
        .await;
        assert!(read_result.is_err());
        assert!(matches!(
            read_result.unwrap_err(),
            tokio::time::error::Elapsed { .. }
        ));
        clients.push(client_stream);
    }

    drop(app_server);

    for mut stream in clients {
        let mut buffer: Vec<u8> = Vec::new();
        let read_result = stream.read_into(&mut buffer).await;
        assert!(read_result.is_err());
        let error = read_result.unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::UnexpectedEof);
    }
}
