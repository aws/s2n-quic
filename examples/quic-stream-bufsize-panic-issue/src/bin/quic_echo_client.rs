// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::{client::Connect, Client};
use std::{error::Error, net::SocketAddr};
use std::time::Duration;
use tokio::io::AsyncWriteExt;

/// NOTE: this certificate is to be used for demonstration purposes only!
pub static CERT_PEM: &str = include_str!(concat!(
env!("CARGO_MANIFEST_DIR"),
"/../../quic/s2n-quic-core/certs/cert.pem"
));

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let client = Client::builder()
        .with_tls(CERT_PEM)?
        .with_io("0.0.0.0:0")?
        .start()?;

    let addr: SocketAddr = "127.0.0.1:4433".parse()?;
    let connect = Connect::new(addr).with_server_name("localhost");
    let mut connection = client.connect(connect).await?;

    // ensure the connection doesn't time out with inactivity
    connection.keep_alive(true)?;

    // open a new stream and split the receiving and sending sides
    let mut stream = connection.open_bidirectional_stream().await?;

    let buf = vec![0u8; 1000 + 24];
    let split_index = 500;

    //send 500 bytes
    stream.write_all(& buf[..split_index]).await?;
    println!("send {} bytes", split_index);
    tokio::time::sleep(Duration::from_secs(1)).await;
    //then 524 bytes
    stream.write_all(& buf[split_index..]).await?;
    println!("send {} bytes", buf.len());


    Ok(())
}
