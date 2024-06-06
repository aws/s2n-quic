// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use cfg_if::cfg_if;
pub use s2n_quic_core::recovery::congestion_controller::Endpoint;

/// Provides congestion controller support for an endpoint
pub trait Provider {
    type Endpoint: Endpoint;
    type Error: 'static + core::fmt::Display + Send + Sync;

    fn start(self) -> Result<Self::Endpoint, Self::Error>;
}

cfg_if! {
    if #[cfg(feature = "unstable-congestion-controller")] {
        // Export the types needed to implement the CongestionController trait
        pub use s2n_quic_core::{
            random::Generator as RandomGenerator,
            recovery::{congestion_controller::{CongestionController, PathInfo, Publisher}, RttEstimator},
            time::Timestamp,
        };
    }
}

pub use s2n_quic_core::recovery::{bbr::Endpoint as Bbr, cubic::Endpoint as Cubic};
// Build congestion controllers with application provided overrides
pub use s2n_quic_core::recovery::{bbr::builder as bbr, cubic::builder as cubic};
pub type Default = Cubic;

impl_provider_utils!();

impl<T: Endpoint> Provider for T {
    type Endpoint = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Endpoint, Self::Error> {
        Ok(self)
    }
}
