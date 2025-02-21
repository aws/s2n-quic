// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Provides limits support for a connection

pub use s2n_quic_core::connection::limits::{
    ConnectionInfo, Limiter, Limits, PostHandshakeInfo, UpdatableLimits,
};

pub trait Provider {
    type Limits: 'static + Send + Limiter;
    type Error: 'static + core::fmt::Display + Send + Sync;

    fn start(self) -> Result<Self::Limits, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

impl<T: 'static + Send + Limiter> Provider for T {
    type Limits = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Limits, Self::Error> {
        Ok(self)
    }
}

pub mod default {
    #[derive(Debug, Default)]
    pub struct Provider(());

    impl super::Provider for Provider {
        type Limits = super::Limits;
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Limits, Self::Error> {
            Ok(Self::Limits::default())
        }
    }
}
