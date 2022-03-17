// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::callback::{self, Callback};
use bytes::BytesMut;
use core::{marker::PhantomData, task::Poll};
use s2n_quic_core::{
    crypto::{tls, CryptoError, CryptoSuite},
    endpoint, transport,
};
use s2n_quic_ring::RingCryptoSuite;
use s2n_tls::raw::{
    config::Config,
    connection::Connection,
    error::Error,
    ffi::{s2n_blinding, s2n_mode},
};

#[derive(Debug)]
pub struct Session {
    endpoint: endpoint::Type,
    pub(crate) connection: Connection,
    state: callback::State,
    handshake_complete: bool,
    send_buffer: BytesMut,
}

impl Session {
    pub fn new(endpoint: endpoint::Type, config: Config, params: &[u8]) -> Result<Self, Error> {
        let mut connection = Connection::new(match endpoint {
            endpoint::Type::Server => s2n_mode::SERVER,
            endpoint::Type::Client => s2n_mode::CLIENT,
        });

        connection.set_config(config)?;
        connection.enable_quic()?;
        connection.set_quic_transport_parameters(params)?;
        // QUIC handles sending alerts, so no need to apply TLS blinding
        connection.set_blinding(s2n_blinding::SELF_SERVICE_BLINDING)?;

        Ok(Self {
            endpoint,
            connection,
            state: Default::default(),
            handshake_complete: false,
            send_buffer: BytesMut::new(),
        })
    }
}

impl CryptoSuite for Session {
    type HandshakeKey = <RingCryptoSuite as CryptoSuite>::HandshakeKey;
    type HandshakeHeaderKey = <RingCryptoSuite as CryptoSuite>::HandshakeHeaderKey;
    type InitialKey = <RingCryptoSuite as CryptoSuite>::InitialKey;
    type InitialHeaderKey = <RingCryptoSuite as CryptoSuite>::InitialHeaderKey;
    type OneRttKey = <RingCryptoSuite as CryptoSuite>::OneRttKey;
    type OneRttHeaderKey = <RingCryptoSuite as CryptoSuite>::OneRttHeaderKey;
    type ZeroRttKey = <RingCryptoSuite as CryptoSuite>::ZeroRttKey;
    type ZeroRttHeaderKey = <RingCryptoSuite as CryptoSuite>::ZeroRttHeaderKey;
    type RetryKey = <RingCryptoSuite as CryptoSuite>::RetryKey;
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
        };

        unsafe {
            // Safety: the callback struct must live as long as the callbacks are
            // set on on the connection
            callback.set(&mut self.connection);
        }

        let result = self.connection.negotiate().map_ok(|_| ());

        callback.unset(&mut self.connection)?;

        match result {
            Poll::Ready(Ok(())) => {
                // s2n-tls has indicated that the handshake is complete
                if !self.handshake_complete {
                    self.state.on_handshake_complete();
                    context.on_handshake_complete()?;
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
