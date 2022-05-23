// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Allows endpoints to subscribe to connection-level and endpoint-level events

use cfg_if::cfg_if;
pub use s2n_quic_core::{
    endpoint::Location,
    event::{
        api as events,
        api::{ConnectionInfo, ConnectionMeta},
        query, supervisor, Event, Meta, Subscriber, Timestamp,
    },
};

/// Provides event handling support for an endpoint
pub trait Provider {
    type Subscriber: 'static + Subscriber;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Subscriber, Self::Error>;
}

/// Provides an implementation to disable all events
pub mod disabled;

/// This module contains event integration with [`tracing`](https://docs.rs/tracing)
#[cfg(any(feature = "provider-event-tracing", test))]
pub mod tracing;

cfg_if! {
    if #[cfg(any(feature = "provider-event-tracing", test))] {
        pub use self::tracing as default;
    } else {
        // Events are disabled by default.
        pub use self::disabled as default;
    }
}

cfg_if! {
    if #[cfg(all(
        s2n_quic_unstable,
        feature = "unstable-provider-event-bpf"
    ))] {
        pub mod bpf;
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
