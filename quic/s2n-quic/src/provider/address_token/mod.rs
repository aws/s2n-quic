// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides address validation token functionality to a QUIC endpoint.

pub use s2n_quic_core::token::{Context, Format};

pub trait Provider: 'static {
    type Format: 'static + Format;
    type Error: 'static + core::fmt::Display + Send + Sync;

    /// Starts the token provider
    fn start(self) -> Result<Self::Format, Self::Error>;
}

pub mod default;

pub use default::Provider as Default;

impl_provider_utils!();
