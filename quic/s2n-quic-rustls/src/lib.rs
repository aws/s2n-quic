// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use core::fmt;
pub use rustls::{self, Certificate, PrivateKey};
use rustls::{
    quic::{ClientQuicExt, QuicExt, Secrets, ServerQuicExt},
    ClientConfig, ProtocolVersion, ServerConfig, SupportedCipherSuite, TLSError,
};
use s2n_codec::{EncoderBuffer, EncoderValue};
use s2n_quic_core::{
    self,
    crypto::{tls, CryptoError, CryptoSuite},
    transport::error::TransportError,
};
use s2n_quic_ring::{
    handshake::RingHandshakeKey, one_rtt::RingOneRttKey, zero_rtt::RingZeroRttKey, Prk,
    RingCryptoSuite, SecretPair,
};
use std::sync::Arc;
use webpki::DNSNameRef;

pub mod certificate;

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#5.3
//# A cipher suite MUST NOT be
//# negotiated unless a header protection scheme is defined for the
//# cipher suite.
// All of the ciphersuites from the current exported list have HP schemes for QUIC

// The first 3 ciphers are TLS1.3
// https://github.com/ctz/rustls/blob/1287510bece905b7e45cf31d6e7cf3334b98bb2e/rustls/src/suites.rs#L379
pub fn default_ciphersuites() -> Vec<&'static SupportedCipherSuite> {
    rustls::ALL_CIPHERSUITES.iter().take(3).cloned().collect()
}

//= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.2
//# Clients MUST NOT offer TLS versions older than 1.3.
pub static PROTOCOL_VERSIONS: [ProtocolVersion; 1] = [ProtocolVersion::TLSv1_3];

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
enum HandshakePhase {
    Initial,
    Handshake,
    Application,
}

impl HandshakePhase {
    fn transition(&mut self) {
        *self = match self {
            Self::Initial => Self::Handshake,
            _ => Self::Application,
        };
    }
}

