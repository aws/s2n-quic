// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "alloc")]
pub use bytes::{Bytes, BytesMut};
use core::{convert::TryFrom, fmt::Debug};
use zerocopy::{AsBytes, FromBytes, Unaligned};

#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// Holds all application parameters which are exchanged within the TLS handshake.
#[derive(Debug)]
pub struct ApplicationParameters<'a> {
    /// Encoded transport parameters
    pub transport_parameters: &'a [u8],
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-4
//= type=TODO
//= tracking-issue=332
//# To avoid excessive buffering at multiple layers, QUIC implementations
//# SHOULD provide an interface for the cryptographic protocol
//# implementation to communicate its buffering limits.
#[cfg(feature = "alloc")]
pub trait Context<Crypto: crate::crypto::CryptoSuite> {
    fn on_handshake_keys(
        &mut self,
        key: Crypto::HandshakeKey,
        header_key: Crypto::HandshakeHeaderKey,
    ) -> Result<(), crate::transport::Error>;

    fn on_zero_rtt_keys(
        &mut self,
        key: Crypto::ZeroRttKey,
        header_key: Crypto::ZeroRttHeaderKey,
        application_parameters: ApplicationParameters,
    ) -> Result<(), crate::transport::Error>;

    fn on_one_rtt_keys(
        &mut self,
        key: Crypto::OneRttKey,
        header_key: Crypto::OneRttHeaderKey,
        application_parameters: ApplicationParameters,
    ) -> Result<(), crate::transport::Error>;

    fn on_server_name(
        &mut self,
        server_name: crate::application::ServerName,
    ) -> Result<(), crate::transport::Error>;

    fn on_application_protocol(
        &mut self,
        application_protocol: Bytes,
    ) -> Result<(), crate::transport::Error>;

    //= https://www.rfc-editor.org/rfc/rfc9001#section-4.1.1
    //# The TLS handshake is considered complete when the
    //# TLS stack has reported that the handshake is complete.  This happens
    //# when the TLS stack has both sent a Finished message and verified the
    //# peer's Finished message.
    fn on_handshake_complete(&mut self) -> Result<(), crate::transport::Error>;

    /// Receives data from the initial packet space
    ///
    /// A `max_len` may be provided to indicate how many bytes the TLS implementation
    /// is willing to buffer.
    fn receive_initial(&mut self, max_len: Option<usize>) -> Option<Bytes>;

    /// Receives data from the handshake packet space
    ///
    /// A `max_len` may be provided to indicate how many bytes the TLS implementation
    /// is willing to buffer.
    fn receive_handshake(&mut self, max_len: Option<usize>) -> Option<Bytes>;

    /// Receives data from the application packet space
    ///
    /// A `max_len` may be provided to indicate how many bytes the TLS implementation
    /// is willing to buffer.
    fn receive_application(&mut self, max_len: Option<usize>) -> Option<Bytes>;

    fn can_send_initial(&self) -> bool;
    fn send_initial(&mut self, transmission: Bytes);

    fn can_send_handshake(&self) -> bool;
    fn send_handshake(&mut self, transmission: Bytes);

    fn can_send_application(&self) -> bool;
    fn send_application(&mut self, transmission: Bytes);

    fn waker(&self) -> &core::task::Waker;
}

#[cfg(feature = "alloc")]
pub trait Endpoint: 'static + Sized + Send {
    type Session: Session;

    fn new_server_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
    ) -> Self::Session;

    fn new_client_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
        server_name: crate::application::ServerName,
    ) -> Self::Session;

    /// The maximum length of a tag for any algorithm that may be negotiated
    fn max_tag_length(&self) -> usize;
}

#[cfg(feature = "alloc")]
pub trait Session: crate::crypto::CryptoSuite + Sized + Send + Debug {
    fn poll<C: Context<Self>>(
        &mut self,
        context: &mut C,
    ) -> core::task::Poll<Result<(), crate::transport::Error>>;
}

