// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides a path specific MTU configuration.
//!
//! By default paths inherit the endpoint configured MTU values. Applications
//! should implement this provider to override the MTU configuration for
//! specific paths.

pub use s2n_quic_core::path::mtu::{Builder, Config, Endpoint, Inherit as Default, PathInfo};

pub trait Provider {
    type Config: 'static + Send + Endpoint;
    type Error: 'static + core::fmt::Display + Send + Sync;

    fn start(self) -> Result<Self::Config, Self::Error>;
}

impl_provider_utils!();

impl<T: 'static + Send + Endpoint> Provider for T {
    type Config = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Config, Self::Error> {
        Ok(self)
    }
}
