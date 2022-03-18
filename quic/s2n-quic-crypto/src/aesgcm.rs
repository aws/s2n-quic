// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
mod x86;

mod ring;

pub mod generic;
pub mod payload;

#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub const NONCE_LEN: usize = 12;

pub use crate::ghash::TAG_LEN;

#[cfg(test)]
mod tests;
