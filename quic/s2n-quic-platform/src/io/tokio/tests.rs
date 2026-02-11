// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::*;
use core::{
    convert::TryInto,
    task::{Context, Poll},
};
use s2n_quic::{client::Connect, provider::io::tokio::Builder, Client, Server};
use s2n_quic_core::{
    crypto::tls::testing::certificates,
    endpoint::{self, CloseError},
    event,
    inet::ExplicitCongestionNotification,
    io::{rx, tx},
    path::{mtu, Handle as _},
    time::{Clock, Duration, Timestamp},
};
use std::{
    collections::BTreeMap,
    net::{SocketAddr, ToSocketAddrs},
    time::Duration as StdDuration,
};

struct TestEndpoint<const IS_SERVER: bool> {
    handle: PathHandle,
    messages: BTreeMap<u32, Option<Timestamp>>,
    now: Option<Timestamp>,
    subscriber: NoopSubscriber,
}

impl<const IS_SERVER: bool> TestEndpoint<IS_SERVER> {
    fn new(handle: PathHandle) -> Self {
        let messages = if IS_SERVER { 0 } else { 30 };
        let messages = (0..messages).map(|id| (id, None)).collect();
        Self {
            handle,
            messages,
            now: None,
            subscriber: Default::default(),
        }
    }
}

#[derive(Debug, Default)]
struct NoopSubscriber;

impl event::Subscriber for NoopSubscriber {
    type ConnectionContext = ();

    fn create_connection_context(
        &mut self,
        _meta: &event::api::ConnectionMeta,
        _info: &event::api::ConnectionInfo,
    ) -> Self::ConnectionContext {
    }
}

impl<const IS_SERVER: bool> Endpoint for TestEndpoint<IS_SERVER> {
    type PathHandle = PathHandle;
    type Subscriber = NoopSubscriber;

    const ENDPOINT_TYPE: endpoint::Type = if IS_SERVER {
        endpoint::Type::Server
    } else {
        endpoint::Type::Client
    };

    fn transmit<Tx: tx::Queue<Handle = PathHandle>, C: Clock>(
        &mut self,
        queue: &mut Tx,
        clock: &C,
    ) {
        let now = clock.get_time();
        self.now = Some(now);

        for (id, tx_time) in &mut self.messages {
            match tx_time {
                Some(time) if now.saturating_duration_since(*time) < Duration::from_millis(50) => {
                    continue
                }
                _ => {
                    let payload = id.to_be_bytes();
                    let addr = self.handle;
                    let ecn = ExplicitCongestionNotification::Ect0;
                    let msg = (addr, ecn, payload);
                    if queue.push(msg).is_ok() {
                        *tx_time = Some(now);
                    } else {
                        // no more capacity
                        return;
                    }
                }
            }
        }
    }

    fn receive<Rx: rx::Queue<Handle = PathHandle>, C: Clock>(&mut self, queue: &mut Rx, clock: &C) {
        let now = clock.get_time();
        self.now = Some(now);

        queue.for_each(|_header, payload| {
            // we should only be receiving u32 values
            if payload.len() != 4 {
                return;
            }

            let id = (&*payload).try_into().unwrap();
            let id = u32::from_be_bytes(id);

            if IS_SERVER {
                self.messages.insert(id, None);
            } else {
                self.messages.remove(&id);
            }
        });
    }

    fn poll_wakeups<C: Clock>(
        &mut self,
        _cx: &mut Context<'_>,
        clock: &C,
    ) -> Poll<Result<usize, CloseError>> {
        let now = clock.get_time();
        self.now = Some(now);

        if !IS_SERVER && self.messages.is_empty() {
            return Err(CloseError).into();
        }

        Poll::Pending
    }

    fn timeout(&self) -> Option<Timestamp> {
        self.now.map(|now| now + Duration::from_millis(50))
    }

    fn set_mtu_config(&mut self, _mtu_config: mtu::Config) {
        // noop
    }

    fn subscriber(&mut self) -> &mut Self::Subscriber {
        &mut self.subscriber
    }
}

