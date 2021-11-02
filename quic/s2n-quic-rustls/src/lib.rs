// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

mod ciphersuite;
mod error;
mod session;

pub mod certificate;
pub mod client;
pub mod server;

pub use client::Client;
pub use server::Server;

//= https://www.rfc-editor.org/rfc/rfc9001.txt#4.2
//# Clients MUST NOT offer TLS versions older than 1.3.
static PROTOCOL_VERSIONS: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];

/// The supported version of quic
const QUIC_VERSION: rustls::quic::Version = rustls::quic::Version::V1;

/// Encodes transport parameters into a byte vec
pub(crate) fn encode_transport_parameters<Params: s2n_codec::EncoderValue>(
    params: &Params,
) -> Vec<u8> {
    let len = params.encoding_size();
    let mut buffer = vec![0; len];
    params.encode(&mut s2n_codec::EncoderBuffer::new(&mut buffer));
    buffer
}

#[test]
fn client_server_test() {
    use s2n_quic_core::crypto::tls::{self, testing::certificates::*};

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

    let mut pair = tls::testing::Pair::new(&mut server, &mut client, "localhost".into());

    while pair.is_handshaking() {
        pair.poll().unwrap();
    }

    pair.finish();
}
