// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use rustls::{cipher_suite as ciphers, quic, CipherSuite, SupportedCipherSuite};
use s2n_quic_core::{
    crypto::{self, CryptoError, HeaderProtectionMask, Key},
    event,
};

pub struct PacketKey(quic::PacketKey, event::builder::Ciphersuite);

impl PacketKey {
    pub(crate) fn new(
        keys: quic::DirectionalKeys,
        ciphersuite: event::builder::Ciphersuite,
    ) -> (Self, HeaderProtectionKey) {
        (
            Self(keys.packet, ciphersuite),
            HeaderProtectionKey(keys.header),
        )
    }
}

impl crypto::Key for PacketKey {
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        match self.0.decrypt_in_place(packet_number, header, payload) {
            Ok(_tag) => Ok(()),
            Err(_) => Err(CryptoError::DECRYPT_ERROR),
        }
    }

    fn encrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        let (payload, tag_storage) = payload.split_at_mut(payload.len() - self.tag_len());
        match self.0.encrypt_in_place(packet_number, header, payload) {
            Ok(tag) => {
                tag_storage.copy_from_slice(tag.as_ref());
                Ok(())
            }
            Err(_) => Err(CryptoError::INTERNAL_ERROR),
        }
    }

    fn tag_len(&self) -> usize {
        self.0.tag_len()
    }

    fn aead_confidentiality_limit(&self) -> u64 {
        self.0.confidentiality_limit()
    }

    fn aead_integrity_limit(&self) -> u64 {
        self.0.integrity_limit()
    }

    fn ciphersuite(&self) -> s2n_quic_core::event::builder::Ciphersuite {
        self.1.clone()
    }
}

impl crypto::ZeroRttKey for PacketKey {}

pub struct PacketKeys {
    sealer: PacketKey,
    opener: PacketKey,
}

impl PacketKeys {
    pub(crate) fn new(keys: quic::Keys, ciphersuite: CipherSuite) -> (Self, HeaderProtectionKeys) {
        let quic::Keys { local, remote } = keys;

        let ciphersuite = match ciphersuite {
            CipherSuite::TLS13_AES_128_GCM_SHA256 => {
                event::builder::Ciphersuite::TLS_AES_128_GCM_SHA256
            }
            CipherSuite::TLS13_AES_256_GCM_SHA384 => {
                event::builder::Ciphersuite::TLS_AES_256_GCM_SHA384
            }
            CipherSuite::TLS13_CHACHA20_POLY1305_SHA256 => {
                event::builder::Ciphersuite::TLS_CHACHA20_POLY1305_SHA256
            }
            _ => event::builder::Ciphersuite::Unknown,
        };

        let (sealer_packet, sealer_header) = PacketKey::new(local, ciphersuite.clone());
        let (opener_packet, opener_header) = PacketKey::new(remote, ciphersuite);

        let key = Self {
            sealer: sealer_packet,
            opener: opener_packet,
        };

        let header_key = HeaderProtectionKeys {
            sealer: sealer_header,
            opener: opener_header,
        };

        (key, header_key)
    }
}

impl crypto::Key for PacketKeys {
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
        self.sealer.aead_integrity_limit()
    }

    fn ciphersuite(&self) -> s2n_quic_core::event::builder::Ciphersuite {
        self.sealer.ciphersuite()
    }
}

impl crypto::HandshakeKey for PacketKeys {}

pub struct HeaderProtectionKey(quic::HeaderProtectionKey);

impl HeaderProtectionKey {
    /// Returns the header protection mask for the given ciphertext sample
    ///
    /// Rustls API applies the header protection rather than returning
    /// the mask. This method exists for extracting the mask from rustls by calling
    /// the `encrypt_in_place` api and reversing the operation it applies.
    ///
    /// The primary motivation for extracting the mask from rustls is to maintain API
    /// compatibility with other tls providers (s2n-tls) and have one common code
    /// for applying and removing header protection.
    fn get_mask(&self, ciphertext_sample: &[u8]) -> HeaderProtectionMask {
        let mut mask = HeaderProtectionMask::default();

        // tell rustls we need all of the packet number mask by setting the pn length to 4
        let tag = 0b0000_0011;
        mask[0] = tag;

        let (packet_tag, packet_number) = mask.split_first_mut().unwrap();
        self.0
            .encrypt_in_place(ciphertext_sample, packet_tag, packet_number)
            .unwrap();

        // rustls XORs the mask with the original tag so undo that operation
        mask[0] ^= tag;

        mask
    }
}

