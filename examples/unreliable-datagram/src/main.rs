// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::Server;
use std::error::Error;

/// NOTE: this certificate/key pair is to be used for demonstration purposes only!
pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));
pub static KEY_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/key.pem"
));

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let datagram_provider = s2n_quic_core::datagram::default::DatagramEndpoint::builder()
        .with_send_capacity(200)
        .build()
        .unwrap();

    // Build an `s2n_quic::Server`
    let mut server = Server::builder()
        .with_tls((CERT_PEM, KEY_PEM))?
        .with_io("127.0.0.1:4433")?
        .with_datagram(datagram_provider)?
        .start()?;

    // while let Some(mut connection) = server.accept().await {
    //     // spawn a new task for the connection
    //     tokio::spawn(async move {
    //         eprintln!("Connection accepted from {:?}", connection.remote_addr());

    //         connection.datagram_sender(
    //             |provider: &mut s2n_quic_core::datagram::default::DatagramEndpoint| {
    //                 provider.send_datagram(bytes::Bytes::from_static(&[1, 2, 3]))
    //             },
    //         );
    //     });
    // }

    Ok(())
}
