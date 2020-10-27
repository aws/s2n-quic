mod sent_packets;
pub use sent_packets::*;

mod cubic;
mod hybrid_slow_start;
mod manager;

pub use cubic::*;
pub use manager::*;

/// re-export core
pub use s2n_quic_core::recovery::*;

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;

    #[derive(Debug)]
    pub struct Endpoint;

    impl congestion_controller::Endpoint for Endpoint {
        type CongestionController = CubicCongestionController;

        fn new_congestion_controller(
            &mut self,
            _: congestion_controller::PathInfo,
        ) -> Self::CongestionController {
            todo!()
        }
    }
}
