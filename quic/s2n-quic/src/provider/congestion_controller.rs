pub use s2n_quic_core::{
    inet::SocketAddress,
    recovery::congestion_controller::{CongestionController, Endpoint, PathInfo},
};
pub use s2n_quic_transport::recovery::CubicCongestionController;

/// Provides congestion controller support for an endpoint
pub trait Provider {
    type Endpoint: Endpoint;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Endpoint, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    use s2n_quic_transport::recovery::CubicCongestionController;

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
        type CongestionController = CubicCongestionController;

        fn new_congestion_controller(
            &mut self,
            path_info: super::PathInfo,
        ) -> Self::CongestionController {
            CubicCongestionController::new(path_info.max_datagram_size)
        }
    }
}