async fn runtime<A: ToSocketAddrs>(
    receive_addr: A,
    send_addr: Option<A>,
) -> io::Result<(super::Io, SocketAddress)> {
    let mut io_builder = Io::builder();

    let rx_socket = syscall::bind_udp(receive_addr, false, false, false)?;
    rx_socket.set_nonblocking(true)?;
    let rx_socket: std::net::UdpSocket = rx_socket.into();
    let rx_addr = rx_socket.local_addr()?;

    io_builder = io_builder.with_rx_socket(rx_socket)?;

    if let Some(tx_addr) = send_addr {
        let tx_socket = syscall::bind_udp(tx_addr, false, false, false)?;
        tx_socket.set_nonblocking(true)?;
        let tx_socket: std::net::UdpSocket = tx_socket.into();
        io_builder = io_builder.with_tx_socket(tx_socket)?
    }

    let io = io_builder.build()?;

    let rx_addr = if rx_addr.is_ipv6() {
        ("::1", rx_addr.port())
    } else {
        ("127.0.0.1", rx_addr.port())
    }
    .to_socket_addrs()?
    .next()
    .unwrap();

    Ok((io, rx_addr.into()))
}

/// The tokio IO provider allows the application to configure different sockets for rx
/// and tx. This function will accept optional TX addresses to test this functionality.
async fn test<A: ToSocketAddrs>(
    server_rx_addr: A,
    server_tx_addr: Option<A>,
    client_rx_addr: A,
    client_tx_addr: Option<A>,
) -> io::Result<()> {
    let (server_io, server_addr) = runtime(server_rx_addr, server_tx_addr).await?;
    let (client_io, client_addr) = runtime(client_rx_addr, client_tx_addr).await?;

    let server_endpoint = {
        let mut handle = PathHandle::from_remote_address(client_addr.into());
        handle.local_address = server_addr.into();
        TestEndpoint::<true>::new(handle)
    };

    let client_endpoint = {
        let mut handle = PathHandle::from_remote_address(server_addr.into());
        handle.local_address = client_addr.into();
        TestEndpoint::<false>::new(handle)
    };

    let (server_task, actual_server_addr) = server_io.start(server_endpoint)?;
    assert_eq!(actual_server_addr, server_addr);

    let (client_task, actual_client_addr) = client_io.start(client_endpoint)?;
    assert_eq!(actual_client_addr, client_addr);

    tokio::time::timeout(core::time::Duration::from_secs(60), client_task).await??;

    server_task.abort();

    Ok(())
}

static IPV4_LOCALHOST: &str = "127.0.0.1:0";
static IPV6_LOCALHOST: &str = "[::1]:0";

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn ipv4_test() -> io::Result<()> {
    test(IPV4_LOCALHOST, None, IPV4_LOCALHOST, None).await
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn ipv4_two_socket_test() -> io::Result<()> {
    test(
        IPV4_LOCALHOST,
        Some(IPV4_LOCALHOST),
        IPV4_LOCALHOST,
        Some(IPV4_LOCALHOST),
    )
    .await
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn ipv6_test() -> io::Result<()> {
    let result = test(IPV6_LOCALHOST, None, IPV6_LOCALHOST, None).await;

    match result {
        Err(err) if err.kind() == io::ErrorKind::AddrNotAvailable => {
            eprintln!("The current environment does not support IPv6; skipping");
            Ok(())
        }
        other => other,
    }
}

#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn ipv6_two_socket_test() -> io::Result<()> {
    let result = test(
        IPV6_LOCALHOST,
        Some(IPV6_LOCALHOST),
        IPV6_LOCALHOST,
        Some(IPV6_LOCALHOST),
    )
    .await;

    match result {
        Err(err) if err.kind() == io::ErrorKind::AddrNotAvailable => {
            eprintln!("The current environment does not support IPv6; skipping");
            Ok(())
        }
        other => other,
    }
}

#[cfg(unix)]
#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn only_v6_test() -> io::Result<()> {
    // Socket always set only_v6 to true if it binds
    // to a specific IPV6 address. We use ANY address
    // to test for only_v6.
    static IPV6_ANY_ADDRESS: &str = "[::]:0";

    let mut only_v6 = false;
    let socket = syscall::bind_udp(IPV6_ANY_ADDRESS, false, false, only_v6)?;
    assert!(!socket.only_v6()?);

    only_v6 = true;
    let socket = syscall::bind_udp(IPV6_ANY_ADDRESS, false, false, only_v6)?;
    assert!(socket.only_v6()?);

    Ok(())
}

