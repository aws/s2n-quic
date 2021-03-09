// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(feature = "checked_range_unsafe"), forbid(unsafe_code))]
#![cfg_attr(not(any(test, feature = "std")), no_std)]

#[cfg(any(feature = "testing", test))]
#[macro_use]
pub mod testing;

#[macro_use]
pub mod zerocopy;

pub mod decoder;
pub mod encoder;
pub mod unaligned;

pub use decoder::*;
pub use encoder::*;
pub use unaligned::*;
