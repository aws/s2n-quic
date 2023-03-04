// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::task::Poll;
use s2n_quic::{
    client::Connect,
    provider::datagram::default::{Endpoint, Receiver},
    Client,
};
use std::{error::Error, net::SocketAddr};

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
    // Create a datagram provider that has recv queue capacity
    let datagram_provider = Endpoint::builder()
        .with_recv_capacity(200)?
        .build()
        .unwrap();

    // Build an `s2n_quic::Client`
    let client = Client::builder()
        .with_tls((CERT_PEM, KEY_PEM))?
        .with_io("0.0.0.0:0")?
        .with_datagram(datagram_provider)?
        .start()?;

    let addr: SocketAddr = "127.0.0.1:4433".parse()?;
    let connect = Connect::new(addr).with_server_name("localhost");
    let connection = client.connect(connect).await?;

    loop {
        let recv_result = futures::future::poll_fn(|cx| {
            // datagram_mut takes a closure which calls the requested datagram function. The type
            // of the closure parameter should be either the datagram Sender type or the
            // datagram Receiver type. The datagram_mut function will check this type against
            // its stored datagram Sender and Receiver, and if the type matches, the requested
            // function will execute. Here, that requested function is poll_recv_datagram.
            match connection.datagram_mut(|recv: &mut Receiver| recv.poll_recv_datagram(cx)) {
                // If the function is successfully called on the provider, it will return Poll<Bytes>.
                // Here we send an Ok() to wrap around the Bytes so the poll_fn doesn't complain.
                Ok(poll_value) => poll_value.map(Ok),
                // The datagram_mut function may return a query error if it can't find the type
                // referenced in the closure. Here we wrap the error in a Poll::Ready enum so the
                // poll_fn doesn't complain.
                Err(query_err) => Poll::Ready(Err(query_err)),
            }
        })
        .await;

        match recv_result {
            Ok(value) => eprintln!("RECV {:?}", value),
            Err(err) => {
                eprintln!("{:?}", err);
                return Ok(());
            }
        }
    }
}
