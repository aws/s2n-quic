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
use s2n_tls::{
    config::Config,
    connection::{Connection, Mode},
    error::Error,
    raw::{s2n_blinding, s2n_error_type},
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
            endpoint::Type::Server => Mode::Server,
            endpoint::Type::Client => Mode::Client,
        });

        connection.set_config(config)?;
        connection.enable_quic()?;
        connection.set_quic_transport_parameters(params)?;
        // QUIC handles sending alerts, so no need to apply TLS blinding
        connection.set_blinding(s2n_blinding::SelfServiceBlinding)?;

        Ok(Self {
            endpoint,
            connection,
            state: Default::default(),
            handshake_complete: false,
            send_buffer: BytesMut::new(),
        })
    }

    fn translate_error(&self, error: Error) -> transport::Error {
        if error.kind() == s2n_error_type::Alert {
            if let Some(code) = self.connection.alert() {
                return CryptoError::new(code).into();
            }
        }

        transport::Error::INTERNAL_ERROR
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
    fn poll<W>(&mut self, context: &mut W) -> Result<(), transport::Error>
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

        let result = self.connection.negotiate();

        callback.unset(&mut self.connection)?;

        match result {
            Poll::Ready(Ok(())) => {
                // only emit handshake done once
                if !self.handshake_complete {
                    context.on_handshake_complete()?;
                    self.handshake_complete = true;
                }
                Ok(())
            }
            Poll::Ready(Err(err)) => Err(self.translate_error(err)),
            Poll::Pending => Ok(()),
        }
    }
}
