// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(feature = "alloc")]
pub use bytes::{Bytes, BytesMut};
use core::{convert::TryFrom, fmt::Debug};
use zerocopy::{AsBytes, FromBytes, FromZeroes, Unaligned};

#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[cfg(all(feature = "alloc", any(test, feature = "testing")))]
pub mod null;

/// Holds all application parameters which are exchanged within the TLS handshake.
#[derive(Debug)]
pub struct ApplicationParameters<'a> {
    /// Encoded transport parameters
    pub transport_parameters: &'a [u8],
}

#[derive(Debug)]
#[non_exhaustive]
pub enum TlsExportError {
    #[non_exhaustive]
    Failure,
}

impl TlsExportError {
    pub fn failure() -> Self {
        TlsExportError::Failure
    }
}

pub trait TlsSession: Send {
    /// See <https://datatracker.ietf.org/doc/html/rfc5705> and <https://www.rfc-editor.org/rfc/rfc8446>.
    fn tls_exporter(
        &self,
        label: &[u8],
        context: &[u8],
        output: &mut [u8],
    ) -> Result<(), TlsExportError>;

    fn cipher_suite(&self) -> CipherSuite;
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

    fn on_tls_exporter_ready(
        &mut self,
        session: &impl TlsSession,
    ) -> Result<(), crate::transport::Error>;

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

    fn process_post_handshake_message<C: Context<Self>>(
        &mut self,
        context: &mut C,
    ) -> Result<(), crate::transport::Error>;

    fn discard_session(&self, received_ticket: bool) -> bool;

    /// Parses a hello message of the provided type
    ///
    /// The default implementation of this function assumes TLS messages are being exchanged.
    #[inline]
    fn parse_hello(
        msg_type: HandshakeType,
        header_chunk: &[u8],
        total_received_len: u64,
        max_hello_size: u64,
    ) -> Result<Option<HelloOffsets>, crate::transport::Error> {
        let buffer = s2n_codec::DecoderBuffer::new(header_chunk);

        let header = if let Ok((header, _)) = buffer.decode::<HandshakeHeader>() {
            header
        } else {
            // we don't have enough data to parse the header so wait until later
            return Ok(None);
        };

        if header.msg_type() != Some(msg_type) {
            return Err(crate::transport::Error::PROTOCOL_VIOLATION
                .with_reason("first TLS message should be a hello message"));
        }

        let payload_len = header.len() as u64;

        if payload_len > max_hello_size {
            return Err(crate::transport::Error::CRYPTO_BUFFER_EXCEEDED
                .with_reason("hello message cannot exceed 16k"));
        }

        let header_len = core::mem::size_of::<HandshakeHeader>() as u64;

        // wait until we have more chunks
        if total_received_len < payload_len + header_len {
            return Ok(None);
        }

        let offsets = HelloOffsets {
            payload_offset: header_len as _,
            payload_len: payload_len as _,
        };

        Ok(Some(offsets))
    }
}

#[derive(Copy, Clone, Debug)]
pub struct HelloOffsets {
    pub payload_offset: usize,
    pub payload_len: usize,
}

impl HelloOffsets {
    #[inline]
    pub fn trim_chunks<'a, I: Iterator<Item = &'a [u8]>>(
        &self,
        chunks: I,
    ) -> impl Iterator<Item = &'a [u8]> {
        let mut offsets = *self;

        chunks.filter_map(move |mut chunk| {
            // trim off the header
            if offsets.payload_offset > 0 {
                let start = offsets.payload_offset.min(chunk.len());
                chunk = &chunk[start..];
                offsets.payload_offset -= start;
            }

            // trim off any trailing data after we've trimmed the header
            if offsets.payload_offset == 0 && offsets.payload_len > 0 {
                let end = offsets.payload_len.min(chunk.len());
                chunk = &chunk[..end];
                offsets.payload_len -= end;
            } else {
                // if the payload doesn't have any remaining data, return an empty chunk
                return None;
            }

            if chunk.is_empty() {
                None
            } else {
                Some(chunk)
            }
        })
    }
}

