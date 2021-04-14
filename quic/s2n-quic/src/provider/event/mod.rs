// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use s2n_quic_core::event::*;
pub mod default;

/// Provides logging support for an endpoint
pub trait Provider {
    type Subscriber: 'static + Subscriber;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Subscriber, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();
