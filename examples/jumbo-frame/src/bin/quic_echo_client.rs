// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::{client::Connect, provider::io::TryInto, Client};
use std::{error::Error, net::SocketAddr};

/// NOTE: this certificate is to be used for demonstration purposes only!
pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let address: SocketAddr = "0.0.0.0:2198".parse()?;
    let io = s2n_quic::provider::io::Default::builder()
        .with_max_mtu(9001)?
        .with_receive_address(address)?
        .with_recv_buffer_size(12_000_000)?
        .with_send_buffer_size(12_000_000)?
        .build()?;
    println!("got the io created");
    let client = Client::builder().with_tls(CERT_PEM)?.with_io(io)?.start()?;
    println!("the client is started");
    let addr: SocketAddr = "127.0.0.1:4433".parse()?;
    let connect = Connect::new(addr).with_server_name("localhost");
    let mut connection = client.connect(connect).await?;

    // ensure the connection doesn't time out with inactivity
    connection.keep_alive(true)?;

    // open a new stream and split the receiving and sending sides
    let stream = connection.open_bidirectional_stream().await?;
    let (mut receive_stream, mut send_stream) = stream.split();

    // spawn a task that copies responses from the server to stdout
    tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        let _ = tokio::io::copy(&mut receive_stream, &mut stdout).await;
    });

    // copy data from stdin and send it to the server
    let mut stdin = tokio::io::stdin();
    tokio::io::copy(&mut stdin, &mut send_stream).await?;

    Ok(())
}