#[derive(Copy, Clone, Debug, Default)]
#[allow(non_camel_case_types)]
pub enum CipherSuite {
    TLS_AES_128_GCM_SHA256,
    TLS_AES_256_GCM_SHA384,
    TLS_CHACHA20_POLY1305_SHA256,
    #[default]
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
        #[derive(Clone, Copy, Debug, PartialEq, Eq, AsBytes, Unaligned)]
        #[cfg_attr(any(test, feature = "bolero-generator"), derive(bolero_generator::TypeGenerator))]
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
#[derive(Clone, Copy, Debug, AsBytes, FromBytes, FromZeroes, Unaligned)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use bolero::check;
    use hex_literal::hex;

    const MAX_HELLO_SIZE: u64 = if cfg!(kani) { 32 } else { 255 };

    type Chunk = crate::testing::InlineVec<u8, { MAX_HELLO_SIZE as usize + 2 }>;

    /// make sure the hello parser doesn't panic on arbitrary inputs
    #[test]
    #[cfg_attr(kani, kani::proof, kani::solver(cadical), kani::unwind(36))]
    fn parse_hello_test() {
        check!()
            .with_type::<(HandshakeType, Chunk, u64)>()
            .for_each(|(ty, chunk, total_received_len)| {
                let _ =
                    testing::Session::parse_hello(*ty, chunk, *total_received_len, MAX_HELLO_SIZE);
            });
    }

    macro_rules! h {
        ($($tt:tt)*) => {
            &hex!($($tt)*)[..]
        }
    }

    fn parse_hello<'a>(
        ty: HandshakeType,
        input: &'a [&'a [u8]],
    ) -> Result<Option<Vec<&'a [u8]>>, crate::transport::Error> {
        let total_received_len: usize = input.iter().map(|chunk| chunk.len()).sum();

        let empty = &[][..];
        let first = input.iter().copied().next().unwrap_or(empty);

        let outcome =
            testing::Session::parse_hello(ty, first, total_received_len as _, MAX_HELLO_SIZE)?;

        if let Some(offsets) = outcome {
            let payload = offsets.trim_chunks(input.iter().copied()).collect();
            Ok(Some(payload))
        } else {
            Ok(None)
        }
    }

    #[test]
    fn client_hello_valid_tests() {
        let tests = [
            (&[h!("01 00 00 02 aa bb cc")][..], &[h!("aa bb")][..]),
            (&[h!("01 00 00 01"), h!("aa bb cc dd")], &[h!("aa")]),
            (
                &[h!("01 00 00 02"), h!("aa"), h!("bb"), h!("cc")],
                &[h!("aa"), h!("bb")],
            ),
        ];

        for (input, expected) in tests {
            let output = parse_hello(HandshakeType::ClientHello, input)
                .unwrap()
                .unwrap();

            assert_eq!(&output[..], expected);
        }
    }

    #[test]
    fn server_hello_valid_tests() {
        let tests = [(&[h!("02 00 00 02 aa bb cc")][..], &[h!("aa bb")][..])];

        for (input, expected) in tests {
            let output = parse_hello(HandshakeType::ServerHello, input)
                .unwrap()
                .unwrap();

            assert_eq!(&output[..], expected);
        }
    }

    #[test]
    fn client_hello_incomplete_tests() {
        let tests = [
            &[][..],
            // missing header
            &[h!("01 00 00")],
            // missing entire payload
            &[h!("01 00 00 01")],
            // missing partial payload
            &[h!("01 00 00 04"), h!("aa"), h!("bb")],
        ];

        for input in tests {
            assert_eq!(
                parse_hello(HandshakeType::ClientHello, input).unwrap(),
                None
            );
        }
    }

    #[test]
    fn client_hello_invalid_tests() {
        let tests = [
            // invalid message
            &[h!("02 00 00 01 aa")],
            // invalid size - too big
            &[h!("01 00 01 00 aa")],
            // invalid size - too big
            &[h!("01 ff ff ff aa")],
        ];

        for input in tests {
            assert!(parse_hello(HandshakeType::ClientHello, input).is_err());
        }
    }
}
