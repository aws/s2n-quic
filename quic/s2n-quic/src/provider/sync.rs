// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

/// Provides synchronization support for an endpoint
pub trait Provider {
    type Sync: 'static + Send;
    type Error: 'static + core::fmt::Display + Send + Sync;

    fn start(self) -> Result<Self::Sync, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Sync = (); // TODO
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Sync, Self::Error> {
            // TODO
            Ok(())
        }
    }
}
