// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Basic UDP echo example using turmoil network simulation.
//!
//! Run with: `cargo run --example turmoil_udp_echo --features unstable-provider-io-turmoil`

use s2n_quic_dc::stream::socket::{Protocol, Socket};
use s2n_quic_dc::stream::TransportFeatures;
use std::net::SocketAddr;
use turmoil::{Builder, Result};

fn main() -> Result {
    let mut sim = Builder::new()
        .simulation_duration(core::time::Duration::from_secs(10))
        .build();

    sim.host("server", || async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:9000").await?;

        // The Socket trait is automatically implemented
        assert_eq!(socket.protocol(), Protocol::Udp);
        assert_eq!(socket.features(), TransportFeatures::UDP);

        println!("Server listening on {:?}", socket.local_addr()?);

        let mut buf = [0u8; 1024];
        let (len, peer) = socket.recv_from(&mut buf).await?;
        println!("Server received {} bytes from {}", len, peer);

        socket.send_to(&buf[..len], peer).await?;
        println!("Server echoed data back");

        Ok(())
    });

    sim.client("client", async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:0").await?;
        let server_addr: SocketAddr = (turmoil::lookup("server"), 9000).into();

        let msg = b"hello turmoil";
        socket.send_to(msg, server_addr).await?;
        println!("Client sent: {:?}", std::str::from_utf8(msg).unwrap());

        let mut buf = [0u8; 1024];
        let (len, _) = socket.recv_from(&mut buf).await?;
        println!(
            "Client received: {:?}",
            std::str::from_utf8(&buf[..len]).unwrap()
        );

        assert_eq!(&buf[..len], msg);
        println!("Echo verified!");

        Ok(())
    });

    sim.run()
}
