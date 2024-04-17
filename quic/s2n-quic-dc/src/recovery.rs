pub use s2n_quic_core::recovery::RttEstimator;

pub fn rtt_estimator() -> RttEstimator {
    RttEstimator::new(core::time::Duration::from_millis(10))
}
