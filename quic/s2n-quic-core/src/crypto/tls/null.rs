// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Minimal endpoint that doesn't perform any handshakes or encryption
//!
//! NOTE: this should only be used for internal testing
//!
//! The main purpose of this endpoint type is to identify bottlenecks in the code _outside_ of
//! crypto, since that's a large portion of the cycles we use. This has similar goals to the
//! proposal in https://datatracker.ietf.org/doc/html/draft-banks-quic-disable-encryption-00, but
//! instead reduces the handshake to simply exchanging transport parameters.

use crate::{
    application::{server_name::LOCALHOST, ServerName},
    crypto::{self, tls},
    transport,
};
use bytes::Bytes;
use core::{mem::size_of, task::Poll};

#[derive(Debug)]
pub struct Endpoint(());

impl Default for Endpoint {
    #[track_caller]
    fn default() -> Self {
        #[cfg(feature = "std")]
        {
            static WARNING: &str = r"
                             ▒▒████
                             ████████
                           ██████████
                           ████▒▒██████
                         ██████    ████▒▒
                         ████      ▒▒████
                       ██████        ██████
                     ▒▒████    ████    ████
                     ████▒▒  ████████  ██████
                   ██████    ████████    ████
                   ████░░    ████████    ██████
                 ██████      ████████      ████▒▒
               ░░████        ████████      ▒▒████
               ██████        ████████        ██████
             ▒▒████          ████████          ████
             ████▒▒          ██████▒▒          ██████
           ██████              ████              ████
           ████                ████              ██████
         ██████                ████                ████▒▒
       ░░████                                      ▒▒████
       ████▓▓                                        ██████
     ▒▒████                    ████                    ████
     ████▒▒                  ████████                  ██████
   ██████                      ▒▒▒▒                      ████░░
   ████                                                  ▒▒████
 ██████  ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░  ░░░░░░░░░░░░░░░░▒▒██████
 ████████████████████████████████████████████████████████████████
 ▓▓████████████████████████████████████████████████████████████▓▓
            ";
            eprintln!("{}", WARNING);
            eprintln!();
            eprintln!("                  =====  W A R N I N G !!! =====");
            eprintln!();
            eprintln!("  An s2n-quic endpoint has configured without cryptographic protections.");
            eprintln!("  This should ONLY be used for testing purposed only.");
            eprintln!();
            let location = core::panic::Location::caller();
            eprintln!("  Endpoint configured by: {}", location);
            eprintln!();
            eprintln!("                  ==============================");
        }

        Self(())
    }
}

impl crypto::tls::Endpoint for Endpoint {
    type Session = Session;

    #[inline]
    fn new_server_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
    ) -> Self::Session {
        let params = encode_transport_parameters(transport_parameters);
        Session::Server(server::Session::Init {
            transport_parameters: params,
        })
    }

    #[inline]
    fn new_client_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
        server_name: ServerName,
    ) -> Self::Session {
        assert_eq!(server_name, LOCALHOST);

        let params = encode_transport_parameters(transport_parameters);
        Session::Client(client::Session::Init {
            transport_parameters: params,
        })
    }

    #[inline]
    fn max_tag_length(&self) -> usize {
        0
    }
}

#[derive(Debug)]
pub enum Session {
    Client(client::Session),
    Server(server::Session),
}

impl crypto::CryptoSuite for Session {
    type HandshakeKey = key::NoCrypto;
    type HandshakeHeaderKey = key::NoCrypto;
    type InitialKey = key::NoCrypto;
    type InitialHeaderKey = key::NoCrypto;
    type OneRttKey = key::NoCrypto;
    type OneRttHeaderKey = key::NoCrypto;
    type ZeroRttKey = key::NoCrypto;
    type ZeroRttHeaderKey = key::NoCrypto;
    type RetryKey = key::NoCrypto;
}

impl tls::Session for Session {
    #[inline]
    fn poll<C: tls::Context<Self>>(
        &mut self,
        context: &mut C,
    ) -> Poll<Result<(), transport::Error>> {
        match self {
            Self::Client(session) => session.poll(context),
            Self::Server(session) => session.poll(context),
        }
    }

