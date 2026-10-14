// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::{testing::dcquic::Context, Protocol};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn migrate(protocol: Protocol) {
    let source = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (context, mut client, mut server) = source.block_on(async {
        let context = Context::new(protocol).await;
        let (mut client, mut server) = context.pair().await;
        server.write_all(b"buffered").await.unwrap();
        let mut first = [0];
        client.read_exact(&mut first).await.unwrap();
        assert_eq!(first, [b'b']);
        // Register a read waker (and, for UDP, a receive timer) on the old runtime.
        let mut byte = [0];
        assert!(
            tokio::time::timeout(Duration::from_millis(20), server.read_exact(&mut byte))
                .await
                .is_err()
        );
        (context, client, server)
    });

    let client_addr = client.local_addr().unwrap();
    let server_addr = server.local_addr().unwrap();
    // An error must preserve the stream, including its partially consumed buffer.
    assert_eq!(
        client.rebind_to_current_runtime().unwrap_err().kind(),
        std::io::ErrorKind::Other
    );
    if protocol.is_tcp() {
        let stopped = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let handle = stopped.handle().clone();
        drop(stopped);
        let _guard = handle.enter();
        // Registration failure must also preserve buffered data and the original socket.
        assert!(client.rebind_to_current_runtime().is_err());
        assert_eq!(client.local_addr().unwrap(), client_addr);
    }
    let destination = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    {
        let _guard = destination.enter();
        for _ in 0..2 {
            client.rebind_to_current_runtime().unwrap();
            server.rebind_to_current_runtime().unwrap();
        }
    }
    // The old application runtime cannot deliver any readiness or timer events now.
    drop(source);
    destination.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            assert_eq!(client.local_addr().unwrap(), client_addr);
            assert_eq!(server.local_addr().unwrap(), server_addr);
            assert_eq!(client.protocol(), protocol);
            let mut buffered = [0; 7];
            client.read_exact(&mut buffered).await.unwrap();
            // The initial 'b' was consumed on the source runtime.
            assert_eq!(&buffered, &b"buffered"[1..]);

            let request = vec![0x42; 128 * 1024];
            let response = vec![0x24; 128 * 1024];
            tokio::join!(
                async {
                    client.write_all(&request).await.unwrap();
                    client.flush().await.unwrap();
                    let mut actual = vec![0; response.len()];
                    client.read_exact(&mut actual).await.unwrap();
                    assert_eq!(actual, response);
                    client.shutdown().await.unwrap();
                },
                async {
                    let mut actual = vec![0; request.len()];
                    server.read_exact(&mut actual).await.unwrap();
                    assert_eq!(actual, request);
                    server.write_all(&response).await.unwrap();
                    server.flush().await.unwrap();
                    let mut eof = [0];
                    assert_eq!(server.read(&mut eof).await.unwrap(), 0);
                    server.shutdown().await.unwrap();
                }
            );
        })
        .await
        .unwrap();
        // Receive shutdown can poll its timer while draining the stream.
        // Drop the streams while their destination runtime is still alive.
        drop(client);
        drop(server);
    });
    drop(context);
}

#[test]
fn rebind_tcp_to_current_runtime() {
    migrate(Protocol::Tcp);
}

#[test]
fn rebind_udp_to_current_runtime() {
    migrate(Protocol::Udp);
}

#[test]
fn rebind_unpooled_udp_socket_pair() {
    use crate::{
        msg::{addr::Addr, cmsg},
        stream::socket::application::builder::{Builder, UdpPair},
    };
    use s2n_quic_core::inet::ExplicitCongestionNotification;
    use std::{
        future::poll_fn,
        io::{IoSlice, IoSliceMut},
        sync::Arc,
    };

    let source = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let sockets = {
        let _guard = source.enter();
        let reader = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let writer = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        reader.set_nonblocking(true).unwrap();
        writer.set_nonblocking(true).unwrap();
        Box::new(UdpPair {
            reader: Arc::new(reader),
            writer: Arc::new(writer),
        })
        .build()
        .unwrap()
    };
    let read_addr = sockets.read_application().local_addr().unwrap();
    let write_addr = sockets.write_application().local_addr().unwrap();
    let destination = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let rebound = {
        let _guard = destination.enter();
        sockets.rebind_to_current_runtime().unwrap()
    };
    drop(sockets);
    drop(source);
    destination.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            let peer = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
            peer.send_to(b"request", read_addr).await.unwrap();
            let mut addr = Addr::default();
            let mut cmsg = cmsg::Receiver::default();
            let mut data = [0; 7];
            let mut buffers = [IoSliceMut::new(&mut data)];
            assert_eq!(
                poll_fn(|cx| rebound.read_application().poll_recv(
                    cx,
                    &mut addr,
                    &mut cmsg,
                    &mut buffers,
                ))
                .await
                .unwrap(),
                7
            );
            assert_eq!(&data, b"request");
            assert_eq!(
                poll_fn(|cx| rebound.write_application().poll_send(
                    cx,
                    &addr,
                    ExplicitCongestionNotification::NotEct,
                    &[IoSlice::new(b"response")],
                ))
                .await
                .unwrap(),
                8
            );
            let mut response = [0; 8];
            let (len, from) = peer.recv_from(&mut response).await.unwrap();
            assert_eq!(len, 8);
            assert_eq!(from, write_addr);
            assert_eq!(&response, b"response");
        })
        .await
        .unwrap();
    });
}
