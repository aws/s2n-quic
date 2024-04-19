// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    cipher_suite::NegotiatedCipherSuite as CipherSuite, header_key::HeaderKeyPair,
    ring_aead::Algorithm, SecretPair,
};
use s2n_quic_core::{
    crypto::{packet_protection, scatter, Key},
    endpoint,
};

#[derive(Debug)]
pub struct KeyPair {
    pub(crate) sealer: CipherSuite,
    pub(crate) opener: CipherSuite,
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

        let (sealer, header_sealer) = CipherSuite::new(algorithm, sealer_secret)?;
        let (opener, header_opener) = CipherSuite::new(algorithm, opener_secret)?;

        let key = Self { sealer, opener };
        let header_key = HeaderKeyPair {
            sealer: header_sealer,
            opener: header_opener,
        };

        Some((key, header_key))
    }

    /// Update the cipher_suite as defined in
    /// <https://www.rfc-editor.org/rfc/rfc9001#section-6>
    #[inline]
    pub fn update(&self) -> Self {
        Self {
            sealer: self.sealer.update(),
            opener: self.opener.update(),
        }
    }
}

impl Key for KeyPair {
    #[inline]
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), packet_protection::Error> {
        self.opener.decrypt(packet_number, header, payload)
    }

    #[inline]
    fn encrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut scatter::Buffer,
    ) -> Result<(), packet_protection::Error> {
        self.sealer.encrypt(packet_number, header, payload)
    }

    #[inline]
    fn tag_len(&self) -> usize {
        self.sealer.tag_len()
    }

    #[inline]
    fn aead_confidentiality_limit(&self) -> u64 {
        self.sealer.aead_confidentiality_limit()
    }

    #[inline]
    fn aead_integrity_limit(&self) -> u64 {
        self.opener.aead_integrity_limit()
    }

    #[inline]
    fn cipher_suite(&self) -> s2n_quic_core::crypto::tls::CipherSuite {
        self.opener.cipher_suite()
    }
}

macro_rules! negotiated_crypto {
    ($name:ident, $header_key:ident) => {
        #[derive(Debug)]
        pub struct $name(crate::negotiated::KeyPair);

        impl $name {
            /// Create a server cipher suite with a given negotiated algorithm and secret
            pub fn new_server(
                algorithm: &$crate::ring_aead::Algorithm,
                secrets: $crate::SecretPair,
            ) -> Option<(Self, $header_key)> {
                Self::new(s2n_quic_core::endpoint::Type::Server, algorithm, secrets)
            }

            /// Create a client cipher suite with a given negotiated algorithm and secret
            pub fn new_client(
                algorithm: &$crate::ring_aead::Algorithm,
                secrets: $crate::SecretPair,
            ) -> Option<(Self, $header_key)> {
                Self::new(s2n_quic_core::endpoint::Type::Client, algorithm, secrets)
            }

            /// Create a cipher_suite for an endpoint type with a given negotiated algorithm and secret
            pub fn new(
                endpoint: s2n_quic_core::endpoint::Type,
                algorithm: &$crate::ring_aead::Algorithm,
                secrets: $crate::SecretPair,
            ) -> Option<(Self, $header_key)> {
                let (key, header_key) =
                    crate::negotiated::KeyPair::new(endpoint, algorithm, secrets)?;

                let key = Self(key);
                let header_key = $header_key::from(header_key);
                Some((key, header_key))
            }

            /// Update the cipher suite as defined in
            /// <https://www.rfc-editor.org/rfc/rfc9001#section-6>
            #[inline]
            #[must_use]
            pub fn update(&self) -> Self {
                Self(self.0.update())
            }
        }

        impl s2n_quic_core::crypto::Key for $name {
            #[inline]
            fn decrypt(
                &self,
                packet_number: u64,
                header: &[u8],
                payload: &mut [u8],
            ) -> Result<(), s2n_quic_core::crypto::packet_protection::Error> {
                self.0.decrypt(packet_number, header, payload)
            }

            #[inline]
            fn encrypt(
                &self,
                packet_number: u64,
                header: &[u8],
                payload: &mut s2n_quic_core::crypto::scatter::Buffer,
            ) -> Result<(), s2n_quic_core::crypto::packet_protection::Error> {
                self.0.encrypt(packet_number, header, payload)
            }

            #[inline]
            fn tag_len(&self) -> usize {
                self.0.tag_len()
            }

            #[inline]
            fn aead_confidentiality_limit(&self) -> u64 {
                self.0.aead_confidentiality_limit()
            }

            #[inline]
            fn aead_integrity_limit(&self) -> u64 {
                self.0.aead_integrity_limit()
            }

            #[inline]
            fn cipher_suite(&self) -> s2n_quic_core::crypto::tls::CipherSuite {
                self.0.cipher_suite()
            }
        }
    };
}
