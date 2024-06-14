// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides a path specific MTU configuration.

pub use s2n_quic_core::path::mtu::{Builder, Config, Endpoint, PathInfo};

pub trait Provider {
    type Config: 'static + Send + Endpoint;
    type Error: 'static + core::fmt::Display + Send + Sync;

    fn start(self) -> Result<Self::Config, Self::Error>;
}

pub use Config as Default;

impl_provider_utils!();

impl<T: 'static + Send + Endpoint> Provider for T {
    type Config = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Config, Self::Error> {
        Ok(self)
    }
}
