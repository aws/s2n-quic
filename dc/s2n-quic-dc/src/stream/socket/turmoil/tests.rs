// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Example tests demonstrating turmoil integration with s2n-quic-dc streams.
//!
//! Run with: `cargo test --features unstable-provider-io-turmoil turmoil`

use super::super::{Protocol, Socket, TransportFeatures};
use std::net::SocketAddr;
use turmoil::{Builder, Result};

/// Basic UDP echo test using turmoil simulation
#[test]
fn udp_echo() -> Result {
    let mut sim = Builder::new()
        .simulation_duration(core::time::Duration::from_secs(10))
        .build();

    sim.host("server", || async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:9000").await?;

        // Verify Socket trait implementation
        assert_eq!(socket.protocol(), Protocol::Udp);
        assert_eq!(socket.features(), TransportFeatures::UDP);

        let mut buf = [0u8; 1024];
        let (len, peer) = socket.recv_from(&mut buf).await?;
        socket.send_to(&buf[..len], peer).await?;

        Ok(())
    });

    sim.client("client", async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:0").await?;
        let server_addr: SocketAddr = (turmoil::lookup("server"), 9000).into();

        let msg = b"hello turmoil";
        socket.send_to(msg, server_addr).await?;

        let mut buf = [0u8; 1024];
        let (len, _) = socket.recv_from(&mut buf).await?;

        assert_eq!(&buf[..len], msg);
        Ok(())
    });

    sim.run()
}

/// Test network partition and repair
#[test]
fn network_partition() -> Result {
    let mut sim = Builder::new()
        .simulation_duration(core::time::Duration::from_secs(10))
        .build();

    sim.host("server", || async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:9000").await?;
        let mut buf = [0u8; 1024];

        // This will eventually succeed after partition is repaired
        let _ = tokio::time::timeout(
            core::time::Duration::from_secs(5),
            socket.recv_from(&mut buf),
        )
        .await;

        Ok(())
    });

    sim.client("client", async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:0").await?;
        let server_addr: SocketAddr = (turmoil::lookup("server"), 9000).into();

        // Partition the network - packets will be dropped
        turmoil::partition("client", "server");

        // Send while partitioned (will be dropped)
        let _ = socket.send_to(b"dropped", server_addr).await;

        // Wait a bit
        tokio::time::sleep(core::time::Duration::from_millis(100)).await;

        // Repair the network
        turmoil::repair("client", "server");

        // This should now succeed
        socket.send_to(b"delivered", server_addr).await?;

        Ok(())
    });

    sim.run()
}

/// Test using the Socket trait poll methods directly
#[test]
fn socket_trait_poll_methods() -> Result {
    use crate::msg::addr::Addr;
    use crate::msg::cmsg;
    use s2n_quic_core::inet::ExplicitCongestionNotification;
    use std::io::{IoSlice, IoSliceMut};

    let mut sim = Builder::new()
        .simulation_duration(core::time::Duration::from_secs(10))
        .build();

    sim.host("server", || async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:9000").await?;

        // Use poll_recv via the Socket trait
        let mut addr = Addr::default();
        let mut cmsg_recv = cmsg::Receiver::default();
        let mut buf = [0u8; 1024];
        let mut iov = [IoSliceMut::new(&mut buf)];

        let len =
            std::future::poll_fn(|cx| socket.poll_recv(cx, &mut addr, &mut cmsg_recv, &mut iov))
                .await?;

        assert!(len > 0);
        Ok(())
    });

    sim.client("client", async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:0").await?;
        let server_addr: SocketAddr = (turmoil::lookup("server"), 9000).into();

        // Use poll_send via the Socket trait
        let mut addr = Addr::default();
        addr.set(server_addr.into());

        let msg = b"hello via trait";
        let iov = [IoSlice::new(msg)];

        // Wait for socket to be writable, then send via poll_send
        let len = std::future::poll_fn(|cx| {
            socket.poll_send(cx, &addr, ExplicitCongestionNotification::Ect0, &iov)
        })
        .await?;

        assert_eq!(len, msg.len());
        Ok(())
    });

    sim.run()
}

/// Test that packets are dropped during partition
#[test]
fn packet_drops() -> Result {
    let mut sim = Builder::new()
        .simulation_duration(core::time::Duration::from_secs(10))
        .build();

    sim.host("server", || async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:9000").await?;
        let mut buf = [0u8; 1024];
        let mut received = Vec::new();

        // Collect messages with timeout
        loop {
            match tokio::time::timeout(
                core::time::Duration::from_millis(500),
                socket.recv_from(&mut buf),
            )
            .await
            {
                Ok(Ok((len, _))) => {
                    received.push(buf[..len].to_vec());
                }
                _ => break,
            }
        }

        // Should only receive "msg2" - "msg1" was dropped during partition
        assert_eq!(received.len(), 1);
        assert_eq!(&received[0], b"msg2");
        Ok(())
    });

    sim.client("client", async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:0").await?;
        let server_addr: SocketAddr = (turmoil::lookup("server"), 9000).into();

        // Partition network - packets will be dropped
        turmoil::partition("client", "server");
        socket.send_to(b"msg1", server_addr).await?;

        tokio::time::sleep(core::time::Duration::from_millis(50)).await;

        // Repair network
        turmoil::repair("client", "server");
        socket.send_to(b"msg2", server_addr).await?;

        tokio::time::sleep(core::time::Duration::from_millis(100)).await;
        Ok(())
    });

    sim.run()
}

/// Test packet hold/release - verifies held packets are delayed until released
#[test]
fn packet_hold_release() -> Result {
    let mut sim = Builder::new()
        .simulation_duration(core::time::Duration::from_secs(10))
        .build();

    sim.host("server", || async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:9000").await?;
        let mut buf = [0u8; 1024];

        // First receive should timeout - packets are held
        let first_result = tokio::time::timeout(
            core::time::Duration::from_millis(200),
            socket.recv_from(&mut buf),
        )
        .await;
        assert!(first_result.is_err(), "expected timeout while packets held");

        // After client releases, we should receive the message
        let (len, _) = tokio::time::timeout(
            core::time::Duration::from_secs(2),
            socket.recv_from(&mut buf),
        )
        .await
        .expect("should receive after release")?;

        assert_eq!(&buf[..len], b"held_msg");
        Ok(())
    });

    sim.client("client", async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:0").await?;
        let server_addr: SocketAddr = (turmoil::lookup("server"), 9000).into();

        // Hold packets from client to server
        turmoil::hold("client", "server");
        socket.send_to(b"held_msg", server_addr).await?;

        // Wait for server's first timeout attempt
        tokio::time::sleep(core::time::Duration::from_millis(300)).await;

        // Release held packets
        turmoil::release("client", "server");

        tokio::time::sleep(core::time::Duration::from_millis(200)).await;
        Ok(())
    });

    sim.run()
}
