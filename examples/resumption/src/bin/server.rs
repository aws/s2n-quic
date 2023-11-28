// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::provider::tls::s2n_tls::Server;
use std::{error::Error, time::SystemTime};

pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));
pub static KEY_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/key.pem"
));

/// NOTE: this key is for testing and should NOT be used in production!
pub static TICKET_KEY: [u8; 16] = [0; 16];
pub static TICKET_KEY_NAME: &[u8] = "keyname".as_bytes();

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut tls = Server::builder().with_certificate(CERT_PEM, KEY_PEM)?;

    tls.config_mut()
        .add_session_ticket_key(TICKET_KEY_NAME, &TICKET_KEY, SystemTime::now())?;

    let mut server = s2n_quic::Server::builder()
        .with_tls(tls.build()?)?
        .with_io("127.0.0.1:4433")?
        .start()?;

    while let Some(mut connection) = server.accept().await {
        // spawn a new task for the connection
        tokio::spawn(async move {
            eprintln!("Connection accepted from {:?}", connection.remote_addr());

            while let Ok(Some(stream)) = connection.accept_bidirectional_stream().await {
                // spawn a new task for the stream
                tokio::spawn(async move {
                    eprintln!("Stream opened from {:?}", stream.connection().remote_addr());
                });
            }
        });
    }
    Ok(())
}
