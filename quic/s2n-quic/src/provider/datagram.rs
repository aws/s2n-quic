// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides unreliable datagram support

pub use s2n_quic_core::datagram::default;
use s2n_quic_core::datagram::{traits::Endpoint, Disabled};

pub trait Provider {
    type Endpoint: Endpoint;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Endpoint, Self::Error>;
}

impl_provider_utils!();

pub type Default = Disabled;

impl<T: 'static + Send + Endpoint> Provider for T {
    type Endpoint = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Endpoint, Self::Error> {
        Ok(self)
    }
}
