// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use s2n_quic_core::recovery::{
    congestion_controller::{CongestionController, Endpoint, PathInfo},
    CubicCongestionController,
};

/// Provides congestion controller support for an endpoint
pub trait Provider {
    type Endpoint: Endpoint;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Endpoint, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    use s2n_quic_core::recovery::cubic::Endpoint;

    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Endpoint = Endpoint;
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Endpoint, Self::Error> {
            Ok(Endpoint::default())
        }
    }
}
