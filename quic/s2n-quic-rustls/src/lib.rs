#![forbid(unsafe_code)]

use core::fmt;
pub use rustls;
use rustls::{
    quic::{ClientQuicExt, QuicExt, Secrets, ServerQuicExt},
    ClientConfig, ProtocolVersion, ServerConfig, Session, SupportedCipherSuite,
};
use s2n_codec::{EncoderBuffer, EncoderValue};
use s2n_quic_core::crypto::{tls, CryptoError, CryptoSuite};
use s2n_quic_ring::{
    handshake::RingHandshakeCrypto, one_rtt::RingOneRTTCrypto, zero_rtt::RingZeroRTTCrypto, Prk,
    RingCryptoSuite, SecretPair,
};
use std::sync::Arc;
use webpki::DNSNameRef;

// The first 3 ciphers are TLS1.3
// https://github.com/ctz/rustls/blob/1287510bece905b7e45cf31d6e7cf3334b98bb2e/rustls/src/suites.rs#L379
pub static CIPHERSUITES: [&SupportedCipherSuite; 3] = [
    rustls::ALL_CIPHERSUITES[0],
    rustls::ALL_CIPHERSUITES[1],
    rustls::ALL_CIPHERSUITES[2],
];

pub static PROTOCOL_VERSIONS: [ProtocolVersion; 1] = [ProtocolVersion::TLSv1_3];

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
enum HandshakePhase {
    Initial,
    Handshake,
    Application,
}

impl Default for HandshakePhase {
    fn default() -> Self {
        Self::Initial
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct InnerSession<Session> {
    session: Session,
    phase: HandshakePhase,
    emitted_early_secret: bool,
}

impl<Session> InnerSession<Session> {
    fn new(session: Session) -> Self {
        Self {
            session,
            phase: Default::default(),
            emitted_early_secret: false,
        }
    }
}

macro_rules! impl_tls {
    ($endpoint:ident, $session:ident, $rustls_session:ident, $config:ident, $new:ident) => {
        pub struct $endpoint {
            config: Arc<rustls::$config>,
        }

        impl fmt::Debug for $endpoint {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.debug_struct(stringify!($endpoint)).finish()
            }
        }

        #[derive(Debug)]
        pub struct $session(Box<InnerSession<rustls::$rustls_session>>);

        impl $session {
            fn new(session: rustls::$rustls_session) -> Self {
                let session = InnerSession::new(session);
                Self(Box::new(session))
            }
        }

        impl CryptoSuite for $session {
            type HandshakeCrypto = <RingCryptoSuite as CryptoSuite>::HandshakeCrypto;
            type InitialCrypto = <RingCryptoSuite as CryptoSuite>::InitialCrypto;
            type OneRTTCrypto = <RingCryptoSuite as CryptoSuite>::OneRTTCrypto;
            type ZeroRTTCrypto = <RingCryptoSuite as CryptoSuite>::ZeroRTTCrypto;
        }

        impl tls::Session for $session {
            fn poll<W: tls::Context<Self>>(&mut self, context: &mut W) -> Result<(), CryptoError> {
                let crypto_data = match self.0.phase {
                    HandshakePhase::Initial => context.receive_initial(),
                    HandshakePhase::Handshake => context.receive_handshake(),
                    HandshakePhase::Application => context.receive_application(),
                };

                if let Some(crypto_data) = crypto_data {
                    self.receive(&crypto_data)?;
                }

                if let Some(early_secret) = self.early_secret() {
                    context.on_zero_rtt_keys(
                        RingZeroRTTCrypto::new(early_secret.clone()),
                        self.application_parameters()?,
                    )?;
                }

                loop {
                    let can_send = match self.0.phase {
                        HandshakePhase::Initial => context.can_send_initial(),
                        HandshakePhase::Handshake => context.can_send_handshake(),
                        HandshakePhase::Application => context.can_send_application(),
                    };

                    if !can_send {
                        return Ok(());
                    }

                    let mut transmission_buffer = vec![];

                    let key_upgrade = self.transmit(&mut transmission_buffer);

                    if transmission_buffer.is_empty() {
                        return Ok(());
                    }

                    if !transmission_buffer.is_empty() {
                        match self.0.phase {
                            HandshakePhase::Initial => {
                                context.send_initial(transmission_buffer.into())
                            }
                            HandshakePhase::Handshake => {
                                context.send_handshake(transmission_buffer.into())
                            }
                            HandshakePhase::Application => {
                                context.send_application(transmission_buffer.into())
                            }
                        }
                    }

                    if let Some(key_pair) = key_upgrade {
                        let algorithm = self
                            .0
                            .session
                            .get_negotiated_ciphersuite()
                            .expect("ciphersuite should be available")
                            .get_aead_alg();

                        match self.0.phase {
                            HandshakePhase::Initial => {
                                let keys = RingHandshakeCrypto::$new(algorithm, key_pair)
                                    .expect("invalid cipher");
                                self.0.phase = HandshakePhase::Handshake;
                                context.on_handshake_keys(keys)?;
                            }
                            HandshakePhase::Handshake | HandshakePhase::Application => {
                                let keys = RingOneRTTCrypto::$new(algorithm, key_pair)
                                    .expect("invalid cipher");
                                self.0.phase = HandshakePhase::Application;
                                context.on_one_rtt_keys(keys, self.application_parameters()?)?;
                            }
                        }
                    }
                }
            }
        }

        impl $session {
            fn receive(&mut self, crypto_data: &[u8]) -> Result<(), CryptoError> {
                self.0
                    .session
                    .read_hs(crypto_data)
                    .map_err(tls_error_reason)
                    .map_err(|reason| {
                        self.0
                            .session
                            .get_alert()
                            .map(|alert| CryptoError {
                                code: alert.get_u8(),
                                reason,
                            })
                            .unwrap_or_else(|| CryptoError {
                                code: 0,
                                reason: "",
                            })
                    })
            }

            fn application_parameters(&self) -> Result<tls::ApplicationParameters, CryptoError> {
                Ok(tls::ApplicationParameters {
                    alpn_protocol: self.0.session.get_alpn_protocol(),
                    transport_parameters: self.0.session.get_quic_transport_parameters().ok_or(
                        CryptoError::MISSING_EXTENSION
                            .with_reason("Missing QUIC transport parameters"),
                    )?,
                    sni: self.sni(),
                })
            }

            fn early_secret(&mut self) -> Option<Prk> {
                if self.0.emitted_early_secret {
                    return None;
                }

                let value = self.0.session.get_early_secret().cloned()?;
                self.0.emitted_early_secret = true;
                Some(value)
            }

            fn transmit(&mut self, buffer: &mut Vec<u8>) -> Option<SecretPair> {
                self.0
                    .session
                    .write_hs(buffer)
                    .map(|Secrets { client, server }| SecretPair { client, server })
            }
        }
    };
}

impl_tls!(
    RustlsServerEndpoint,
    RustlsServerSession,
    ServerSession,
    ServerConfig,
    new_server
);

impl RustlsServerEndpoint {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}

impl tls::Endpoint for RustlsServerEndpoint {
    type Session = RustlsServerSession;

