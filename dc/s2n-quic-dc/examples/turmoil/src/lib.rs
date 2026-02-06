// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_dc::stream::testing::{server, Client, Server};
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use turmoil::{Builder, Result};

async fn run_server(server: Server) -> io::Result<()> {
    let (mut stream, _) = server.accept().await?;
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).await?;
    stream.write_all(&buf[..n]).await?;
    stream.shutdown().await?;
    Ok(())
}

async fn run_client(client: Client, server: &server::Handle) -> io::Result<()> {
    let mut stream = client.connect_to(server).await?;
    let msg = b"hello s2n-quic-dc!";
    stream.write_all(msg).await?;
    stream.shutdown().await?;
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).await?;
    assert_eq!(&buf[..n], msg);
    Ok(())
}

#[test]
fn echo_test() -> Result {
    let mut sim = Builder::new()
        .simulation_duration(core::time::Duration::from_secs(30))
        .build();

    sim.host("server", || async move {
        let server = Server::udp().port(9000).build();
        run_server(server).await.map_err(|e| e.into())
    });

    sim.client("client", async move {
        let client = Client::builder().build();
        let server = Server::udp().port(9000).build();
        run_client(client, &server.handle()).await.map_err(|e| e.into())
    });

    sim.run()
}

#[test]
fn partition_test() -> Result {
    let mut sim = Builder::new()
        .simulation_duration(core::time::Duration::from_secs(30))
        .build();

    sim.host("server", || async move {
        let server = Server::udp().port(9000).build();
        match tokio::time::timeout(core::time::Duration::from_secs(5), server.accept()).await {
            Ok(Ok((mut stream, _))) => {
                let mut buf = [0u8; 1024];
                if let Ok(n) = stream.read(&mut buf).await {
                    let msg = String::from_utf8_lossy(&buf[..n]);
                    assert!(!msg.contains("during partition"));
                }
            }
            _ => {}
        }
        Ok(())
    });

    sim.client("client", async move {
        let client = Client::builder().build();
        let server = Server::udp().port(9000).build();

        turmoil::partition("client", "server");
        let _ = tokio::time::timeout(
            core::time::Duration::from_millis(500),
            client.connect_to(&server.handle()),
        )
        .await;

        turmoil::repair("client", "server");
        let mut stream = client.connect_to(&server.handle()).await?;
        stream.write_all(b"after repair").await?;
        stream.shutdown().await?;
        Ok(())
    });

    sim.run()
}
