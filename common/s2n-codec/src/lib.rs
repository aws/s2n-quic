// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![allow(unexpected_cfgs)]
#![cfg_attr(not(any(test, feature = "std")), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(any(feature = "testing", test))]
#[macro_use]
pub mod testing;

#[macro_use]
pub mod zerocopy;

pub mod decoder;
pub mod encoder;
pub mod unaligned;

pub use decoder::{
    CheckedRange, DecoderBuffer, DecoderBufferMut, DecoderBufferMutResult, DecoderBufferResult,
    DecoderError, DecoderParameterizedValue, DecoderParameterizedValueMut, DecoderValue,
    DecoderValueMut,
};
pub use encoder::{Encoder, EncoderBuffer, EncoderLenEstimator, EncoderValue};
pub use unaligned::*;
