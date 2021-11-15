// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use ring::aead;
use s2n_quic_core::{
    crypto::{self, mask_from_packet_tag, xor_mask, CryptoError, HeaderProtectionMask},
    packet::number::PacketNumberSpace,
};

pub struct HeaderKey(pub(crate) aead::quic::HeaderProtectionKey);

impl crypto::HeaderKey for HeaderKey {
    #[inline]
    fn opening_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        self.header_protection_mask(sample)
    }

    #[inline]
    fn opening_sample_len(&self) -> usize {
        self.0.algorithm().sample_len()
    }

    #[inline]
    fn sealing_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        self.header_protection_mask(sample)
    }

    #[inline]
    fn sealing_sample_len(&self) -> usize {
        self.0.algorithm().sample_len()
    }

    #[inline]
    fn unprotect(
        &self,
        ciphertext_sample: &[u8],
        first: &mut u8,
        packet_number: &mut [u8],
        space: PacketNumberSpace,
    ) -> Result<(), CryptoError> {
        let mask = self.sealing_header_protection_mask(ciphertext_sample);
        *first ^= mask[0] & mask_from_packet_tag(*first);
        let packet_number_len = space.new_packet_number_len(*first);

        xor_mask(&mut packet_number[..packet_number_len.bytesize()], &mask);
        Ok(())
    }

    #[inline]
    fn protect(
        &self,
        sample: &[u8],
        first: &mut u8,
        packet_number: &mut [u8],
    ) -> Result<(), CryptoError> {
        let mask = self.sealing_header_protection_mask(sample);
        *first ^= mask[0] & mask_from_packet_tag(*first);

        xor_mask(packet_number, &mask);
        Ok(())
    }
}

impl HeaderKey {
    #[inline]
    fn header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        self.0
            .new_mask(sample)
            .expect("sample length already checked")
    }
}

impl fmt::Debug for HeaderKey {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("HeaderKey").finish()
    }
}

#[derive(Debug)]
pub struct HeaderKeyPair {
    pub(crate) sealer: HeaderKey,
    pub(crate) opener: HeaderKey,
}

impl crypto::HeaderKey for HeaderKeyPair {
    #[inline]
    fn opening_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        self.opener.opening_header_protection_mask(sample)
    }

    #[inline]
    fn opening_sample_len(&self) -> usize {
        self.opener.opening_sample_len()
    }

    #[inline]
    fn sealing_header_protection_mask(&self, sample: &[u8]) -> HeaderProtectionMask {
        self.sealer.sealing_header_protection_mask(sample)
    }

    #[inline]
    fn sealing_sample_len(&self) -> usize {
        self.sealer.sealing_sample_len()
    }

    fn unprotect(
        &self,
        ciphertext_sample: &[u8],
        first: &mut u8,
        packet_number: &mut [u8],
        space: PacketNumberSpace,
    ) -> Result<(), CryptoError> {
        self.opener
            .unprotect(ciphertext_sample, first, packet_number, space)
    }

    fn protect(
        &self,
        sample: &[u8],
        first: &mut u8,
        packet_number: &mut [u8],
    ) -> Result<(), CryptoError> {
        self.sealer.protect(sample, first, packet_number)
    }
}

macro_rules! header_key {
    ($name:ident) => {
        #[derive(Debug)]
        pub struct $name(crate::header_key::HeaderKeyPair);

        impl s2n_quic_core::crypto::HeaderKey for $name {
            #[inline]
            fn opening_header_protection_mask(
                &self,
                sample: &[u8],
            ) -> s2n_quic_core::crypto::HeaderProtectionMask {
                self.0.opening_header_protection_mask(sample)
            }

            #[inline]
            fn opening_sample_len(&self) -> usize {
                self.0.opening_sample_len()
            }

            #[inline]
            fn sealing_header_protection_mask(
                &self,
                sample: &[u8],
            ) -> s2n_quic_core::crypto::HeaderProtectionMask {
                self.0.sealing_header_protection_mask(sample)
            }

            #[inline]
            fn sealing_sample_len(&self) -> usize {
                self.0.sealing_sample_len()
            }

            #[inline]
            fn unprotect(
                &self,
                ciphertext_sample: &[u8],
                first: &mut u8,
                packet_number: &mut [u8],
                space: s2n_quic_core::packet::number::PacketNumberSpace,
            ) -> Result<(), s2n_quic_core::crypto::CryptoError> {
                self.0
                    .unprotect(ciphertext_sample, first, packet_number, space)
            }

            #[inline]
            fn protect(
                &self,
                sample: &[u8],
                first: &mut u8,
                packet_number: &mut [u8],
            ) -> Result<(), s2n_quic_core::crypto::CryptoError> {
                self.0.protect(sample, first, packet_number)
            }
        }
    };
}