impl Default for HandshakePhase {
    fn default() -> Self {
        Self::Initial
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct InnerSession<Session> {
    session: Session,
    rx_phase: HandshakePhase,
    tx_phase: HandshakePhase,
    emitted_early_secret: bool,
    emitted_handshake_done: bool,
}

impl<Session> InnerSession<Session> {
    fn new(session: Session) -> Self {
        Self {
            session,
            tx_phase: Default::default(),
            rx_phase: Default::default(),
            emitted_early_secret: false,
            emitted_handshake_done: false,
        }
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

        impl tls::Session for $session {
            fn poll<W: tls::Context<Self>>(
                &mut self,
                context: &mut W,
            ) -> Result<(), TransportError> {
                use rustls::Session;

                // Tracks if we have attempted to receive data at least once
                let mut has_tried_receive = false;

                loop {
                    let crypto_data = match self.0.rx_phase {
                        HandshakePhase::Initial => context.receive_initial(None),
                        HandshakePhase::Handshake => context.receive_handshake(None),
                        HandshakePhase::Application => context.receive_application(None),
                    };

                    // receive anything in the incoming buffer
                    if let Some(crypto_data) = crypto_data {
                        self.receive(&crypto_data)?;
                    } else if has_tried_receive {
                        // If there's nothing to receive then we're done for now
                        return Ok(());
                    }

                    // mark that we tried to receive some data so we know next time we loop
                    // to bail if nothing changed
                    has_tried_receive = true;

                    // we're done with the handshake!
                    if self.0.tx_phase == HandshakePhase::Application
                        && !self.0.session.is_handshaking()
                    {
                        if !self.0.emitted_handshake_done {
                            self.0.rx_phase.transition();
                            context.on_handshake_done()?;
                        }

                        self.0.emitted_handshake_done = true;
                        return Ok(());
                    }

                    // try to pull out the early secrets, if any
                    if let Some(early_secret) = self.early_secret() {
                        let (key, header_key) = RingZeroRttKey::new(early_secret);
                        context.on_zero_rtt_keys(
                            key,
                            header_key,
                            self.application_parameters()?,
                        )?;
                    }

                    loop {
                        // make sure we can send data before pulling it out of rustls
                        let can_send = match self.0.tx_phase {
                            HandshakePhase::Initial => context.can_send_initial(),
                            HandshakePhase::Handshake => context.can_send_handshake(),
                            HandshakePhase::Application => context.can_send_application(),
                        };

                        if !can_send {
                            break;
                        }

                        let mut transmission_buffer = vec![];

                        let key_upgrade = self.transmit(&mut transmission_buffer);

                        // if we didn't upgrade the key or transmit anything then we're waiting for
                        // more reads
                        if key_upgrade.is_none() && transmission_buffer.is_empty() {
                            break;
                        }

                        // fill the correct buffer according to the handshake phase
                        match self.0.tx_phase {
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

                        // we got new TLS keys!
                        if let Some(key_pair) = key_upgrade {
                            let algorithm = self
                                .0
                                .session
                                .get_negotiated_ciphersuite()
                                .expect("ciphersuite should be available")
                                .get_aead_alg();

                            match self.0.tx_phase {
                                HandshakePhase::Initial => {
                                    let (key, header_key) =
                                        RingHandshakeKey::$new_crypto(algorithm, key_pair)
                                            .expect("invalid cipher");

                                    context.on_handshake_keys(key, header_key)?;

                                    // Transition both phases to Handshake
                                    self.0.tx_phase.transition();
                                    self.0.rx_phase.transition();
                                }
                                _ => {
                                    let (key, header_key) =
                                        RingOneRttKey::$new_crypto(algorithm, key_pair.clone())
                                            .expect("invalid cipher");
                                    context.on_one_rtt_keys(
                                        key,
                                        header_key,
                                        self.application_parameters()?,
                                    )?;

                                    // Transition the tx_phase to Application
                                    // Note: the rx_phase is transitioned when the handshake is done
                                    self.0.tx_phase.transition();
                                }
                            }
                        }
                    }
                }
            }
        }

        impl $session {
            fn receive(&mut self, crypto_data: &[u8]) -> Result<(), TransportError> {
                self.0
                    .session
                    .read_hs(crypto_data)
                    .map_err(tls_error_reason)
                    .map_err(|reason| {
                        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.8
                        //# The alert level of all TLS alerts is "fatal"; a TLS stack MUST NOT
                        //# generate alerts at the "warning" level.

                        // According to the rustls docs, `get_alert` only returns fatal alerts:
                        // > https://docs.rs/rustls/0.19.0/rustls/quic/trait.QuicExt.html#tymethod.get_alert
                        // > Emit the TLS description code of a fatal alert, if one has arisen.

                        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.8
                        //= type=TODO
                        //= tracking-issue=304
                        //# Endpoints MAY use a generic error
                        //# code to avoid possibly exposing confidential information.

                        self.0
                            .session
                            .get_alert()
                            .map(|alert| CryptoError {
                                code: alert.get_u8(),
                                reason,
                            })
                            .unwrap_or(CryptoError::INTERNAL_ERROR)
                    })?;
                Ok(())
            }

            fn application_parameters(&self) -> Result<tls::ApplicationParameters, TransportError> {
                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#8.1
                //# Unless
                //# another mechanism is used for agreeing on an application protocol,
                //# endpoints MUST use ALPN for this purpose.
                let alpn_protocol = rustls::Session::get_alpn_protocol(&self.0.session).ok_or(
                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#8.1
                    //# When using ALPN, endpoints MUST immediately close a connection (see
                    //# Section 10.2 of [QUIC-TRANSPORT]) with a no_application_protocol TLS
                    //# alert (QUIC error code 0x178; see Section 4.8) if an application
                    //# protocol is not negotiated.

                    //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#8.1
                    //# While [ALPN] only specifies that servers
                    //# use this alert, QUIC clients MUST use error 0x178 to terminate a
                    //# connection when ALPN negotiation fails.
                    CryptoError::NO_APPLICATION_PROTOCOL.with_reason("Missing ALPN protocol"),
                )?;

                //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#8.2
                //# endpoints that
                //# receive ClientHello or EncryptedExtensions messages without the
                //# quic_transport_parameters extension MUST close the connection with an
                //# error of type 0x16d (equivalent to a fatal TLS missing_extension
                //# alert, see Section 4.8).

                let transport_parameters = self.0.session.get_quic_transport_parameters().ok_or(
                    CryptoError::MISSING_EXTENSION.with_reason("Missing QUIC transport parameters"),
                )?;

                let sni = self.sni();

                Ok(tls::ApplicationParameters {
                    alpn_protocol,
                    transport_parameters,
                    sni,
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

        pub fn with_certificate<
            C: certificate::IntoCertificate,
            PK: certificate::IntoPrivateKey,
        >(
            mut self,
            certificate: C,
            private_key: PK,
        ) -> Result<Self, TLSError> {
            let certificate = certificate.into_certificate()?;
            let private_key = private_key.into_private_key()?;
            self.config.set_single_cert(certificate.0, private_key.0)?;
            Ok(self)
        }

        pub fn with_alpn_protocols<P: Iterator<Item = I>, I: AsRef<[u8]>>(
            mut self,
            protocols: P,
        ) -> Result<Self, TLSError> {
            self.config.alpn_protocols = protocols.map(|p| p.as_ref().to_vec()).collect();
            Ok(self)
        }

        pub fn with_key_logging(mut self) -> Result<Self, TLSError> {
            self.config.key_log = Arc::new(rustls::KeyLogFile::new());
            Ok(self)
        }

        pub fn build(self) -> Result<Server, TLSError> {
            Ok(Server::new(self.config))
        }
    }

    impl tls::Endpoint for Server {
        type Session = Session;

        fn new_server_session<Params: EncoderValue>(&mut self, params: &Params) -> Self::Session {
            //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#8.2
            //# Endpoints
            //# MUST send the quic_transport_parameters extension;
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

        fn max_tag_length(&self) -> usize {
            s2n_quic_ring::MAX_TAG_LEN
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

        fn max_tag_length(&self) -> usize {
            s2n_quic_ring::MAX_TAG_LEN
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

        pub fn with_certificate<C: certificate::IntoCertificate>(
            mut self,
            cert: C,
        ) -> Result<Self, TLSError> {
            let certificates = cert.into_certificate()?;
            let root_certificate = certificates.0.get(0).ok_or_else(|| {
                TLSError::General("Certificate chain needs to have at least one entry".to_string())
            })?;
            self.config
                .root_store
                .add(&root_certificate)
                .map_err(|err| TLSError::General(err.to_string()))?;
            Ok(self)
        }

        pub fn with_key_logging(mut self) -> Result<Self, TLSError> {
            self.config.key_log = Arc::new(rustls::KeyLogFile::new());
            Ok(self)
        }

        pub fn build(self) -> Result<Client, TLSError> {
            Ok(Client::new(self.config))
        }
    }

    impl Session {
        fn sni(&self) -> Option<&[u8]> {
            // TODO return the specified sni value
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
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.2
        //# An endpoint MUST terminate the connection if a
        //# version of TLS older than 1.3 is negotiated.
        TLSError::PeerIncompatibleError(_) => "peer is incompatible",
        TLSError::PeerMisbehavedError(_) => "peer misbehaved",
        TLSError::AlertReceived(_) => "received fatal alert",
        //= https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#4.4
        //# A client MUST authenticate the identity of the server.
        TLSError::WebPKIError(_) => "invalid certificate",
        TLSError::InvalidSCT(_) => "invalid certificate timestamp",
        TLSError::FailedToGetCurrentTime => "failed to get current time",
        TLSError::HandshakeNotComplete => "handshake not complete",
        TLSError::PeerSentOversizedRecord => "peer sent excess record size",
        TLSError::NoApplicationProtocol => "peer doesn't support any known protocol",
        TLSError::General(msg) => {
            // rustls doesn't provide a specialized variant for these so we'll need to match on the `String` itself
            // and try and map it to a `&'static str`
            match msg.as_str() {
                "no server certificate chain resolved" => "no server certificate chain resolved",
                _ => "unexpected error",
            }
        }
        // rustls may add a new variant in the future that breaks us so do a wildcard
        #[allow(unreachable_patterns)]
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

#[test]
fn client_server_test() {
    use s2n_quic_core::crypto::tls::testing::certificates::*;

    let mut client = client::Builder::new()
        .with_certificate(CERT_PEM)
        .unwrap()
        .build()
        .unwrap();

    let mut server = server::Builder::new()
        .with_certificate(CERT_PEM, KEY_PEM)
        .unwrap()
        .build()
        .unwrap();

    let mut pair = tls::testing::Pair::new(&mut server, &mut client, b"localhost");

    while pair.is_handshaking() {
        pair.poll().unwrap();
    }

    pair.finish();
}
