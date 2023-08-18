// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic::{client::Connect, Client};

use std::{error::Error, net::SocketAddr};

pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let tls = s2n_quic::provider::tls::s2n_tls::Client::builder()
        .with_certificate(CERT_PEM)?
        .with_session_tickets()?
        .build()?;
    let client = Client::builder()
        .with_tls(tls)?
        .with_event(s2n_quic::provider::event::tracing::Subscriber::default())?
        .with_io("0.0.0.0:0")?
        .start()?;
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
    match tokio::io::copy(&mut stdin, &mut send_stream).await {
        Ok(num) => println!("sent! {:?}", num),
        Err(err) => println!("err! {:?}", err),
    }

    Ok(())
}
