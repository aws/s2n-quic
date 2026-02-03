// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Example using the Socket trait's poll methods with turmoil.
//!
//! Run with: `cargo run --example turmoil_socket_trait --features unstable-provider-io-turmoil`

use s2n_quic_dc::msg::addr::Addr;
use s2n_quic_dc::msg::cmsg;
use s2n_quic_dc::stream::socket::Socket;
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::io::{IoSlice, IoSliceMut};
use std::net::SocketAddr;
use turmoil::{Builder, Result};

fn main() -> Result {
    let mut sim = Builder::new()
        .simulation_duration(core::time::Duration::from_secs(10))
        .build();

    sim.host("server", || async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:9000").await?;
        println!("Server listening on {:?}", socket.local_addr()?);

        // Use poll_recv via the Socket trait
        let mut addr = Addr::default();
        let mut cmsg_recv = cmsg::Receiver::default();
        let mut buf = [0u8; 1024];
        let mut iov = [IoSliceMut::new(&mut buf)];

        let len =
            std::future::poll_fn(|cx| socket.poll_recv(cx, &mut addr, &mut cmsg_recv, &mut iov))
                .await?;

        println!(
            "Server received {} bytes via poll_recv: {:?}",
            len,
            std::str::from_utf8(&buf[..len]).unwrap()
        );

        Ok(())
    });

    sim.client("client", async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:0").await?;
        let server_addr: SocketAddr = (turmoil::lookup("server"), 9000).into();

        // Use poll_send via the Socket trait
        let mut addr = Addr::default();
        addr.set(server_addr.into());

        let msg = b"hello via Socket trait";
        let iov = [IoSlice::new(msg)];

        let len = std::future::poll_fn(|cx| {
            socket.poll_send(cx, &addr, ExplicitCongestionNotification::Ect0, &iov)
        })
        .await?;

        println!("Client sent {} bytes via poll_send", len);
        assert_eq!(len, msg.len());

        Ok(())
    });

    sim.run()
}
