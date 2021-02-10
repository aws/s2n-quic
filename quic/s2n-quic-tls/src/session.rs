use crate::callback::{self, Callback};
use bytes::BytesMut;
use core::{marker::PhantomData, task::Poll};
use s2n_quic_core::{
    crypto::{tls, CryptoError, CryptoSuite},
    endpoint,
    transport::error::TransportError,
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
    handshake_done: bool,
    send_buffer: BytesMut,
}

unsafe impl Send for Session {}

impl Session {
    pub fn new(endpoint: endpoint::Type, config: Config, params: &[u8]) -> Result<Self, Error> {
        let mut connection = Connection::new(match endpoint {
            endpoint::Type::Server => Mode::Server,
            endpoint::Type::Client => Mode::Client,
        });

        connection.set_config(config)?;
        connection.set_quic_transport_parameters(params)?;
        connection.set_blinding(s2n_blinding::SelfServiceBlinding)?;

        Ok(Self {
            endpoint,
            connection,
            state: Default::default(),
            handshake_done: false,
            send_buffer: BytesMut::new(),
        })
    }

    fn translate_error(&self, error: Error) -> TransportError {
        if error.kind() == s2n_error_type::Alert {
            if let Some(code) = self.connection.alert() {
                return CryptoError::new(code).into();
            }
        }

        TransportError::INTERNAL_ERROR
    }
}

impl CryptoSuite for Session {
    type HandshakeCrypto = <RingCryptoSuite as CryptoSuite>::HandshakeCrypto;
    type InitialCrypto = <RingCryptoSuite as CryptoSuite>::InitialCrypto;
    type OneRTTCrypto = <RingCryptoSuite as CryptoSuite>::OneRTTCrypto;
    type ZeroRTTCrypto = <RingCryptoSuite as CryptoSuite>::ZeroRTTCrypto;
    type RetryCrypto = <RingCryptoSuite as CryptoSuite>::RetryCrypto;
}

impl tls::Session for Session {
    fn poll<W>(&mut self, context: &mut W) -> Result<(), TransportError>
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
                if !self.handshake_done {
                    context.on_handshake_done()?;
                    self.handshake_done = true;
                }
                Ok(())
            }
            Poll::Ready(Err(err)) => Err(self.translate_error(err)),
            Poll::Pending => Ok(()),
        }
    }
}
