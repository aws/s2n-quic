// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::callback::{self, Callback};
use bytes::BytesMut;
use core::{marker::PhantomData, task::Poll};
use s2n_quic_core::{
    application::ServerName,
    crypto::{
        tls::{self, CipherSuite},
        CryptoSuite,
    },
    endpoint, ensure,
    path::LocalAddress,
    transport,
};
use s2n_quic_crypto::Suite;
use s2n_tls::{
    config::Config,
    connection::Connection,
    enums::{Blinding, Mode},
    error::{Error, ErrorType},
};

#[derive(Debug)]
pub struct Session {
    endpoint: endpoint::Type,
    pub(crate) connection: Connection,
    state: callback::State,
    handshake_complete: bool,
    send_buffer: BytesMut,
    // This field is used to minimize allocations for the client.
    // No allocation needs to occur when the on_server_name callback triggers
    // since the client has already stored the server_name at the beginning
    // of a session.
    server_name: Option<ServerName>,
    received_ticket: bool,
    server_params: Vec<u8>,
}

impl Session {
    pub fn new(
        endpoint: endpoint::Type,
        config: Config,
        params: &[u8],
        server_name: Option<ServerName>,
        server_local_addr: Option<LocalAddress>,
    ) -> Result<Self, Error> {
        let mut connection = Connection::new(match endpoint {
            endpoint::Type::Server => Mode::Server,
            endpoint::Type::Client => Mode::Client,
        });

        connection.set_config(config)?;
        connection.enable_quic()?;

        let server_params = {
            if endpoint.is_client() {
                connection.set_quic_transport_parameters(params)?;
                Vec::new()
            } else {
                // Save the server's transport parameters for later, in case
                // additional values need to be appended
                params.to_vec()
            }
        };

        // QUIC handles sending alerts, so no need to apply TLS blinding
        connection.set_blinding(Blinding::SelfService)?;

        if let Some(server_name) = server_name.as_ref() {
            connection
                .set_server_name(server_name)
                .expect("invalid server name value");
        }

        if let Some(server_local_addr) = server_local_addr {
            connection.set_application_context(server_local_addr);
        }

        Ok(Self {
            endpoint,
            connection,
            state: Default::default(),
            handshake_complete: false,
            send_buffer: BytesMut::new(),
            server_name,
            received_ticket: false,
            server_params,
        })
    }
}

impl CryptoSuite for Session {
    type HandshakeKey = <Suite as CryptoSuite>::HandshakeKey;
    type HandshakeHeaderKey = <Suite as CryptoSuite>::HandshakeHeaderKey;
    type InitialKey = <Suite as CryptoSuite>::InitialKey;
    type InitialHeaderKey = <Suite as CryptoSuite>::InitialHeaderKey;
    type OneRttKey = <Suite as CryptoSuite>::OneRttKey;
    type OneRttHeaderKey = <Suite as CryptoSuite>::OneRttHeaderKey;
    type ZeroRttKey = <Suite as CryptoSuite>::ZeroRttKey;
    type ZeroRttHeaderKey = <Suite as CryptoSuite>::ZeroRttHeaderKey;
    type RetryKey = <Suite as CryptoSuite>::RetryKey;
}

impl tls::TlsSession for Session {
    fn tls_exporter(
        &self,
        label: &[u8],
        context: &[u8],
        output: &mut [u8],
    ) -> Result<(), tls::TlsExportError> {
        self.connection
            .tls_exporter(label, context, output)
            .map_err(|_| tls::TlsExportError::failure())
    }

    fn cipher_suite(&self) -> CipherSuite {
        self.state.cipher_suite()
    }

    fn peer_cert_chain_der(&self) -> Result<Vec<Vec<u8>>, tls::ChainError> {
        self.connection
            .peer_cert_chain()
            .map_err(|_| tls::ChainError::failure())?
            .iter()
            .map(|v| Ok(v?.der()?.to_vec()))
            .collect::<Result<Vec<Vec<u8>>, s2n_tls::error::Error>>()
            .map_err(|_| tls::ChainError::failure())
    }
}

impl tls::Session for Session {
    fn poll<W>(&mut self, context: &mut W) -> Poll<Result<(), transport::Error>>
    where
        W: tls::Context<Self>,
    {
        let mut callback: Callback<W, Self> = Callback {
            context,
            endpoint: self.endpoint,
            state: &mut self.state,
            suite: PhantomData,
            err: None,
            send_buffer: &mut self.send_buffer,
            server_name: &mut self.server_name,
            server_params: &mut self.server_params,
        };

        unsafe {
            // Safety: the callback struct must live as long as the callbacks are
            // set on on the connection
            callback.set(&mut self.connection);
        }

        let result = self.connection.poll_negotiate().map_ok(|_| ());

        callback.unset(&mut self.connection)?;

        match result {
            Poll::Ready(Ok(())) => {
                // s2n-tls has indicated that the handshake is complete
                if !self.handshake_complete {
                    self.state.on_handshake_complete();
                    context.on_handshake_complete()?;
                    context.on_tls_exporter_ready(self)?;
                    self.handshake_complete = true;
                }
                // TODO Add new s2n-tls new api, take and put in quic::connection
                // let ctx: Option<Box<dyn Any + Send + Sync>> =
                //     self.connection.take_tls_context();
                // context.on_tls_context(ctx);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => {
                context.on_tls_handshake_failed(self, &e)?;

                Poll::Ready(Err(e
                    .alert()
                    .map(tls::Error::new)
                    .unwrap_or(tls::Error::HANDSHAKE_FAILURE)
                    .with_reason(e.message())
                    .into()))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn process_post_handshake_message<W>(&mut self, context: &mut W) -> Result<(), transport::Error>
    where
        W: tls::Context<Self>,
    {
        let mut callback: Callback<W, Self> = Callback {
            context,
            endpoint: self.endpoint,
            state: &mut self.state,
            suite: PhantomData,
            err: None,
            send_buffer: &mut self.send_buffer,
            server_name: &mut self.server_name,
            server_params: &mut self.server_params,
        };

        unsafe {
            // Safety: the callback struct must live as long as the callbacks are
            // set on on the connection
            callback.set(&mut self.connection);
        }

        let result = self
            .connection
            .quic_process_post_handshake_message()
            .map(|_| ());

        callback.unset(&mut self.connection)?;

        match result {
            Ok(_) => {
                self.received_ticket = true;
                Ok(())
            }
            Err(e) => {
                // Blocking errors are the only type of s2n-tls error
                // that can be retried.
                if matches!(e.kind(), ErrorType::Blocked) {
                    Ok(())
                } else {
                    Err(e
                        .alert()
                        .map(tls::Error::new)
                        .unwrap_or(tls::Error::HANDSHAKE_FAILURE)
                        .into())
                }
            }
        }
    }

    fn should_discard_session(&self) -> bool {
        // Only clients process post-handshake messages currently
        ensure!(self.endpoint.is_client(), true);

        // Clients that haven't enabled resumption can discard the session
        ensure!(self.connection.are_session_tickets_enabled(), true);

        // Discard the session once a ticket is received
        self.received_ticket
    }
}