    #[inline]
    fn parse_hello(
        _msg_type: tls::HandshakeType,
        _header_chunk: &[u8],
        _total_received_len: u64,
        _max_hello_size: u64,
    ) -> Result<Option<tls::HelloOffsets>, crate::transport::Error> {
        let offsets = tls::HelloOffsets {
            payload_offset: 0,
            payload_len: 0,
        };
        Ok(Some(offsets))
    }
}

/// Encodes transport parameters into a byte vec
fn encode_transport_parameters<Params: s2n_codec::EncoderValue>(params: &Params) -> Bytes {
    let len = params.encoding_size();
    let mut buffer = vec![0; len];
    params.encode(&mut s2n_codec::EncoderBuffer::new(&mut buffer));
    buffer.into()
}

static FIN: Bytes = Bytes::from_static(b"FIN");
static NULL: Bytes = Bytes::from_static(b"NULL");

pub mod client {
    use super::*;

    #[derive(Debug)]
    pub enum Session {
        Init { transport_parameters: Bytes },
        WaitingInitial {},
        WaitingHandshake { params: Bytes },
        Complete,
    }

    impl Session {
        #[inline]
        pub fn poll<C: tls::Context<super::Session>>(
            &mut self,
            context: &mut C,
        ) -> Poll<Result<(), transport::Error>> {
            loop {
                match self {
                    Self::Init {
                        ref mut transport_parameters,
                    } => {
                        context.send_initial(core::mem::take(transport_parameters));

                        context.on_server_name(LOCALHOST.clone()).unwrap();

                        *self = Self::WaitingInitial {};
                    }
                    Self::WaitingInitial {} => {
                        let params = match context.receive_initial(None) {
                            Some(bytes) => bytes,
                            None => return Poll::Pending,
                        };

                        context
                            .on_handshake_keys(key::NoCrypto, key::NoCrypto)
                            .unwrap();

                        // notify the server we're done
                        context.send_handshake(FIN.clone());

                        *self = Self::WaitingHandshake { params };
                    }
                    Self::WaitingHandshake { params } => {
                        if context.receive_handshake(None).is_none() {
                            return Poll::Pending;
                        }

                        context.on_application_protocol(NULL.clone()).unwrap();

                        context
                            .on_one_rtt_keys(
                                key::NoCrypto,
                                key::NoCrypto,
                                tls::ApplicationParameters {
                                    transport_parameters: params,
                                },
                            )
                            .unwrap();

                        context.on_handshake_complete().unwrap();

                        *self = Self::Complete;

                        return Ok(()).into();
                    }
                    Self::Complete => return Ok(()).into(),
                }
            }
        }
    }
}

pub mod server {
    use super::*;

    #[derive(Debug)]
    pub enum Session {
        Init { transport_parameters: Bytes },
        WaitingComplete,
        Complete,
    }

    impl Session {
        #[inline]
        pub fn poll<C: tls::Context<super::Session>>(
            &mut self,
            context: &mut C,
        ) -> Poll<Result<(), transport::Error>> {
            loop {
                match self {
                    Self::Init {
                        ref mut transport_parameters,
                    } => {
                        let client_params = match context.receive_initial(None) {
                            Some(data) => data,
                            None => return Poll::Pending,
                        };
                        context.send_initial(core::mem::take(transport_parameters));

                        context
                            .on_handshake_keys(key::NoCrypto, key::NoCrypto)
                            .unwrap();
                        context.send_handshake(FIN.clone());

                        context.on_application_protocol(NULL.clone()).unwrap();

                        context
                            .on_one_rtt_keys(
                                key::NoCrypto,
                                key::NoCrypto,
                                tls::ApplicationParameters {
                                    transport_parameters: &client_params,
                                },
                            )
                            .unwrap();

                        context.on_server_name(LOCALHOST.clone()).unwrap();

                        *self = Self::WaitingComplete;
                    }
                    Self::WaitingComplete => {
                        if context.receive_handshake(None).is_none() {
                            return core::task::Poll::Pending;
                        }

                        *self = Self::Complete;
                        context.on_handshake_complete().unwrap();

                        return Ok(()).into();
                    }
                    Self::Complete => return Ok(()).into(),
                }
            }
        }
    }
}

