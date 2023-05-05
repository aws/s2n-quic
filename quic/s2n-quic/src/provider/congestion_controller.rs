// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use s2n_quic_core::{
    random::Generator,
    recovery::{
        congestion_controller::{CongestionController, Endpoint, PathInfo, Publisher},
        RttEstimator,
    },
    time::Timestamp,
};

/// Provides congestion controller support for an endpoint
pub trait Provider {
    type Endpoint: Endpoint;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Endpoint, Self::Error>;
}

pub use s2n_quic_core::recovery::{bbr::Endpoint as Bbr, cubic::Endpoint as Cubic};
pub type Default = Cubic;

impl_provider_utils!();

impl<T: Endpoint> Provider for T {
    type Endpoint = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Endpoint, Self::Error> {
        Ok(self)
    }
}
