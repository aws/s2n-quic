// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provider for formatting CONNECTION_CLOSE frames

pub use s2n_quic_core::connection::close::*;

/// Provider for formatting CONNECTION_CLOSE frames
pub trait Provider: 'static {
    type Formatter: 'static + Formatter;
    type Error: core::fmt::Display + Send + Sync;

    /// Starts the token provider
    fn start(self) -> Result<Self::Formatter, Self::Error>;
}

pub type Default = Production;

/// Implement Provider for all implementations of Formatter
impl<T: Formatter> Provider for T {
    type Formatter = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Formatter, Self::Error> {
        Ok(self)
    }
}

impl_provider_utils!();
