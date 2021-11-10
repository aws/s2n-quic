// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod x86;

#[cfg(any(test, feature = "ring"))]
mod ring;

pub mod generic;
pub mod payload;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

#[derive(Clone, Copy, Debug, Default)]
pub struct Error;

pub const NONCE_LEN: usize = 12;

pub use crate::ghash::TAG_LEN;

pub trait AesGcm {
    fn encrypt(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        payload: &mut [u8],
        tag: &mut [u8; TAG_LEN],
    );

    fn decrypt(
        &self,
        nonce: &[u8; NONCE_LEN],
        aad: &[u8],
        payload: &mut [u8],
        tag: &[u8; TAG_LEN],
    ) -> Result<(), Error>;
}

#[cfg(test)]
mod tests;
