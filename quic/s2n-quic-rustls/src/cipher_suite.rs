// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use rustls::{
    crypto::{aws_lc_rs, CryptoProvider},
    quic, CipherSuite, SupportedCipherSuite,
};
use s2n_codec::Encoder;
use s2n_quic_core::crypto::{self, packet_protection, scatter, tls, HeaderProtectionMask, Key};

/// `aws_lc_rs` is the default crypto provider since that is also the
/// default used by rustls.
pub fn default_crypto_provider() -> Result<CryptoProvider, rustls::Error> {
    Ok(CryptoProvider {
        cipher_suites: DEFAULT_CIPHERSUITES.to_vec(),
        ..aws_lc_rs::default_provider()
    })
}

pub struct PacketKey {
    key: Box<dyn quic::PacketKey>,
    cipher_suite: tls::CipherSuite,
}

impl PacketKey {
    pub(crate) fn new(
        keys: quic::DirectionalKeys,
        cipher_suite: tls::CipherSuite,
    ) -> (Self, HeaderProtectionKey) {
        (
            Self {
                key: keys.packet,
                cipher_suite,
            },
            HeaderProtectionKey(keys.header),
        )
    }
}

impl crypto::Key for PacketKey {
    #[inline]
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), packet_protection::Error> {
        match self.key.decrypt_in_place(packet_number, header, payload) {
            Ok(_tag) => Ok(()),
            Err(_) => Err(packet_protection::Error::DECRYPT_ERROR),
        }
    }

    #[inline]
    fn encrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut scatter::Buffer,
    ) -> Result<(), packet_protection::Error> {
        let buffer = payload.flatten();
        let (payload, _) = buffer.split_mut();
        match self.key.encrypt_in_place(packet_number, header, payload) {
            Ok(tag) => {
                buffer.write_slice(tag.as_ref());
                Ok(())
            }
            Err(_) => Err(packet_protection::Error::INTERNAL_ERROR),
        }
    }

    #[inline]
    fn tag_len(&self) -> usize {
        self.key.tag_len()
    }

    #[inline]
    fn aead_confidentiality_limit(&self) -> u64 {
        self.key.confidentiality_limit()
    }

    #[inline]
    fn aead_integrity_limit(&self) -> u64 {
        self.key.integrity_limit()
    }

    #[inline]
    fn cipher_suite(&self) -> tls::CipherSuite {
        self.cipher_suite
    }
}

impl crypto::ZeroRttKey for PacketKey {}

pub struct PacketKeys {
    sealer: PacketKey,
    opener: PacketKey,
}

impl PacketKeys {
    pub(crate) fn new(keys: quic::Keys, cipher_suite: CipherSuite) -> (Self, HeaderProtectionKeys) {
        let quic::Keys { local, remote } = keys;

        let cipher_suite = match cipher_suite {
            CipherSuite::TLS13_AES_128_GCM_SHA256 => tls::CipherSuite::TLS_AES_128_GCM_SHA256,
            CipherSuite::TLS13_AES_256_GCM_SHA384 => tls::CipherSuite::TLS_AES_256_GCM_SHA384,
            CipherSuite::TLS13_CHACHA20_POLY1305_SHA256 => {
                tls::CipherSuite::TLS_CHACHA20_POLY1305_SHA256
            }
            _ => tls::CipherSuite::Unknown,
        };

        let (sealer_packet, sealer_header) = PacketKey::new(local, cipher_suite);
        let (opener_packet, opener_header) = PacketKey::new(remote, cipher_suite);

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
        self.sealer.aead_integrity_limit()
    }

    #[inline]
    fn cipher_suite(&self) -> tls::CipherSuite {
        self.sealer.cipher_suite()
    }
}

impl crypto::HandshakeKey for PacketKeys {}

pub struct HeaderProtectionKey(Box<dyn quic::HeaderProtectionKey>);

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
        cipher_suite: CipherSuite,
    ) -> (Self, HeaderProtectionKeys) {
        let (key, header_key) = PacketKeys::new(keys, cipher_suite);
        let key = Self { key, secrets };
        (key, header_key)
    }
}

impl crypto::Key for OneRttKey {
    #[inline]
    fn decrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut [u8],
    ) -> Result<(), packet_protection::Error> {
        self.key.decrypt(packet_number, header, payload)
    }

    #[inline]
    fn encrypt(
        &self,
        packet_number: u64,
        header: &[u8],
        payload: &mut scatter::Buffer,
    ) -> Result<(), packet_protection::Error> {
        self.key.encrypt(packet_number, header, payload)
    }

    #[inline]
    fn tag_len(&self) -> usize {
        self.key.tag_len()
    }

    #[inline]
    fn aead_confidentiality_limit(&self) -> u64 {
        self.key.aead_confidentiality_limit()
    }

    #[inline]
    fn aead_integrity_limit(&self) -> u64 {
        self.key.aead_integrity_limit()
    }

    #[inline]
    fn cipher_suite(&self) -> tls::CipherSuite {
        self.key.cipher_suite()
    }
}

impl crypto::OneRttKey for OneRttKey {
    fn derive_next_key(&self) -> Self {
        let cipher_suite = self.cipher_suite();
        let mut secrets = self.secrets.clone();
        let quic::PacketKeySet { local, remote } = secrets.next_packet_keys();
        Self {
            key: PacketKeys {
                sealer: PacketKey {
                    key: local,
                    cipher_suite,
                },
                opener: PacketKey {
                    key: remote,
                    cipher_suite,
                },
            },
            secrets,
        }
    }

    fn update_sealer_pmtu(&mut self, _pmtu: u16) {
        // rustls doesn't have any ptmu specialization
    }

    fn update_opener_pmtu(&mut self, _pmtu: u16) {
        // rustls doesn't have any ptmu specialization
    }
}

//= https://www.rfc-editor.org/rfc/rfc9001#section-5.3
//# A cipher suite MUST NOT be
//# negotiated unless a header protection scheme is defined for the
//# cipher suite.
// All of the cipher_suites from the current exported list have HP schemes for QUIC
static DEFAULT_CIPHERSUITES: &[SupportedCipherSuite] = &[
    aws_lc_rs::cipher_suite::TLS13_AES_128_GCM_SHA256,
    aws_lc_rs::cipher_suite::TLS13_AES_256_GCM_SHA384,
    aws_lc_rs::cipher_suite::TLS13_CHACHA20_POLY1305_SHA256,
];

#[test]
fn test_default_cipher_suites() {
    insta::assert_debug_snapshot!("default_cipher_suites", DEFAULT_CIPHERSUITES);
}
