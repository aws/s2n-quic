// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use s2n_quic::{
    client::Connect,
    provider::tls::s2n_tls::{
        callbacks::{ConnectionFuture, SessionTicket, SessionTicketCallback},
        config::ConnectionInitializer,
        connection,
        error::Error,
        Client,
    },
};
use std::{
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
};

/// NOTE: this certificate is to be used for demonstration purposes only!
pub static CERT_PEM: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../quic/s2n-quic-core/certs/cert.pem"
));

#[derive(Default, Clone)]
pub struct SessionTicketHandler {
    stored_ticket: Arc<Mutex<Option<Vec<u8>>>>,
}

impl ConnectionInitializer for SessionTicketHandler {
    fn initialize_connection(
        &self,
        connection: &mut connection::Connection,
    ) -> Result<Option<Pin<Box<(dyn ConnectionFuture)>>>, Error> {
        if let Some(ticket) = (*self.stored_ticket).lock().unwrap().as_deref() {
            connection.set_session_ticket(ticket)?;
        }
        Ok(None)
    }
}

// Implement the session ticket callback that stores the SessionTicket data
impl SessionTicketCallback for SessionTicketHandler {
    fn on_session_ticket(
        &self,
        _connection: &mut connection::Connection,
        session_ticket: &SessionTicket,
    ) {
        let size = session_ticket.len().unwrap();
        let mut data = vec![0; size];
        session_ticket.data(&mut data).unwrap();
        let mut ticket = (*self.stored_ticket).lock().unwrap();
        if ticket.is_none() {
            *ticket = Some(data);
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut tls = Client::builder()
        .with_certificate(CERT_PEM)?
        .with_key_logging()?;
    let handler = SessionTicketHandler::default();
    let config = tls.config_mut();
    config
        .enable_session_tickets(true)?
        .set_session_ticket_callback(handler.clone())?
        .set_connection_initializer(handler.clone())?;

    let client = s2n_quic::Client::builder()
        .with_tls(tls.build()?)?
        .with_io("0.0.0.0:0")?
        .start()?;

    let addr: SocketAddr = "127.0.0.1:4433".parse()?;
    let connect = Connect::new(addr).with_server_name("localhost");
    let _connection = client.connect(connect.clone()).await?;

    // Give the client a chance to receive the session ticket since it is sent after the handshake
    tokio::time::sleep(std::time::Duration::new(1, 0)).await;

    let _connection = client.connect(connect).await?;
    Ok(())
}
