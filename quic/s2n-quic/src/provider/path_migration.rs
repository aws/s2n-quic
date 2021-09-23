// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use cfg_if::cfg_if;

pub use s2n_quic_core::path::migration::{Attempt, Outcome, Validator};

/// Provides limits support for an endpoint
pub trait Provider {
    type Validator: 'static + Send + Validator;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Validator, Self::Error>;
}

cfg_if! {
    if #[cfg(feature = "connection-migration")] {
        pub use s2n_quic_core::path::migration::default;
    } else {
        pub use s2n_quic_core::path::migration::disabled as default;
    }
}

pub use default::Validator as Default;

impl_provider_utils!();

impl<T: 'static + Send + Validator> Provider for T {
    type Validator = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Validator, Self::Error> {
        Ok(self)
    }
}
