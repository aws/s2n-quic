// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

// this functionality isn't public but should be assumed that it
// eventually will be
#[allow(unused_imports)]
pub use s2n_quic_core::path::migration::{
    default::{self, Validator as Default},
    disabled, Attempt, Outcome, Validator,
};

/// Provides limits support for an endpoint
pub trait Provider {
    type Validator: 'static + Send + Validator;
    type Error: 'static + core::fmt::Display + Send + Sync;

    fn start(self) -> Result<Self::Validator, Self::Error>;
}

impl_provider_utils!();

impl<T: 'static + Send + Validator> Provider for T {
    type Validator = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Validator, Self::Error> {
        Ok(self)
    }
}
