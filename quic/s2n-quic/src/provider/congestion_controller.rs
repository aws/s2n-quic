pub use s2n_quic_core::{
    inet::SocketAddress,
    recovery::congestion_controller::{CongestionController, Endpoint, PathInfo},
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
    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Endpoint = Endpoint;
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Endpoint, Self::Error> {
            Ok(Endpoint {})
        }
    }

    #[derive(Debug, Default)]
    pub struct Endpoint {}

    impl super::Endpoint for Endpoint {
        type CongestionController = CongestionController;

        fn new_congestion_controller(
            &mut self,
            _path_info: super::PathInfo,
        ) -> Self::CongestionController {
            CongestionController::default()
        }
    }

    #[derive(Clone, Debug, Default)]
    pub struct CongestionController {}

    impl super::CongestionController for CongestionController {
        // TODO implement callbacks once defined
    }
}
