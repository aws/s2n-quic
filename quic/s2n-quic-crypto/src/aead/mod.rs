// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::crypto::{packet_protection::Error, scatter};

mod default;
#[cfg(feature = "fips")]
pub mod fips;

pub type Result<T = (), E = Error> = core::result::Result<T, E>;

pub trait Aead {
    type Nonce;
    type Tag;

    fn encrypt(&mut self, nonce: &Self::Nonce, aad: &[u8], payload: &mut scatter::Buffer)
        -> Result;

    fn decrypt(
        &self,
        nonce: &Self::Nonce,
        aad: &[u8],
        payload: &mut [u8],
        tag: &Self::Tag,
    ) -> Result;
}