// Tests that the ROUTER cBPF filter correctly routes packets to the appropriate socket.
#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn router_cbpf_packet_filtering_test() -> io::Result<()> {
    use std::time::Duration as StdDuration;
    use tokio::net::UdpSocket;

    // Create two rx sockets bound to same port with SO_REUSEPORT
    let rx_socket_0 = syscall::bind_udp(IPV4_LOCALHOST, false, true, false)?;
    rx_socket_0.set_nonblocking(true)?;
    let port = rx_socket_0.local_addr()?.as_socket().unwrap().port();

    let rx_socket_1 = syscall::bind_udp(("127.0.0.1", port), false, true, false)?;
    rx_socket_1.set_nonblocking(true)?;

    // Attach ROUTER to both sockets
    ROUTER.attach(&rx_socket_0)?;
    ROUTER.attach(&rx_socket_1)?;

    // Convert to tokio sockets for async recv
    let rx_socket_0 = UdpSocket::from_std(rx_socket_0.into())?;
    let rx_socket_1 = UdpSocket::from_std(rx_socket_1.into())?;

    // Create sender socket
    let sender = UdpSocket::bind("127.0.0.1:0").await?;
    let target_addr: std::net::SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

    // Test packet A: Initial packet with DCID length = 8
    // Should route to socket 0
    // Format: [header byte, version (4 bytes), dcid_len, ...]
    let packet_a = {
        let mut p = vec![0u8; 32];
        p[0] = 0xC0; // Initial packet (first 4 bits = 1100)
        p[1..5].copy_from_slice(&[0x00, 0x00, 0x00, 0x01]); // version
        p[5] = 0x08; // DCID length = 8
        p
    };

    // Test packet B: Handshake packet
    // Should route to socket 1
    let packet_b = {
        let mut p = vec![0u8; 32];
        p[0] = 0xE0; // Handshake packet first four bits are 1110
        p
    };

    // Test packet C: Initial packet but DCID length != 8
    // Should route to socket 1
    let packet_c = {
        let mut p = vec![0u8; 32];
        p[0] = 0xC0; // Initial packet (first 4 bits = 1100)
        p[1..5].copy_from_slice(&[0x00, 0x00, 0x00, 0x01]); // version
        p[5] = 0x10; // DCID length = 16
        p
    };

    // Send packets
    sender.send_to(&packet_a, target_addr).await?;
    sender.send_to(&packet_b, target_addr).await?;
    sender.send_to(&packet_c, target_addr).await?;

    // Give the kernel time to route packets
    tokio::time::sleep(StdDuration::from_millis(100)).await;

    // Receive and verify routing
    let mut buf = [0u8; 1024];

    // Socket 0 should receive packet_a (Initial with DCID len = 8)
    let recv_result = tokio::time::timeout(
        StdDuration::from_millis(500),
        rx_socket_0.recv_from(&mut buf),
    )
    .await;
    assert!(
        recv_result.is_ok(),
        "Socket 0 should receive packet_a (Initial with DCID len=8)"
    );
    let (len, _) = recv_result.unwrap()?;
    assert_eq!(buf[0], 0xC0, "Socket 0 should receive Initial packet");
    assert_eq!(
        buf[5], 0x08,
        "Socket 0 should receive packet with DCID len=8"
    );
    assert_eq!(len, 32);

    // Socket 1 should receive packet_b and packet_c
    let recv_result = tokio::time::timeout(
        StdDuration::from_millis(500),
        rx_socket_1.recv_from(&mut buf),
    )
    .await;
    assert!(
        recv_result.is_ok(),
        "Socket 1 should receive packet_b or packet_c"
    );

    let recv_result = tokio::time::timeout(
        StdDuration::from_millis(500),
        rx_socket_1.recv_from(&mut buf),
    )
    .await;
    assert!(
        recv_result.is_ok(),
        "Socket 1 should receive packet_b or packet_c"
    );

    // Socket 0 should not have any more packets
    let recv_result = tokio::time::timeout(
        StdDuration::from_millis(100),
        rx_socket_0.recv_from(&mut buf),
    )
    .await;
    assert!(
        recv_result.is_err(),
        "Socket 0 should not receive any more packets"
    );

    Ok(())
}

