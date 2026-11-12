// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    event,
    path::secret::{stateless_reset::Signer, Map},
    psk::{client::Provider as ClientProvider, server::Provider as ServerProvider},
    stream::{
        client::tokio::Client as ClientTokio,
        server::{application, manager},
        Protocol,
    },
    testing::{init_tracing, server_name, NoopSubscriber, TestTlsProvider},
};
use s2n_quic_core::time::StdClock;
use std::{
    net::SocketAddr,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

type Client = ClientTokio<ClientProvider, NoopSubscriber>;

#[derive(Clone, Default)]
struct Counts {
    handed_off: Arc<AtomicUsize>,
    forwarded: Arc<AtomicUsize>,
    io_errors: Arc<AtomicUsize>,
}

impl event::Subscriber for Counts {
    type ConnectionContext = ();

    fn create_connection_context(
        &self,
        _meta: &event::api::ConnectionMeta,
        _info: &event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }

    fn on_acceptor_udp_stream_handed_off(
        &self,
        _meta: &event::api::EndpointMeta,
        _event: &event::api::AcceptorUdpStreamHandedOff,
    ) {
        self.handed_off.fetch_add(1, Ordering::Relaxed);
    }

    fn on_acceptor_udp_packet_forwarded(
        &self,
        _meta: &event::api::EndpointMeta,
        _event: &event::api::AcceptorUdpPacketForwarded,
    ) {
        self.forwarded.fetch_add(1, Ordering::Relaxed);
    }

    fn on_acceptor_udp_io_error(
        &self,
        _meta: &event::api::EndpointMeta,
        _event: &event::api::AcceptorUdpIoError,
    ) {
        self.io_errors.fetch_add(1, Ordering::Relaxed);
    }
}

fn socket_path(name: &str) -> PathBuf {
    PathBuf::from(format!(
        "/tmp/dc-udp-manager-{}-{name}.sock",
        std::process::id()
    ))
}

fn client() -> Client {
    client_at("127.0.0.1:0")
}

fn client_at(address: &str) -> Client {
    let subscriber = NoopSubscriber {};
    let map = Map::new(
        Signer::new(b"default"),
        100,
        false,
        StdClock::default(),
        subscriber.clone(),
    );
    let handshake = ClientProvider::builder()
        .start(
            address.parse().unwrap(),
            map,
            TestTlsProvider {},
            subscriber.clone(),
            server_name(),
        )
        .unwrap();
    Client::builder()
        .with_default_protocol(Protocol::Udp)
        .build(handshake, subscriber)
        .unwrap()
}

async fn handshake_server() -> ServerProvider {
    handshake_server_at("127.0.0.1:0").await
}

async fn handshake_server_at(address: &str) -> ServerProvider {
    let subscriber = NoopSubscriber {};
    let map = Map::new(
        Signer::new(b"default"),
        100,
        false,
        StdClock::default(),
        subscriber.clone(),
    );
    ServerProvider::builder()
        .start(
            address.parse().unwrap(),
            TestTlsProvider {},
            subscriber,
            map,
        )
        .await
        .unwrap()
}

fn application(path: &Path) -> application::Server<NoopSubscriber> {
    application_at(path, "127.0.0.1:0")
}

fn application_at(path: &Path, address: &str) -> application::Server<NoopSubscriber> {
    application::Server::<NoopSubscriber>::builder()
        .with_protocol(Protocol::Udp)
        .with_udp_address(address.parse().unwrap())
        .with_socket_path(path)
        .build(NoopSubscriber {})
        .unwrap()
}

fn manager(
    handshake: &ServerProvider,
    path: &Path,
    counts: &Counts,
) -> manager::Server<ServerProvider, Counts> {
    manager::Server::<ServerProvider, Counts>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_udp(true)
        .with_workers(NonZeroUsize::new(1).unwrap())
        .with_socket_path(path)
        .build(handshake.clone(), counts.clone())
        .unwrap()
}

struct Setup {
    client: Client,
    handshake: ServerProvider,
}

impl Setup {
    async fn new() -> Self {
        init_tracing();
        let client = client();
        let handshake = handshake_server().await;
        client
            .handshake_with(handshake.local_addr(), server_name())
            .await
            .unwrap();
        Self { client, handshake }
    }

    async fn exchange(
        &self,
        manager_addr: SocketAddr,
        app: &application::Server<NoopSubscriber>,
        request: &[u8],
        response: &[u8],
    ) -> SocketAddr {
        let mut client_stream = self
            .client
            .connect(self.handshake.local_addr(), manager_addr, server_name())
            .await
            .unwrap();
        let mut request_slice = request;
        client_stream
            .write_all_from_fin(&mut request_slice)
            .await
            .unwrap();

        let (mut server_stream, _peer) = tokio::time::timeout(Duration::from_secs(5), app.accept())
            .await
            .unwrap()
            .unwrap();
        let mut received: Vec<u8> = vec![];
        while server_stream.read_into(&mut received).await.unwrap() > 0 {}
        assert_eq!(received, request);

        let mut response_slice = response;
        server_stream
            .write_all_from_fin(&mut response_slice)
            .await
            .unwrap();

        let mut received: Vec<u8> = vec![];
        while client_stream.read_into(&mut received).await.unwrap() > 0 {}
        assert_eq!(received, response);

        client_stream.peer_addr().unwrap()
    }
}

async fn assert_no_stream(app: &application::Server<NoopSubscriber>) {
    assert!(
        tokio::time::timeout(Duration::from_millis(200), app.accept())
            .await
            .is_err(),
        "the application accepted an unexpected stream"
    );
}

#[tokio::test]
async fn udp_handoff_client_follows_application() {
    let setup = Setup::new().await;
    let path = socket_path("follow");
    let app = application(&path);
    let counts = Counts::default();
    let manager = manager(&setup.handshake, &path, &counts);
    let manager_addr = manager.acceptor_addr().unwrap();
    assert_ne!(manager_addr.port(), 0);

    let peer = setup
        .exchange(manager_addr, &app, b"hello", b"from the application")
        .await;

    assert_eq!(peer.ip(), manager_addr.ip());
    assert_ne!(peer.port(), manager_addr.port());
    assert_eq!(counts.handed_off.load(Ordering::Relaxed), 1);
    assert_no_stream(&app).await;
}

#[tokio::test]
async fn udp_handoff_steady_state_bypasses_manager() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let setup = Setup::new().await;
        let path = socket_path("steady");
        let app = application(&path);
        let counts = Counts::default();
        let manager = manager(&setup.handshake, &path, &counts);
        let manager_addr = manager.acceptor_addr().unwrap();
        let mut client_stream = setup
            .client
            .connect(setup.handshake.local_addr(), manager_addr, server_name())
            .await
            .unwrap();
        client_stream
            .write_all_from(&mut &b"hello"[..])
            .await
            .unwrap();

        let (mut server_stream, _) = app.accept().await.unwrap();
        let mut received = Vec::<u8>::new();
        while received.len() < 5 {
            assert!(server_stream.read_into(&mut received).await.unwrap() > 0);
        }
        assert_eq!(received, b"hello");
        server_stream
            .write_all_from(&mut &b"ready"[..])
            .await
            .unwrap();
        let mut response = Vec::<u8>::new();
        while response.len() < 5 {
            assert!(client_stream.read_into(&mut response).await.unwrap() > 0);
        }
        assert_eq!(response, b"ready");
        let application_addr = client_stream.peer_addr().unwrap();
        assert_eq!(application_addr.ip(), manager_addr.ip());
        assert_ne!(application_addr.port(), manager_addr.port());

        // Drain packets sent before the client followed the reply port.
        tokio::time::sleep(Duration::from_millis(50)).await;
        let forwarded = counts.forwarded.load(Ordering::Relaxed);
        let request = vec![42u8; 4096];
        client_stream
            .write_all_from_fin(&mut &request[..])
            .await
            .unwrap();
        received.clear();
        while server_stream.read_into(&mut received).await.unwrap() > 0 {}
        assert_eq!(received, request);
        server_stream
            .write_all_from_fin(&mut &b"done"[..])
            .await
            .unwrap();
        response.clear();
        while client_stream.read_into(&mut response).await.unwrap() > 0 {}
        assert_eq!(response, b"done");
        assert_eq!(counts.forwarded.load(Ordering::Relaxed), forwarded);
        assert_eq!(counts.handed_off.load(Ordering::Relaxed), 1);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn udp_handoff_ipv6_client_follows_application() {
    let client = client_at("[::1]:0");
    let handshake = handshake_server_at("[::1]:0").await;
    client
        .handshake_with(handshake.local_addr(), server_name())
        .await
        .unwrap();
    let setup = Setup { client, handshake };
    let path = socket_path("ipv6");
    let app = application_at(&path, "[::1]:0");
    let counts = Counts::default();
    let manager = manager::Server::<ServerProvider, Counts>::builder()
        .with_address("[::1]:0".parse().unwrap())
        .with_protocol(Protocol::Udp)
        .with_workers(NonZeroUsize::new(1).unwrap())
        .with_socket_path(&path)
        .build(setup.handshake.clone(), counts.clone())
        .unwrap();
    let manager_addr = manager.acceptor_addr().unwrap();
    let peer = setup.exchange(manager_addr, &app, b"ipv6", b"reply").await;
    assert_eq!(peer.ip(), manager_addr.ip());
    assert_ne!(peer.port(), manager_addr.port());
    assert_eq!(counts.handed_off.load(Ordering::Relaxed), 1);
    assert_no_stream(&app).await;
}

#[tokio::test]
async fn udp_handoff_forwards_duplicates_and_early_data() {
    let setup = Setup::new().await;
    let path = socket_path("forward");
    let app = application(&path);
    let counts = Counts::default();
    let manager = manager(&setup.handshake, &path, &counts);
    let manager_addr = manager.acceptor_addr().unwrap();

    let mut client_stream = setup
        .client
        .connect(setup.handshake.local_addr(), manager_addr, server_name())
        .await
        .unwrap();
    let request = vec![42u8; 200_000];
    let write = tokio::spawn({
        let request = request.clone();
        async move {
            let mut request_slice = &request[..];
            client_stream
                .write_all_from_fin(&mut request_slice)
                .await
                .unwrap();
            client_stream
        }
    });

    // Hold off accepting so the client keeps retransmitting to the manager.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(counts.forwarded.load(Ordering::Relaxed) > 0);

    let (mut server_stream, _peer) = tokio::time::timeout(Duration::from_secs(5), app.accept())
        .await
        .unwrap()
        .unwrap();
    let mut received: Vec<u8> = vec![];
    while server_stream.read_into(&mut received).await.unwrap() > 0 {}
    assert_eq!(received, request);

    let mut response = &b"done"[..];
    server_stream
        .write_all_from_fin(&mut response)
        .await
        .unwrap();
    let mut client_stream = write.await.unwrap();
    let mut received: Vec<u8> = vec![];
    while client_stream.read_into(&mut received).await.unwrap() > 0 {}
    assert_eq!(received, b"done");

    assert_ne!(
        client_stream.peer_addr().unwrap().port(),
        manager_addr.port()
    );
    assert_eq!(counts.handed_off.load(Ordering::Relaxed), 1);
    assert_no_stream(&app).await;
}

#[tokio::test]
async fn udp_handoff_routes_by_port() {
    let setup = Setup::new().await;
    let path_a = socket_path("route-a");
    let path_b = socket_path("route-b");
    let app_a = application(&path_a);
    let app_b = application(&path_b);
    let counts = Counts::default();
    let manager_a = manager(&setup.handshake, &path_a, &counts);
    let manager_b = manager(&setup.handshake, &path_b, &counts);
    let addr_a = manager_a.acceptor_addr().unwrap();
    let addr_b = manager_b.acceptor_addr().unwrap();
    assert_ne!(addr_a, addr_b);

    for _ in 0..3 {
        setup.exchange(addr_a, &app_a, b"to a", b"from a").await;
        setup.exchange(addr_b, &app_b, b"to b", b"from b").await;
    }

    assert_no_stream(&app_a).await;
    assert_no_stream(&app_b).await;
}

#[tokio::test]
async fn udp_handoff_survives_application_crash() {
    let setup = Setup::new().await;
    let path = socket_path("crash");
    let app = application(&path);
    let counts = Counts::default();
    let manager = manager(&setup.handshake, &path, &counts);
    let manager_addr = manager.acceptor_addr().unwrap();

    setup
        .exchange(manager_addr, &app, b"before", b"crash")
        .await;

    drop(app);

    let mut client_stream = setup
        .client
        .connect(setup.handshake.local_addr(), manager_addr, server_name())
        .await
        .unwrap();
    let mut request = &b"nobody is listening"[..];
    client_stream
        .write_all_from_fin(&mut request)
        .await
        .unwrap();
    let mut buffer: Vec<u8> = vec![];
    assert!(
        tokio::time::timeout(
            Duration::from_millis(500),
            client_stream.read_into(&mut buffer)
        )
        .await
        .is_err(),
        "the manager must not answer for a crashed application"
    );
    assert!(counts.io_errors.load(Ordering::Relaxed) > 0);

    let app = application(&path);
    setup
        .exchange(manager_addr, &app, b"after", b"restart")
        .await;
}

#[tokio::test]
async fn udp_only_manager_listens() {
    let setup = Setup::new().await;
    let path = socket_path("udp-only");
    let _app = application(&path);
    let manager = manager::Server::<ServerProvider, Counts>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_protocol(Protocol::Udp)
        .with_socket_path(&path)
        .build(setup.handshake.clone(), Counts::default())
        .unwrap();
    let addr = manager.acceptor_addr().unwrap();
    assert_ne!(addr.port(), 0);

    let err = std::net::UdpSocket::bind(addr).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
}

#[tokio::test]
async fn no_transport_is_rejected() {
    let setup = Setup::new().await;
    let path = socket_path("no-transport");
    let expected = "at least one acceptor type needs to be enabled";

    let err = manager::Server::<ServerProvider, Counts>::builder()
        .with_address("127.0.0.1:0".parse().unwrap())
        .with_socket_path(&path)
        .build(setup.handshake.clone(), Counts::default())
        .err()
        .unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(err.to_string(), expected);

    let err = application::Server::<NoopSubscriber>::builder()
        .with_socket_path(&path)
        .build(NoopSubscriber {})
        .err()
        .unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(err.to_string(), expected);
}
