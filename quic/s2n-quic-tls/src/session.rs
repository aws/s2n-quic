// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::callback::{self, Callback};
use bytes::BytesMut;
use core::{marker::PhantomData, task::Poll};
use s2n_quic_core::{
    application::ServerName,
    crypto::{tls, CryptoError, CryptoSuite},
    endpoint, transport,
};
use s2n_quic_crypto::Suite;
use s2n_tls::{
    config::Config,
    connection::Connection,
    enums::{Blinding, Mode},
    error::Error,
};

#[derive(Debug)]
pub struct Session {
    endpoint: endpoint::Type,
    pub(crate) connection: Connection,
    state: callback::State,
    handshake_complete: bool,
    send_buffer: BytesMut,
    emitted_server_name: bool,
    // This is only set for the client to avoid an extra allocation
    server_name: Option<ServerName>,
}

impl Session {
    pub fn new(
        endpoint: endpoint::Type,
        config: Config,
        params: &[u8],
        server_name: Option<ServerName>,
    ) -> Result<Self, Error> {
        let mut connection = Connection::new(match endpoint {
            endpoint::Type::Server => Mode::Server,
            endpoint::Type::Client => Mode::Client,
        });

        connection.set_config(config)?;
        connection.enable_quic()?;
        connection.set_quic_transport_parameters(params)?;
        // QUIC handles sending alerts, so no need to apply TLS blinding
        connection.set_blinding(Blinding::SelfService)?;

        if let Some(server_name) = server_name.as_ref() {
            connection
                .set_server_name(server_name)
                .expect("invalid server name value");
        }

        Ok(Self {
            endpoint,
            connection,
            state: Default::default(),
            handshake_complete: false,
            send_buffer: BytesMut::new(),
            emitted_server_name: false,
            server_name,
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
            emitted_server_name: &mut self.emitted_server_name,
            server_name: &self.server_name,
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
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(e)) => Poll::Ready(Err(e
                .alert()
                .map(CryptoError::new)
                .unwrap_or(CryptoError::HANDSHAKE_FAILURE)
                .into())),
            Poll::Pending => Poll::Pending,
        }
    }
}
