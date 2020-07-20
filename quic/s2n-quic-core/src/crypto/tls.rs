use crate::crypto::{CryptoError, CryptoSuite};
pub use bytes::{Bytes, BytesMut};
use s2n_codec::EncoderValue;

/// Holds all application parameters which are exchanged within the TLS handshake.
#[derive(Debug)]
pub struct ApplicationParameters<'a> {
    /// The negotiated Application Layer Protocol
    pub alpn_protocol: Option<&'a [u8]>,
    /// Server Name Indication
    pub sni: Option<&'a [u8]>,
    /// Encoded transport parameters
    pub transport_parameters: &'a [u8],
}

pub trait Context<Crypto: CryptoSuite> {
    fn on_handshake_keys(&mut self, keys: Crypto::HandshakeCrypto) -> Result<(), CryptoError>;
    fn on_zero_rtt_keys(
        &mut self,
        keys: Crypto::ZeroRTTCrypto,
        application_parameters: ApplicationParameters,
    ) -> Result<(), CryptoError>;
    fn on_one_rtt_keys(
        &mut self,
        keys: Crypto::OneRTTCrypto,
        application_parameters: ApplicationParameters,
    ) -> Result<(), CryptoError>;

    fn receive_initial(&mut self) -> Option<Bytes>;
    fn receive_handshake(&mut self) -> Option<Bytes>;
    fn receive_application(&mut self) -> Option<Bytes>;

    fn can_send_initial(&self) -> bool;
    fn send_initial(&mut self, transmission: Bytes);

    fn can_send_handshake(&self) -> bool;
    fn send_handshake(&mut self, transmission: Bytes);

    fn can_send_application(&self) -> bool;
    fn send_application(&mut self, transmission: Bytes);
}

pub trait Endpoint: Sized {
    type Session: Session;

    fn new_server_session<Params: EncoderValue>(
        &mut self,
        transport_parameters: &Params,
    ) -> Self::Session;

    fn new_client_session<Params: EncoderValue>(
        &mut self,
        transport_parameters: &Params,
        sni: &[u8],
    ) -> Self::Session;
}

pub trait Session: CryptoSuite + Sized + Send {
    fn poll<C: Context<Self>>(&mut self, context: &mut C) -> Result<(), CryptoError>;
}
