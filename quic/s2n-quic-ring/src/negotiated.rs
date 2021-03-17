// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    ciphersuite::NegotiatedCiphersuite as Ciphersuite, header_key::HeaderKeyPair, Algorithm,
    SecretPair,
};
use s2n_quic_core::{
    crypto::{CryptoError, Key},
    endpoint,
};

#[derive(Debug)]
pub struct KeyPair {
    sealer: Ciphersuite,
    opener: Ciphersuite,
}

impl KeyPair {
    pub fn new(
        endpoint: endpoint::Type,
        algorithm: &Algorithm,
        secrets: SecretPair,
    ) -> Option<(Self, HeaderKeyPair)> {
        let (sealer_secret, opener_secret) = match endpoint {
            endpoint::Type::Client => (secrets.client, secrets.server),
            endpoint::Type::Server => (secrets.server, secrets.client),
        };

        let (sealer, header_sealer) = Ciphersuite::new(algorithm, sealer_secret)?;
        let (opener, header_opener) = Ciphersuite::new(algorithm, opener_secret)?;

        let key = Self { sealer, opener };
        let header_key = HeaderKeyPair {
            sealer: header_sealer,
            opener: header_opener,
        };

        Some((key, header_key))
    }

    /// Update the ciphersuite as defined in
    /// https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6
    pub fn update(&self) -> Self {
        Self {
            sealer: self.sealer.update(),
            opener: self.opener.update(),
        }
    }
}

impl Key for KeyPair {
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        self.opener.decrypt(packet_number, header, payload)
    }

    fn encrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        self.sealer.encrypt(packet_number, header, payload)
    }

    fn tag_len(&self) -> usize {
        self.sealer.tag_len()
    }

    fn aead_confidentiality_limit(&self) -> u64 {
        self.sealer.aead_confidentiality_limit()
    }

    fn aead_integrity_limit(&self) -> u64 {
        self.opener.aead_integrity_limit()
    }
}

macro_rules! negotiated_crypto {
    ($name:ident, $header_key:ident) => {
        #[derive(Debug)]
        pub struct $name(crate::negotiated::KeyPair);

        impl $name {
            /// Create a server ciphersuite with a given negotiated algorithm and secret
            pub fn new_server(
                algorithm: &$crate::Algorithm,
                secrets: $crate::SecretPair,
            ) -> Option<(Self, $header_key)> {
                Self::new(s2n_quic_core::endpoint::Type::Server, algorithm, secrets)
            }

            /// Create a client ciphersuite with a given negotiated algorithm and secret
            pub fn new_client(
                algorithm: &$crate::Algorithm,
                secrets: $crate::SecretPair,
            ) -> Option<(Self, $header_key)> {
                Self::new(s2n_quic_core::endpoint::Type::Client, algorithm, secrets)
            }

            /// Create a ciphersuite for an endpoint type with a given negotiated algorithm and secret
            pub fn new(
                endpoint: s2n_quic_core::endpoint::Type,
                algorithm: &$crate::Algorithm,
                secrets: $crate::SecretPair,
            ) -> Option<(Self, $header_key)> {
                let (key, header_key) =
                    crate::negotiated::KeyPair::new(endpoint, algorithm, secrets)?;

                let key = Self(key);
                let header_key = $header_key(header_key);
                Some((key, header_key))
            }

            /// Update the ciphersuite as defined in
            /// https://tools.ietf.org/id/draft-ietf-quic-tls-32.txt#6
            pub fn update(&self) -> Self {
                Self(self.0.update())
            }
        }

        impl s2n_quic_core::crypto::Key for $name {
            fn decrypt(
                &self,
                packet_number: u64,
                header: &[u8],
                payload: &mut [u8],
            ) -> Result<(), s2n_quic_core::crypto::CryptoError> {
                self.0.decrypt(packet_number, header, payload)
            }

            fn encrypt(
                &self,
                packet_number: u64,
                header: &[u8],
                payload: &mut [u8],
            ) -> Result<(), s2n_quic_core::crypto::CryptoError> {
                self.0.encrypt(packet_number, header, payload)
            }

            fn tag_len(&self) -> usize {
                self.0.tag_len()
            }

            fn aead_confidentiality_limit(&self) -> u64 {
                self.0.aead_confidentiality_limit()
            }

            fn aead_integrity_limit(&self) -> u64 {
                self.0.aead_integrity_limit()
            }
        }
    };
}
