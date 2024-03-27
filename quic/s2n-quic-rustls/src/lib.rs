// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

// re-export rustls
pub use rustls;

mod cipher_suite;
mod error;
mod session;

pub mod certificate;
pub mod client;
pub mod server;

pub use cipher_suite::default_crypto_provider;
pub use client::{default_config_builder as client_config_builder, Client};
pub use server::{default_config_builder as server_config_builder, Server};

//= https://www.rfc-editor.org/rfc/rfc9001#section-4.2
//# Clients MUST NOT offer TLS versions older than 1.3.
static PROTOCOL_VERSIONS: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];

/// The supported version of quic
const QUIC_VERSION: rustls::quic::Version = rustls::quic::Version::V1;

#[cfg(test)]
mod tests {
    use super::*;
    use s2n_quic_core::crypto::tls::{self, testing::certificates::*};

    #[test]
    fn client_server_test() {
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
            pair.poll(None).unwrap();
        }

        pair.finish();
    }

    #[test]
    fn client_server_der_test() {
        let mut client = client::Builder::new()
            .with_certificate(CERT_DER)
            .unwrap()
            .build()
            .unwrap();

        let mut server = server::Builder::new()
            .with_certificate(CERT_DER, KEY_DER)
            .unwrap()
            .build()
            .unwrap();

        let mut pair = tls::testing::Pair::new(&mut server, &mut client, "localhost".into());

        while pair.is_handshaking() {
            pair.poll(None).unwrap();
        }

        pair.finish();
    }

    #[test]
    fn client_server_pkcs1_test() {
        let mut client = client::Builder::new()
            .with_certificate(CERT_PKCS1_PEM)
            .unwrap()
            .build()
            .unwrap();

        let mut server = server::Builder::new()
            .with_certificate(CERT_PKCS1_PEM, KEY_PKCS1_PEM)
            .unwrap()
            .build()
            .unwrap();

        let mut pair = tls::testing::Pair::new(&mut server, &mut client, "localhost".into());

        while pair.is_handshaking() {
            pair.poll(None).unwrap();
        }

        pair.finish();
    }
}
