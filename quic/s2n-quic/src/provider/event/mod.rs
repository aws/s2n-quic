// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use cfg_if::cfg_if;
pub use s2n_quic_core::event::Subscriber;

/// Provides logging support for an endpoint
pub trait Provider {
    type Subscriber: 'static + Subscriber;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Subscriber, Self::Error>;
}

cfg_if! {
    if #[cfg(feature = "tracing")] {
        pub use self::tracing as default;
    } else {
        pub mod default {
            // TODO export stub implementation that panics on initialization
        }
    }
}

pub use default::Provider as Default;

#[cfg(feature = "tracing")]
pub mod tracing;

impl_provider_utils!();
