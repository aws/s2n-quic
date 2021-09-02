// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use cfg_if::cfg_if;
pub use s2n_quic_core::event::{
    api as events, api::ConnectionMeta, query, Event, Meta, Subscriber, Timestamp,
};

/// Provides logging support for an endpoint
pub trait Provider {
    type Subscriber: 'static + Subscriber;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Subscriber, Self::Error>;
}

/// Provides an implementation to disable all logging
pub mod disabled;

cfg_if! {
    if #[cfg(feature = "tracing-provider")] {
        pub use self::tracing as default;
        pub mod tracing;
    } else {
        pub use self::disabled as default;
    }
}

pub use default::Provider as Default;

impl<S> Provider for S
where
    S: 'static + Subscriber,
{
    type Subscriber = S;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<S, Self::Error> {
        Ok(self)
    }
}

impl_provider_utils!();
