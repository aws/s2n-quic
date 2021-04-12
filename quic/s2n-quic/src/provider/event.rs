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

pub mod default {
    use super::*;

    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Subscriber = Subscriber;
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Subscriber, Self::Error> {
            Ok(Subscriber)
        }
    }

    pub struct Subscriber;

    impl super::Subscriber for Subscriber {
        fn on_version_information(&mut self, event: &events::VersionInformation) {
            // TODO log this event
            let _ = event;
        }
    }
}
