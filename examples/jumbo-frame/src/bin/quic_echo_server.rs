// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::Server;
use std::{error::Error, net::{SocketAddrV4, Ipv4Addr, SocketAddr}};

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
    let io = s2n_quic::provider::io::Default::builder()
        .with_receive_address(address)?
        // Specify that the endpoint should try to use frames up to 9001 bytes.
        // This is the maximum mtu that most ec2 instances will support.
        // https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/network_mtu.html
        .with_max_mtu(9001)?
        // It's wise to benchmark for your individual usecase, but for the high
        // throughput scenarios that jumbo frames sometimes enable, it is wise
        // to set larger buffers on the sockets.
        .with_recv_buffer_size(12_000_000)?
        .with_send_buffer_size(12_000_000)?
        .build()?;
    let mut server = Server::builder()
        .with_tls((CERT_PEM, KEY_PEM))?
        .with_io(io)?
        .with_event(s2n_quic::provider::event::tracing::Subscriber::default())?
        .start()?;

    while let Some(mut connection) = server.accept().await {
        // spawn a new task for the connection
        tokio::spawn(async move {
            eprintln!("Connection accepted from {:?}", connection.remote_addr());

            while let Ok(Some(mut stream)) = connection.accept_bidirectional_stream().await {
                // spawn a new task for the stream
                tokio::spawn(async move {
                    eprintln!("Stream opened from {:?}", stream.connection().remote_addr());

                    // echo any data back to the stream
                    while let Ok(Some(data)) = stream.receive().await {
                        stream.send(data).await.expect("stream should be open");
                    }
                });
            }
        });
    }

    Ok(())
}
