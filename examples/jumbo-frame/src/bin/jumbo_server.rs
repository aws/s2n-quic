// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use jumbo_frame::MtuEventInformer;
use s2n_quic::Server;
use std::{error::Error, net::SocketAddr};

/// NOTE: this certificate is to be used for demonstration purposes only!
pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));
/// NOTE: this certificate is to be used for demonstration purposes only!
pub static KEY_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/key.pem"
));

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let address: SocketAddr = "127.0.0.1:4433".parse()?;

    // set up an io provider with jumbo mtu and larger socket buffers
    let io = s2n_quic::provider::io::Default::builder()
        .with_receive_address(address)?
        .with_max_mtu(9_001)?
        .with_recv_buffer_size(12_000_000)?
        .with_send_buffer_size(12_000_000)?
        .build()?;

    let mut server = Server::builder()
        .with_tls((CERT_PEM, KEY_PEM))?
        .with_io(io)?
        .with_event(MtuEventInformer {})?
        .start()?;

    eprintln!("Listening for a connection");
    let connection = server.accept().await.unwrap();

    eprintln!("Connection accepted from {:?}", connection.remote_addr());

    // we aren't sending any data, but the endpoint will be probing
    // for higher mtu's during this time.
    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

    Ok(())
}
