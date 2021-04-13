// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use s2n_quic_core::event::*;

/// Provides logging support for an endpoint
pub trait Provider {
    type Subscriber: 'static + Subscriber;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Subscriber, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

#[cfg(feature = "tracing")]
pub mod default {
    use super::*;
    use tracing::info;

    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Subscriber = TracingSubscriber;
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Subscriber, Self::Error> {
            Ok(TracingSubscriber)
        }
    }

    pub struct TracingSubscriber;

    // TODO we should implement Display for Events or maybe opt into serde as a feature
    impl super::Subscriber for TracingSubscriber {
        fn on_version_information(&mut self, event: &events::VersionInformation) {
            info!("{:?}", event);
        }

        fn on_alpn_information(&mut self, event: &events::AlpnInformation) {
            info!("{:?}", event);
        }
    }
}