#[derive(Copy, Clone, Debug)]
#[allow(non_camel_case_types)]
pub enum CipherSuite {
    TLS_AES_128_GCM_SHA256,
    TLS_AES_256_GCM_SHA384,
    TLS_CHACHA20_POLY1305_SHA256,
    Unknown,
}

impl crate::event::IntoEvent<crate::event::builder::CipherSuite> for CipherSuite {
    #[inline]
    fn into_event(self) -> crate::event::builder::CipherSuite {
        use crate::event::builder::CipherSuite::*;
        match self {
            Self::TLS_AES_128_GCM_SHA256 => TLS_AES_128_GCM_SHA256 {},
            Self::TLS_AES_256_GCM_SHA384 => TLS_AES_256_GCM_SHA384 {},
            Self::TLS_CHACHA20_POLY1305_SHA256 => TLS_CHACHA20_POLY1305_SHA256 {},
            Self::Unknown => Unknown {},
        }
    }
}

impl crate::event::IntoEvent<crate::event::api::CipherSuite> for CipherSuite {
    #[inline]
    fn into_event(self) -> crate::event::api::CipherSuite {
        let builder: crate::event::builder::CipherSuite = self.into_event();
        builder.into_event()
    }
}

macro_rules! handshake_type {
    ($($variant:ident($value:literal)),* $(,)?) => {
        #[derive(Debug, PartialEq, Eq, AsBytes, Unaligned)]
        #[repr(u8)]
        pub enum HandshakeType {
            $($variant = $value),*
        }

        impl TryFrom<u8> for HandshakeType {
            type Error = ();

            #[inline]
            fn try_from(value: u8) -> Result<Self, Self::Error> {
                match value {
                    $($value => Ok(Self::$variant),)*
                    _ => Err(()),
                }
            }
        }
    };
}

//= https://www.rfc-editor.org/rfc/rfc5246#A.4
//# enum {
//#     hello_request(0), client_hello(1), server_hello(2),
//#     certificate(11), server_key_exchange (12),
//#     certificate_request(13), server_hello_done(14),
//#     certificate_verify(15), client_key_exchange(16),
//#     finished(20)
//#     (255)
//# } HandshakeType;
handshake_type!(
    HelloRequest(0),
    ClientHello(1),
    ServerHello(2),
    Certificate(11),
    ServerKeyExchange(12),
    CertificateRequest(13),
    ServerHelloDone(14),
    CertificateVerify(15),
    ClientKeyExchange(16),
    Finished(20),
);

//= https://www.rfc-editor.org/rfc/rfc5246#A.4
//# struct {
//#     HandshakeType msg_type;
//#     uint24 length;
//#     select (HandshakeType) {
//#         case hello_request:       HelloRequest;
//#         case client_hello:        ClientHello;
//#         case server_hello:        ServerHello;
//#         case certificate:         Certificate;
//#         case server_key_exchange: ServerKeyExchange;
//#         case certificate_request: CertificateRequest;
//#         case server_hello_done:   ServerHelloDone;
//#         case certificate_verify:  CertificateVerify;
//#         case client_key_exchange: ClientKeyExchange;
//#         case finished:            Finished;
//#   } body;
//# } Handshake;
#[derive(Clone, Copy, Debug, AsBytes, FromBytes, Unaligned)]
#[repr(C)]
pub struct HandshakeHeader {
    msg_type: u8,
    length: [u8; 3],
}

impl HandshakeHeader {
    #[inline]
    pub fn msg_type(self) -> Option<HandshakeType> {
        HandshakeType::try_from(self.msg_type).ok()
    }

    #[inline]
    pub fn len(self) -> usize {
        let mut len = [0u8; 4];
        len[1..].copy_from_slice(&self.length);
        let len = u32::from_be_bytes(len);
        len as _
    }

    #[inline]
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }
}

s2n_codec::zerocopy_value_codec!(HandshakeHeader);
