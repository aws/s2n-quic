// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::{client::Connect, Client};
use std::{error::Error, net::SocketAddr};
use tokio::io::AsyncWriteExt;

/// NOTE: this certificate is to be used for demonstration purposes only!
pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut client = Client::builder()
        .with_tls(CERT_PEM)?
        .with_io("0.0.0.0:0")?
        .start()?;

    let addr: SocketAddr = "127.0.0.1:4433".parse()?;
    let connect = Connect::new(addr).with_server_name("localhost");
    let mut connection = client.connect(connect).await?;

    let mut stream = connection.open_bidirectional_stream().await?;
    stream.write_all(b"hello post-quantum server!").await?;

    let mut stdout = tokio::io::stdout();
    tokio::io::copy(&mut stream, &mut stdout).await?;

    connection.close(0u8.into());

    client.wait_idle().await?;

    Ok(())
}
