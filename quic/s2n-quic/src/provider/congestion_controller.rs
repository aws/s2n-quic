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
pub type Default = Cubic;

impl_provider_utils!();

impl<T: Endpoint> Provider for T {
    type Endpoint = T;
    type Error = core::convert::Infallible;

    fn start(self) -> Result<Self::Endpoint, Self::Error> {
        Ok(self)
    }
}

pub mod cubic {
    use super::Cubic;
    use s2n_quic_core::recovery::cubic::{ApplicationSettings, Builder as EndpointBuilder};

    #[derive(Default)]
    pub struct Builder {
        cwnd: Option<u32>,
    }

    impl Builder {
        /// Set the initial congestion window.
        pub fn with_congestion_window(mut self, cwnd: u32) -> Self {
            self.cwnd = Some(cwnd);
            self
        }

        pub fn build(self) -> Cubic {
            let app_settings = ApplicationSettings { cwnd: self.cwnd };
            EndpointBuilder::build_with(app_settings)
        }
    }
}

pub mod bbr {
    use super::Bbr;
    use s2n_quic_core::recovery::bbr::{ApplicationSettings, Builder as EndpointBuilder};

    #[derive(Debug, Default)]
    pub struct Builder {
        cwnd: Option<u32>,
        cwnd_gain: Option<u32>,
        loss_threshold: Option<u32>,
        up_pacing_gain: Option<u32>,
    }

    impl Builder {
        /// Set the initial congestion window.
        pub fn with_congestion_window(mut self, cwnd: u32) -> Self {
            self.cwnd = Some(cwnd);
            self
        }

        /// The dynamic gain factor used to scale the estimated BDP to produce a congestion window (cwnd)
        #[cfg(feature = "unstable-congestion-controller")]
        pub fn with_congestion_window_gain(mut self, cwnd_gain: u32) -> Self {
            self.cwnd_gain = Some(cwnd_gain);
            self
        }

        /// Set the gain factor used during the ProbeBW_UP phase of the BBR algorithm.
        #[cfg(feature = "unstable-congestion-controller")]
        pub fn with_up_pacing_gain(mut self, up_pacing_gain: u32) -> Self {
            self.up_pacing_gain = Some(up_pacing_gain);
            self
        }

        /// The maximum tolerated per-round-trip packet loss rate when probing for bandwidth.
        #[cfg(feature = "unstable-congestion-controller")]
        pub fn with_loss_threshold(mut self, loss_threshold: u32) -> Self {
            self.loss_threshold = Some(loss_threshold);
            self
        }

        pub fn build(self) -> Bbr {
            let app_settings = ApplicationSettings {
                cwnd: self.cwnd,
                cwnd_gain: self.cwnd_gain,
                loss_threshold: self.loss_threshold,
                up_pacing_gain: self.up_pacing_gain,
            };
            EndpointBuilder::build_with(app_settings)
        }
    }
}