    fn new_server_session<Params: EncoderValue>(
        &mut self,
        transport_parameters: &Params,
    ) -> Self::Session {
        let len = transport_parameters.encoding_size();
        let mut params_buffer = vec![0; len];
        transport_parameters.encode(&mut EncoderBuffer::new(&mut params_buffer));
        let session = rustls::ServerSession::new_quic(&self.config, params_buffer);
        Self::Session::new(session)
    }

    fn new_client_session<Params: EncoderValue>(
        &mut self,
        _transport_parameters: &Params,
        _sni: &[u8],
    ) -> Self::Session {
        panic!("Client sessions are not supported in server mode");
    }
}

impl RustlsServerSession {
    fn sni(&self) -> Option<&[u8]> {
        self.0.session.get_sni_hostname().map(|sni| sni.as_bytes())
    }
}

impl_tls!(
    RustlsClientEndpoint,
    RustlsClientSession,
    ClientSession,
    ClientConfig,
    new_client
);

impl RustlsClientEndpoint {
    pub fn new(config: ClientConfig) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}

impl tls::Endpoint for RustlsClientEndpoint {
    type Session = RustlsClientSession;

    fn new_server_session<Params: EncoderValue>(
        &mut self,
        _transport_parameters: &Params,
    ) -> Self::Session {
        panic!("Server sessions are not supported in client mode");
    }

    fn new_client_session<Params: EncoderValue>(
        &mut self,
        transport_parameters: &Params,
        sni: &[u8],
    ) -> Self::Session {
        let len = transport_parameters.encoding_size();
        let mut params_buffer = vec![0; len];
        transport_parameters.encode(&mut EncoderBuffer::new(&mut params_buffer));
        let sni = DNSNameRef::try_from_ascii(sni).expect("sni hostname should be valid");
        let session = rustls::ClientSession::new_quic(&self.config, sni, params_buffer);
        Self::Session::new(session)
    }
}

impl RustlsClientSession {
    fn sni(&self) -> Option<&[u8]> {
        None
    }
}

fn tls_error_reason(error: rustls::TLSError) -> &'static str {
    use rustls::TLSError;
    match error {
        TLSError::InappropriateMessage { .. } => "received unexpected message",
        TLSError::InappropriateHandshakeMessage { .. } => "received unexpected handshake message",
        TLSError::CorruptMessage | TLSError::CorruptMessagePayload(_) => "received corrupt message",
        TLSError::NoCertificatesPresented => "peer sent no certificates",
        TLSError::DecryptError => "cannot decrypt peer's message",
        TLSError::PeerIncompatibleError(_) => "peer is incompatible",
        TLSError::PeerMisbehavedError(_) => "peer misbehaved",
        TLSError::AlertReceived(_) => "received fatal alert",
        TLSError::WebPKIError(_) => "invalid certificate",
        TLSError::InvalidSCT(_) => "invalid certificate timestamp",
        TLSError::FailedToGetCurrentTime => "failed to get current time",
        TLSError::HandshakeNotComplete => "handshake not complete",
        TLSError::PeerSentOversizedRecord => "peer sent excess record size",
        TLSError::NoApplicationProtocol => "peer doesn't support any known protocol",
        _ => "unexpected error",
    }
}

#[test]
fn session_size() {
    assert_eq!(core::mem::size_of::<RustlsServerSession>(), 8);
    assert_eq!(core::mem::size_of::<RustlsClientSession>(), 8);
}
