// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{aws_lc_aead as aead, hkdf};
use core::fmt;
use s2n_quic_core::crypto::{self, HeaderProtectionMask};

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
}

impl HeaderKey {
    pub fn new<const KEY_LEN: usize>(
        secret: &hkdf::Prk,
        label: &[u8],
        alg: &'static aead::quic::Algorithm,
    ) -> Self {
        let mut bytes = zeroize::Zeroizing::new([0u8; KEY_LEN]);

        secret
            .expand(&[label], alg)
            .expect("label size verified")
            .fill(bytes.as_mut())
            .expect("fill size verified");

        let key = aead::quic::HeaderProtectionKey::new(alg, bytes.as_ref())
            .expect("header secret length already checked");
        Self(key)
    }

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

impl From<aead::quic::HeaderProtectionKey> for HeaderKey {
    fn from(key: aead::quic::HeaderProtectionKey) -> Self {
        Self(key)
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
        }

        impl From<crate::header_key::HeaderKeyPair> for $name {
            fn from(key: crate::header_key::HeaderKeyPair) -> Self {
                Self(key)
            }
        }
    };
}
