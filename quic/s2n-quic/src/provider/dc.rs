// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides dc support

mod confirm;

use s2n_quic_core::dc::Disabled;

// these imports are only accessible if the unstable feature is enabled
#[allow(unused_imports)]
pub use confirm::ConfirmComplete;
#[allow(unused_imports)]
pub use s2n_quic_core::dc::{ApplicationParams, ConnectionInfo, Endpoint, Path};

pub trait Provider {
    type Endpoint: Endpoint;
    type Error: 'static + core::fmt::Display + Send + Sync;

    /// Starts the dc provider
    fn start(self) -> Result<Self::Endpoint, Self::Error>;
}

// This provider is disabled by default
pub type Default = Disabled;

impl_provider_utils!();

impl<T: 'static + Send + Endpoint> Provider for T {
    type Endpoint = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Endpoint, Self::Error> {
        Ok(self)
    }
}
