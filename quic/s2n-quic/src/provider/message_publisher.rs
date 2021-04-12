// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use crate::message::Publisher;

pub use s2n_quic_core::connection::limits::{ConnectionInfo, Limiter, Limits};

/// Provides logging support for an endpoint
pub trait Provider {
    type Publisher: 'static + Publisher;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Publisher, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    pub use crate::message::event::*;

    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Publisher = Publisher;
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Publisher, Self::Error> {
            Ok(Publisher)
        }
    }

    pub struct Publisher;

    impl super::Publisher for Publisher {
        fn on_version_information(&self, event: &VersionInformation) {
            // TODO log this event
            let _ = event;
        }
    }
}
