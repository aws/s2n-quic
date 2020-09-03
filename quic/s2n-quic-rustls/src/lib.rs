#![forbid(unsafe_code)]

use core::fmt;
pub use rustls;
use rustls::{
    quic::{ClientQuicExt, QuicExt, Secrets, ServerQuicExt},
    Certificate, ClientConfig, PrivateKey, ProtocolVersion, ServerConfig, SupportedCipherSuite,
    TLSError,
};
use s2n_codec::{EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    self,
    crypto::{tls, CryptoError, CryptoSuite},
};
use s2n_quic_ring::{
    handshake::RingHandshakeCrypto, one_rtt::RingOneRTTCrypto, zero_rtt::RingZeroRTTCrypto, Prk,
    RingCryptoSuite, SecretPair,
};
use std::sync::Arc;
use webpki::DNSNameRef;

// The first 3 ciphers are TLS1.3
// https://github.com/ctz/rustls/blob/1287510bece905b7e45cf31d6e7cf3334b98bb2e/rustls/src/suites.rs#L379
pub fn default_ciphersuites() -> Vec<&'static SupportedCipherSuite> {
    rustls::ALL_CIPHERSUITES.iter().take(3).cloned().collect()
}

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

pub trait AsCertificate {
    fn as_certificate(self) -> Vec<Certificate>;
}

impl AsCertificate for Vec<u8> {
    fn as_certificate(self) -> Vec<Certificate> {
        vec![Certificate(self)]
    }
}

impl AsCertificate for &Vec<u8> {
    fn as_certificate(self) -> Vec<Certificate> {
        vec![Certificate(self.to_vec())]
    }
}

impl AsCertificate for &[u8] {
    fn as_certificate(self) -> Vec<Certificate> {
        vec![Certificate(self.to_vec())]
    }
}

pub trait AsPrivateKey {
    fn as_private_key(self) -> PrivateKey;
}

impl AsPrivateKey for Vec<u8> {
    fn as_private_key(self) -> PrivateKey {
        PrivateKey(self)
    }
}

impl AsPrivateKey for &Vec<u8> {
    fn as_private_key(self) -> PrivateKey {
        PrivateKey(self.to_vec())
    }
}

impl AsPrivateKey for &[u8] {
    fn as_private_key(self) -> PrivateKey {
        PrivateKey(self.to_vec())
    }
}