impl crypto::HeaderKey for HeaderProtectionKey {
    fn opening_header_protection_mask(&self, ciphertext_sample: &[u8]) -> HeaderProtectionMask {
        self.get_mask(ciphertext_sample)
    }

    fn opening_sample_len(&self) -> usize {
        self.0.sample_len()
    }

    fn sealing_header_protection_mask(&self, ciphertext_sample: &[u8]) -> HeaderProtectionMask {
        self.get_mask(ciphertext_sample)
    }

    fn sealing_sample_len(&self) -> usize {
        self.0.sample_len()
    }
}

impl crypto::ZeroRttHeaderKey for HeaderProtectionKey {}

pub struct HeaderProtectionKeys {
    sealer: HeaderProtectionKey,
    opener: HeaderProtectionKey,
}

impl crypto::HeaderKey for HeaderProtectionKeys {
    fn opening_header_protection_mask(&self, ciphertext_sample: &[u8]) -> HeaderProtectionMask {
        self.opener
            .opening_header_protection_mask(ciphertext_sample)
    }

    fn opening_sample_len(&self) -> usize {
        self.opener.opening_sample_len()
    }

    fn sealing_header_protection_mask(&self, ciphertext_sample: &[u8]) -> HeaderProtectionMask {
        self.sealer
            .sealing_header_protection_mask(ciphertext_sample)
    }

    fn sealing_sample_len(&self) -> usize {
        self.sealer.sealing_sample_len()
    }
}

impl crypto::HandshakeHeaderKey for HeaderProtectionKeys {}
impl crypto::OneRttHeaderKey for HeaderProtectionKeys {}

pub struct OneRttKey {
    key: PacketKeys,
    secrets: quic::Secrets,
}

impl OneRttKey {
    pub(crate) fn new(
        keys: quic::Keys,
        secrets: quic::Secrets,
        ciphersuite: CipherSuite,
    ) -> (Self, HeaderProtectionKeys) {
        let (key, header_key) = PacketKeys::new(keys, ciphersuite);
        let key = Self { key, secrets };
        (key, header_key)
    }
}

impl crypto::Key for OneRttKey {
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        self.key.decrypt(packet_number, header, payload)
    }

    fn encrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), CryptoError> {
        self.key.encrypt(packet_number, header, payload)
    }

    fn tag_len(&self) -> usize {
        self.key.tag_len()
    }

    fn aead_confidentiality_limit(&self) -> u64 {
        self.key.aead_confidentiality_limit()
    }

    fn aead_integrity_limit(&self) -> u64 {
        self.key.aead_integrity_limit()
    }

    fn ciphersuite(&self) -> s2n_quic_core::event::builder::Ciphersuite {
        self.key.ciphersuite()
    }
}

impl crypto::OneRttKey for OneRttKey {
    fn derive_next_key(&self) -> Self {
        let ciphersuite = self.ciphersuite();
        let mut secrets = self.secrets.clone();
        let quic::PacketKeySet { local, remote } = secrets.next_packet_keys();
        Self {
            key: PacketKeys {
                sealer: PacketKey(local, ciphersuite.clone()),
                opener: PacketKey(remote, ciphersuite),
            },
            secrets,
        }
    }
}

//= https://www.rfc-editor.org/rfc/rfc9001.txt#5.3
//# A cipher suite MUST NOT be
//# negotiated unless a header protection scheme is defined for the
//# cipher suite.
// All of the ciphersuites from the current exported list have HP schemes for QUIC
pub static DEFAULT_CIPHERSUITES: &[SupportedCipherSuite] = &[
    ciphers::TLS13_AES_128_GCM_SHA256,
    ciphers::TLS13_AES_256_GCM_SHA384,
    ciphers::TLS13_CHACHA20_POLY1305_SHA256,
];

#[test]
fn test_default_ciphersuites() {
    insta::assert_debug_snapshot!("default_ciphersuites", DEFAULT_CIPHERSUITES);
}
