// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::Path};

use s2n_quic::{provider::tls, Server};

pub static CA_CERT_PEM: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../rustls-mtls/certs/ca-cert.pem"
);
pub static SERVER_CERT_PEM: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../rustls-mtls/certs/server-cert.pem"
);
pub static SERVER_KEY_PEM: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../rustls-mtls/certs/server-key.pem"
);

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let tls = tls::default::Server::builder()
        .with_trusted_certificate(Path::new(CA_CERT_PEM))?
        .with_certificate(Path::new(SERVER_CERT_PEM), Path::new(SERVER_KEY_PEM))?
        .with_client_authentication()?
        .build()?;

    let mut server = Server::builder()
        .with_tls(tls)?
        .with_io("127.0.0.1:4433")?
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
