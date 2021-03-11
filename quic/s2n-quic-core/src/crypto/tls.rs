// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{crypto::CryptoSuite, transport::error::TransportError};
pub use bytes::{Bytes, BytesMut};
use core::fmt::Debug;
use s2n_codec::EncoderValue;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

/// Holds all application parameters which are exchanged within the TLS handshake.
#[derive(Debug)]
pub struct ApplicationParameters<'a> {
    /// The negotiated Application Layer Protocol
    pub alpn_protocol: &'a [u8],
    /// Server Name Indication
    pub sni: Option<&'a [u8]>,
    /// Encoded transport parameters
    pub transport_parameters: &'a [u8],
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#4
//= type=TODO
//= tracking-issue=332
//# To avoid excessive buffering at multiple layers, QUIC implementations
//# SHOULD provide an interface for the cryptographic protocol
//# implementation to communicate its buffering limits.

pub trait Context<Crypto: CryptoSuite> {
    fn on_handshake_keys(&mut self, keys: Crypto::HandshakeCrypto) -> Result<(), TransportError>;

    fn on_zero_rtt_keys(
        &mut self,
        keys: Crypto::ZeroRTTCrypto,
        application_parameters: ApplicationParameters,
    ) -> Result<(), TransportError>;

    fn on_one_rtt_keys(
        &mut self,
        keys: Crypto::OneRTTCrypto,
        application_parameters: ApplicationParameters,
    ) -> Result<(), TransportError>;

    fn on_handshake_done(&mut self) -> Result<(), TransportError>;

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
}

pub trait Endpoint: 'static + Sized {
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

    /// The maximum length of a tag for any algorithm that may be negotiated
    fn max_tag_length(&self) -> usize;
}

pub trait Session: CryptoSuite + Sized + Send + Debug {
    fn poll<C: Context<Self>>(&mut self, context: &mut C) -> Result<(), TransportError>;
}
