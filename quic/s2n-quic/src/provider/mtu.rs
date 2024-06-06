// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides MTU configuration at the connection level.

pub use s2n_quic_core::path::mtu::{Config, Configurator, ConnectionInfo};

pub trait Provider {
    type Config: 'static + Send + Configurator;
    type Error: 'static + core::fmt::Display + Send + Sync;

    fn start(self) -> Result<Self::Config, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

impl<T: 'static + Send + Configurator> Provider for T {
    type Config = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Config, Self::Error> {
        Ok(self)
    }
}

pub mod default {
    pub use s2n_quic_core::path::mtu::Builder;

    #[derive(Debug, Default)]
    pub struct Provider(());

    impl super::Provider for Provider {
        type Config = super::Config;
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Config, Self::Error> {
            Ok(Self::Config::default())
        }
    }
}
