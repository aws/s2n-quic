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
use core::{any::Any, mem::size_of, task::Poll};

#[derive(Debug)]
pub struct Endpoint<T = ()>(Option<T>);
impl<T> Endpoint<T> {
    pub fn new(ctx: Option<T>) -> Self {
        Endpoint(ctx)
    }
}
impl<T> Default for Endpoint<T> {
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
            eprintln!("{WARNING}");
            eprintln!();
            eprintln!("                  =====  W A R N I N G !!! =====");
            eprintln!();
            eprintln!(
                "  An s2n-quic endpoint has been configured without cryptographic protections."
            );
            eprintln!("  This should ONLY be used for testing purposes. Without cryptographic");
            eprintln!("  protections in place, s2n-quic cannot guarantee confidentiality,");
            eprintln!("  integrity, or authenticity.");
            eprintln!();
            let location = core::panic::Location::caller();
            eprintln!("  Endpoint configured by: {location}");
            eprintln!();
            eprintln!("                  ==============================");
        }

        Self(None)
    }
}

impl<T: Send + Clone + 'static + std::fmt::Debug> crypto::tls::Endpoint for Endpoint<T> {
    type Session = Session<T>;

    #[inline]
    fn new_server_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
    ) -> Self::Session {
        let params = transport_parameters.encode_to_vec().into();
        Session::Server(server::TlsSession::Init {
            transport_parameters: params,
            ctx: self.0.clone(),
        })
    }

    #[inline]
    fn new_client_session<Params: s2n_codec::EncoderValue>(
        &mut self,
        transport_parameters: &Params,
        server_name: ServerName,
    ) -> Self::Session {
        assert_eq!(server_name, LOCALHOST);

        let params = transport_parameters.encode_to_vec().into();
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
pub enum Session<T> {
    Client(client::Session<T>),
    Server(server::TlsSession<T>),
}

impl<T> crypto::CryptoSuite for Session<T> {
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

impl<T: std::fmt::Debug + Send + Clone + 'static> tls::Session for Session<T> {
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

static FIN: Bytes = Bytes::from_static(b"FIN");
static NULL: Bytes = Bytes::from_static(b"NULL");

pub mod client {
    use super::*;
    use crate::crypto::tls::NamedGroup;
    use core::marker::PhantomData;

    #[derive(Debug)]
    pub enum Session<T> {
        Init { transport_parameters: Bytes },
        WaitingInitial {},
        WaitingHandshake { params: Bytes },
        Complete,
        _PH(PhantomData<T>),
    }

    impl<T> Session<T> {
        #[inline]
        pub fn poll<C: tls::Context<super::Session<T>>>(
            &mut self,
            context: &mut C,
        ) -> Poll<Result<(), transport::Error>> {
            loop {
                match self {
                    Self::Init {
                        ref mut transport_parameters,
                    } => {
                        context.send_initial(core::mem::take(transport_parameters));

                        context.on_server_name(LOCALHOST.clone())?;

                        *self = Self::WaitingInitial {};
                    }
                    Self::WaitingInitial {} => {
                        let params = match context.receive_initial(None) {
                            Some(bytes) => bytes,
                            None => return Poll::Pending,
                        };

                        context.on_handshake_keys(key::NoCrypto, key::NoCrypto)?;

                        // notify the server we're done
                        context.send_handshake(FIN.clone());

                        *self = Self::WaitingHandshake { params };
                    }
                    Self::WaitingHandshake { params } => {
                        if context.receive_handshake(None).is_none() {
                            return Poll::Pending;
                        }

                        context.on_application_protocol(NULL.clone())?;
                        context.on_key_exchange_group(NamedGroup {
                            group_name: "null_group",
                            contains_kem: false,
                        })?;

                        context.on_one_rtt_keys(
                            key::NoCrypto,
                            key::NoCrypto,
                            tls::ApplicationParameters {
                                transport_parameters: params,
                            },
                        )?;

                        context.on_handshake_complete()?;

                        *self = Self::Complete;

                        return Ok(()).into();
                    }
                    Self::Complete => return Ok(()).into(),
                    _ => unreachable!(),
                }
            }
        }
    }
}

pub mod server {
    use super::*;
    use crate::crypto::tls::NamedGroup;

    #[derive(Debug)]
    pub enum TlsSession<T> {
        Init {
            transport_parameters: Bytes,
            ctx: Option<T>,
        },
        WaitingComplete,
        Complete,
    }

    impl<T: Send + Clone + 'static> TlsSession<T> {
        #[inline]
        pub fn poll<C: tls::Context<super::Session<T>>>(
            &mut self,
            context: &mut C,
        ) -> Poll<Result<(), transport::Error>> {
            loop {
                match self {
                    Self::Init {
                        ref mut transport_parameters,
                        ref mut ctx,
                    } => {
                        let client_params = match context.receive_initial(None) {
                            Some(data) => data,
                            None => return Poll::Pending,
                        };
                        context.send_initial(core::mem::take(transport_parameters));

                        context.on_handshake_keys(key::NoCrypto, key::NoCrypto)?;
                        context.send_handshake(FIN.clone());

                        context.on_application_protocol(NULL.clone())?;
                        context.on_key_exchange_group(NamedGroup {
                            group_name: "null_group",
                            contains_kem: false,
                        })?;
                        // We just clone and set it, in real user case, you can put anything you want.
                        if let Some(ctx) = ctx {
                            context.on_tls_context(
                                Box::new(ctx.clone()) as Box<dyn Any + Send + 'static>
                            );
                        }

                        context.on_one_rtt_keys(
                            key::NoCrypto,
                            key::NoCrypto,
                            tls::ApplicationParameters {
                                transport_parameters: &client_params,
                            },
                        )?;

                        context.on_server_name(LOCALHOST.clone())?;

                        *self = Self::WaitingComplete;
                    }
                    Self::WaitingComplete => {
                        if context.receive_handshake(None).is_none() {
                            return core::task::Poll::Pending;
                        }

                        *self = Self::Complete;
                        context.on_handshake_complete()?;

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
    use crate::crypto::scatter;

    #[derive(Debug)]
    pub struct NoCrypto;

    impl crypto::Key for NoCrypto {
        #[inline(always)]
        fn decrypt(
            &self,
            _packet_number: u64,
            _header: &[u8],
            _payload: &mut [u8],
        ) -> Result<(), crypto::packet_protection::Error> {
            // Do nothing
            Ok(())
        }

        #[inline(always)]
        fn encrypt(
            &mut self,
            _packet_number: u64,
            _header: &[u8],
            payload: &mut scatter::Buffer,
        ) -> Result<(), crypto::packet_protection::Error> {
            // copy any extra bytes into the slice
            payload.flatten();
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
        ) -> Result<(), crypto::packet_protection::Error> {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::tls::testing::Pair;
    use bolero::check;
    use bytes::{BufMut, Bytes, BytesMut};
    use std::collections::VecDeque;

    #[test]
    fn null_test() {
        let mut server = Endpoint::<()>::default();
        let mut client = Endpoint::<()>::default();

        let mut pair = Pair::new(&mut server, &mut client, LOCALHOST.clone());

        while pair.is_handshaking() {
            pair.poll(None).unwrap();
        }

        pair.finish();
    }

    #[test]
    fn fuzz_test() {
        let mut server = Endpoint::<()>::default();
        let mut client = Endpoint::<()>::default();

        check!().for_each(|mut bytes| {
            // replaces a single buffer with fuzz bytes
            let mut replace_bytes = |chunks: &mut VecDeque<Bytes>| {
                for chunk in chunks {
                    let len = chunk.len().min(bytes.len());
                    let (data, remaining) = bytes.split_at(len);
                    bytes = remaining;
                    let mut replacement = BytesMut::with_capacity(chunk.len());
                    replacement.put_slice(data);
                    replacement.put_bytes(0, chunk.len() - data.len());
                    assert_eq!(chunk.len(), replacement.len());
                    *chunk = replacement.freeze();
                }
            };

            let mut pair = Pair::new(&mut server, &mut client, LOCALHOST.clone());

            while pair.is_handshaking() {
                if pair.poll_start().is_err() {
                    break;
                }

                // replace all of the buffers with fuzz bytes
                replace_bytes(&mut pair.server.context.initial.rx);
                replace_bytes(&mut pair.server.context.initial.tx);
                replace_bytes(&mut pair.server.context.handshake.rx);
                replace_bytes(&mut pair.server.context.handshake.tx);
                replace_bytes(&mut pair.server.context.application.rx);
                replace_bytes(&mut pair.server.context.application.tx);

                if pair.poll_finish(None).is_err() {
                    break;
                }
            }
        });
    }
}
