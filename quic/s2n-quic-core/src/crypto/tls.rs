use crate::crypto::{CryptoError, CryptoSuite};
pub use bytes::{Bytes, BytesMut};

/// Holds all application parameters which are exchanged within the TLS handshake.
#[derive(Debug)]
pub struct TLSApplicationParameters<'a> {
    /// The negotiated Application Layer Protocol
    pub alpn_protocol: Option<&'a [u8]>,
    /// Server Name Indication
    pub sni: Option<&'a [u8]>,
    /// Encoded transport parameters
    pub transport_parameters: &'a [u8],
}

pub trait TLSContext<Crypto: CryptoSuite> {
    fn on_handshake_keys(&mut self, keys: Crypto::HandshakeCrypto) -> Result<(), CryptoError>;
    fn on_zero_rtt_keys(
        &mut self,
        keys: Crypto::ZeroRTTCrypto,
        application_parameters: TLSApplicationParameters,
    ) -> Result<(), CryptoError>;
    fn on_one_rtt_keys(
        &mut self,
        keys: Crypto::OneRTTCrypto,
        application_parameters: TLSApplicationParameters,
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

pub trait TLSEndpoint: Sized {
    type Session: TLSSession;

    fn new_server_session(&mut self) -> Self::Session;
    fn new_client_session(&mut self, sni: &[u8]) -> Self::Session;
}

pub trait TLSSession: CryptoSuite + Sized + Send {
    fn poll<C: TLSContext<Self>>(&mut self, context: &mut C) -> Result<(), CryptoError>;
}