/// Creates two UDP sockets bound to the same IP address and port with SO_REUSEPORT.
/// ROUTER will be automatically attached when these are passed to with_rx_socket().
fn create_reuseport_sockets() -> io::Result<(std::net::UdpSocket, std::net::UdpSocket, u16)> {
    let socket_0 = syscall::bind_udp("127.0.0.1:0", false, true, false)?;
    socket_0.set_nonblocking(true)?;
    let port = socket_0.local_addr()?.as_socket().unwrap().port();

    let socket_1 = syscall::bind_udp(("127.0.0.1", port), false, true, false)?;
    socket_1.set_nonblocking(true)?;

    // Convert to std sockets
    let socket_0: std::net::UdpSocket = socket_0.into();
    let socket_1: std::net::UdpSocket = socket_1.into();

    Ok((socket_0, socket_1, port))
}

// Tests the full s2n-quic Server and Client over multiple sockets with SO_REUSEPORT.
#[tokio::test]
#[cfg_attr(miri, ignore)]
async fn router_multi_socket_server_client_test() {
    // Create two sockets on same port with SO_REUSEPORT
    // ROUTER will be automatically attached by Io::start() when multiple sockets are provided
    let (rx_socket_0, rx_socket_1, port) = create_reuseport_sockets().unwrap();
    let server_addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();

    let server_io = Builder::default()
        .with_rx_socket(rx_socket_0)
        .unwrap()
        .with_rx_socket(rx_socket_1)
        .unwrap()
        .build()
        .unwrap();

    let mut server = Server::builder()
        .with_io(server_io)
        .unwrap()
        .with_tls((certificates::CERT_PEM, certificates::KEY_PEM))
        .unwrap()
        .start()
        .unwrap();

    let actual_server_addr = server.local_addr().unwrap();
    assert_eq!(actual_server_addr, server_addr);

    let server_handle = tokio::spawn(async move {
        if let Some(mut connection) = server.accept().await {
            // Accept a stream and echo back
            if let Ok(Some(mut stream)) = connection.accept_bidirectional_stream().await {
                while let Ok(Some(data)) = stream.receive().await {
                    let _ = stream.send(data).await;
                }
            }
        }
    });

    let client_io = Builder::default()
        .with_receive_address("127.0.0.1:0".parse().unwrap())
        .unwrap()
        .build()
        .unwrap();

    let client = Client::builder()
        .with_io(client_io)
        .unwrap()
        .with_tls(certificates::CERT_PEM)
        .unwrap()
        .start()
        .unwrap();

    let connect = Connect::new(server_addr).with_server_name("localhost");
    let mut connection = client.connect(connect).await.unwrap();

    let mut stream = connection.open_bidirectional_stream().await.unwrap();

    let test_data = b"Hello from ROUTER test!";
    stream
        .send(bytes::Bytes::from_static(test_data))
        .await
        .unwrap();

    let received = stream.receive().await.unwrap();
    assert!(received.is_some());
    assert_eq!(&received.unwrap()[..], test_data);

    stream.finish().unwrap();
    connection.close(0u32.into());

    // Give server time to close
    tokio::time::sleep(StdDuration::from_millis(100)).await;
    server_handle.abort();
}
