// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use jumbo_frame::MtuEventInformer;
use s2n_quic::{client::Connect, Client};
use std::{error::Error, net::SocketAddr};
use tokio::time::Duration;

/// NOTE: this certificate is to be used for demonstration purposes only!
pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let address: SocketAddr = "0.0.0.0:2198".parse()?;

    // set up an io provider with jumbo mtu and larger socket buffers
    let io = s2n_quic::provider::io::Default::builder()
        .with_max_mtu(9001)?
        .with_receive_address(address)?
        .with_recv_buffer_size(12_000_000)?
        .with_send_buffer_size(12_000_000)?
        .build()?;
    let client = Client::builder()
        .with_tls(CERT_PEM)?
        .with_io(io)?
        .with_event(MtuEventInformer {})?
        .start()?;
    let addr: SocketAddr = "127.0.0.1:4433".parse()?;
    let connect = Connect::new(addr).with_server_name("localhost");
    let mut connection = client.connect(connect).await?;

    // ensure the connection doesn't time out with inactivity
    connection.keep_alive(true)?;

    // we aren't actually sending any data, but during this time the quic
    // endpoint will be probing to see if we can use a 9_001 byte mtu
    // if we were sending data that would happen concurrently with the mtu
    // probing.
    tokio::time::sleep(Duration::from_secs(3)).await;

    Ok(())
}
