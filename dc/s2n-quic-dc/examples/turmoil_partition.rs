// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Network partition testing example using turmoil.
//!
//! Run with: `cargo run --example turmoil_partition --features unstable-provider-io-turmoil`

use std::net::SocketAddr;
use turmoil::{Builder, Result};

fn main() -> Result {
    let mut sim = Builder::new()
        .simulation_duration(core::time::Duration::from_secs(30))
        .build();

    sim.host("server", || async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:9000").await?;
        println!("Server listening on {:?}", socket.local_addr()?);

        let mut buf = [0u8; 1024];

        // First receive will timeout due to partition
        match tokio::time::timeout(
            core::time::Duration::from_secs(3),
            socket.recv_from(&mut buf),
        )
        .await
        {
            Ok(Ok((len, peer))) => {
                println!("Server received {} bytes from {} (unexpected!)", len, peer);
            }
            Ok(Err(e)) => println!("Server recv error: {}", e),
            Err(_) => println!("Server: first recv timed out (expected due to partition)"),
        }

        // Second receive should succeed after repair
        match tokio::time::timeout(
            core::time::Duration::from_secs(10),
            socket.recv_from(&mut buf),
        )
        .await
        {
            Ok(Ok((len, peer))) => {
                println!(
                    "Server received: {:?} from {}",
                    std::str::from_utf8(&buf[..len]).unwrap(),
                    peer
                );
            }
            Ok(Err(e)) => println!("Server recv error: {}", e),
            Err(_) => println!("Server: second recv timed out (unexpected!)"),
        }

        Ok(())
    });

    sim.client("client", async move {
        let socket = turmoil::net::UdpSocket::bind("0.0.0.0:0").await?;
        let server_addr: SocketAddr = (turmoil::lookup("server"), 9000).into();

        // Partition the network - all packets between client and server are dropped
        println!("Client: partitioning network");
        turmoil::partition("client", "server");

        // Send while partitioned (will be dropped)
        socket.send_to(b"dropped message", server_addr).await?;
        println!("Client: sent message during partition (will be dropped)");

        // Wait during partition
        tokio::time::sleep(core::time::Duration::from_secs(5)).await;

        // Repair the network
        println!("Client: repairing network");
        turmoil::repair("client", "server");

        // This message should be delivered
        socket.send_to(b"delivered message", server_addr).await?;
        println!("Client: sent message after repair (should be delivered)");

        Ok(())
    });

    sim.run()
}