macro_rules! impl_tls {
    (
        $endpoint:ident,
        $session:ident,
        $rustls_config:ident,
        $rustls_session:ident,
        $new_crypto:ident
    ) => {
        pub struct $endpoint {
            config: Arc<rustls::$rustls_config>,
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
                use rustls::Session;

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
                        if matches!(self.0.phase, HandshakePhase::Application) {
                            context.on_handshake_done()?;
                        }
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
                                let keys = RingHandshakeCrypto::$new_crypto(algorithm, key_pair)
                                    .expect("invalid cipher");
                                self.0.phase = HandshakePhase::Handshake;
                                context.on_handshake_keys(keys)?;
                            }
                            HandshakePhase::Handshake | HandshakePhase::Application => {
                                let keys = RingOneRTTCrypto::$new_crypto(algorithm, key_pair)
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
                            .unwrap_or(CryptoError::INTERNAL_ERROR)
                    })
            }

            fn application_parameters(&self) -> Result<tls::ApplicationParameters, CryptoError> {
                Ok(tls::ApplicationParameters {
                    alpn_protocol: rustls::Session::get_alpn_protocol(&self.0.session),
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

pub mod server {
    use super::*;
    pub use rustls::ServerConfig as Config;

    impl_tls!(Server, Session, ServerConfig, ServerSession, new_server);

    impl Server {
        pub fn new(config: ServerConfig) -> Self {
            Self {
                config: Arc::new(config),
            }
        }

        pub fn builder() -> Builder {
            Builder::new()
        }
    }

    impl Default for Server {
        fn default() -> Self {
            Self::builder()
                .build()
                .expect("could not create a default server")
        }
    }

    pub struct Builder {
        config: ServerConfig,
    }

    impl Default for Builder {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Builder {
        pub fn new() -> Self {
            let mut config = ServerConfig::new(rustls::NoClientAuth::new());

            config.ciphersuites = default_ciphersuites();
            config.versions = PROTOCOL_VERSIONS.to_vec();
            config.ignore_client_order = true;
            config.mtu = None;
            config.alpn_protocols = vec![b"h3".to_vec()];

            Self { config }
        }

        pub fn with_certificate<C: AsCertificate, PK: AsPrivateKey>(
            mut self,
            cert: C,
            key: PK,
        ) -> Result<Self, TLSError> {
            self.config
                .set_single_cert(cert.as_certificate(), key.as_private_key())?;
            Ok(self)
        }

        pub fn with_alpn_protocols<'a, P: Iterator<Item = &'a [u8]>>(
            mut self,
            protocols: P,
        ) -> Result<Self, TLSError> {
            self.config.alpn_protocols = protocols.map(|p| p.to_vec()).collect();
            Ok(self)
        }

        pub fn build(self) -> Result<Server, TLSError> {
            Ok(Server::new(self.config))
        }
    }

    impl tls::Endpoint for Server {
        type Session = Session;

        fn new_server_session<Params: EncoderValue>(&mut self, params: &Params) -> Self::Session {
            let params = encode_transport_params(params);
            let session = rustls::ServerSession::new_quic(&self.config, params);
            Self::Session::new(session)
        }

        fn new_client_session<Params: EncoderValue>(
            &mut self,
            _transport_parameters: &Params,
            _sni: &[u8],
        ) -> Self::Session {
            panic!("cannot create a client session from a server config");
        }
    }

    impl Session {
        fn sni(&self) -> Option<&[u8]> {
            self.0.session.get_sni_hostname().map(|sni| sni.as_bytes())
        }
    }
}

pub mod client {
    use super::*;
    pub use rustls::ClientConfig as Config;

    impl_tls!(Client, Session, ClientConfig, ClientSession, new_client);

    impl Client {
        pub fn new(config: ClientConfig) -> Self {
            Self {
                config: Arc::new(config),
            }
        }

        pub fn builder() -> Builder {
            Builder::new()
        }
    }

    impl Default for Client {
        fn default() -> Self {
            Self::builder()
                .build()
                .expect("could not create a default client")
        }
    }

    impl tls::Endpoint for Client {
        type Session = Session;

        fn new_server_session<Params: EncoderValue>(
            &mut self,
            _transport_parameters: &Params,
        ) -> Self::Session {
            panic!("cannot create a server session from a client config");
        }

        fn new_client_session<Params: EncoderValue>(
            &mut self,
            params: &Params,
            sni: &[u8],
        ) -> Self::Session {
            let params = encode_transport_params(params);
            let sni = DNSNameRef::try_from_ascii(sni).expect("sni hostname should be valid");
            let session = rustls::ClientSession::new_quic(&self.config, sni, params);
            Self::Session::new(session)
        }
    }

    pub struct Builder {
        config: ClientConfig,
    }

    impl Default for Builder {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Builder {
        pub fn new() -> Self {
            let mut config = ClientConfig::new();

            config.ciphersuites = default_ciphersuites();
            config.versions = PROTOCOL_VERSIONS.to_vec();
            config.mtu = None;
            config.alpn_protocols = vec![b"h3".to_vec()];

            Self { config }
        }

        pub fn with_alpn_protocols<'a, P: Iterator<Item = &'a [u8]>>(
            mut self,
            protocols: P,
        ) -> Result<Self, TLSError> {
            self.config.alpn_protocols = protocols.map(|p| p.to_vec()).collect();
            Ok(self)
        }

        pub fn build(self) -> Result<Client, TLSError> {
            Ok(Client::new(self.config))
        }
    }

    impl Session {
        fn sni(&self) -> Option<&[u8]> {
            None
        }
    }
}

pub use client::Client;
pub use server::Server;

fn tls_error_reason(error: TLSError) -> &'static str {
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

fn encode_transport_params<Params: EncoderValue>(params: &Params) -> Vec<u8> {
    let len = params.encoding_size();
    let mut buffer = vec![0; len];
    params.encode(&mut EncoderBuffer::new(&mut buffer));
    buffer
}

#[test]
fn session_size() {
    assert_eq!(core::mem::size_of::<server::Session>(), 8);
    assert_eq!(core::mem::size_of::<client::Session>(), 8);
}