mod key {
    use super::*;

    #[derive(Debug)]
    pub struct NoCrypto;

    impl crypto::Key for NoCrypto {
        #[inline(always)]
        fn decrypt(
            &self,
            _packet_number: u64,
            _header: &[u8],
            _payload: &mut [u8],
        ) -> Result<(), crypto::CryptoError> {
            // Do nothing
            Ok(())
        }

        #[inline(always)]
        fn encrypt(
            &self,
            _packet_number: u64,
            _header: &[u8],
            _payload: &mut [u8],
        ) -> Result<(), crypto::CryptoError> {
            // Do nothing
            Ok(())
        }

        #[inline(always)]
        fn tag_len(&self) -> usize {
            0
        }

        #[inline(always)]
        fn aead_confidentiality_limit(&self) -> u64 {
            u64::MAX
        }

        #[inline(always)]
        fn aead_integrity_limit(&self) -> u64 {
            u64::MAX
        }

        #[inline(always)]
        fn cipher_suite(&self) -> tls::CipherSuite {
            tls::CipherSuite::Unknown
        }
    }

    impl crypto::HandshakeKey for NoCrypto {}

    impl crypto::HeaderKey for NoCrypto {
        #[inline(always)]
        fn opening_header_protection_mask(
            &self,
            _ciphertext_sample: &[u8],
        ) -> crypto::HeaderProtectionMask {
            Default::default()
        }

        #[inline(always)]
        fn opening_sample_len(&self) -> usize {
            size_of::<crypto::HeaderProtectionMask>()
        }

        #[inline(always)]
        fn sealing_header_protection_mask(
            &self,
            _ciphertext_sample: &[u8],
        ) -> crypto::HeaderProtectionMask {
            Default::default()
        }

        #[inline(always)]
        fn sealing_sample_len(&self) -> usize {
            size_of::<crypto::HeaderProtectionMask>()
        }
    }

    impl crypto::HandshakeHeaderKey for NoCrypto {}

    impl crypto::InitialKey for NoCrypto {
        type HeaderKey = NoCrypto;

        #[inline(always)]
        fn new_server(_connection_id: &[u8]) -> (Self, Self::HeaderKey) {
            (NoCrypto, NoCrypto)
        }

        #[inline(always)]
        fn new_client(_connection_id: &[u8]) -> (Self, Self::HeaderKey) {
            (NoCrypto, NoCrypto)
        }
    }

    impl crypto::InitialHeaderKey for NoCrypto {}

    impl crypto::OneRttKey for NoCrypto {
        #[inline(always)]
        fn derive_next_key(&self) -> Self {
            NoCrypto
        }

        #[inline(always)]
        fn update_sealer_pmtu(&mut self, _pmtu: u16) {
            // Do nothing
        }

        #[inline(always)]
        fn update_opener_pmtu(&mut self, _pmtu: u16) {
            // Do nothing
        }
    }

    impl crypto::OneRttHeaderKey for NoCrypto {}

    impl crypto::ZeroRttKey for NoCrypto {}

    impl crypto::ZeroRttHeaderKey for NoCrypto {}

    impl crypto::RetryKey for NoCrypto {
        #[inline(always)]
        fn generate_tag(_payload: &[u8]) -> crypto::retry::IntegrityTag {
            Default::default()
        }

        #[inline(always)]
        fn validate(
            _payload: &[u8],
            _tag: crypto::retry::IntegrityTag,
        ) -> Result<(), crypto::CryptoError> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::tls::testing::Pair;

    #[test]
    fn null_test() {
        let mut server = Endpoint::default();
        let mut client = Endpoint::default();

        let mut pair = Pair::new(&mut server, &mut client, LOCALHOST.clone());

        while pair.is_handshaking() {
            pair.poll(None).unwrap();
        }

        pair.finish();
    }
}
