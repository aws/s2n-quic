// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::provider::tls::s2n_tls::{ConfigLoader, ConnectionContext, Server};
use std::{error::Error, time::SystemTime};

pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));
pub static KEY_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/key.pem"
));

pub static TICKET_KEY: [u8; 16] = [0; 16];
pub static TICKET_KEY_NAME: &[u8] = "keyname".as_bytes();

struct ResumptionConfig;
impl ConfigLoader for ResumptionConfig {
    fn load(&mut self, _cx: ConnectionContext) -> s2n_tls::config::Config {
        let mut config_builder = s2n_tls::config::Builder::new();
        config_builder
            .enable_session_tickets(true)
            .unwrap()
            .add_session_ticket_key(TICKET_KEY_NAME, &TICKET_KEY, SystemTime::now())
            .unwrap()
            .load_pem(CERT_PEM.as_bytes(), KEY_PEM.as_bytes())
            .unwrap()
            .set_security_policy(&s2n_tls::security::DEFAULT_TLS13)
            .unwrap()
            .enable_quic()
            .unwrap()
            .set_application_protocol_preference([b"h3"])
            .unwrap();
        config_builder.build().unwrap()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let tls = Server::from_loader(ResumptionConfig);
    let mut server = s2n_quic::Server::builder()
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

                    while let Ok(Some(data)) = stream.receive().await {
                        stream.send(data).await.expect("stream should be open");
                    }
                });
            }
        });
    }
    Ok(())
}
