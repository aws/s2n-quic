//! Default provider for the endpoint limits.
//!

use s2n_quic_core::endpoint_limits::Limits;

pub trait Provider: 'static {
    type Limits: 'static + Limits;
    type Error: core::fmt::Display;

    /// Starts the token provider
    fn start(self) -> Result<Self::Limits, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    use s2n_quic_core::endpoint_limits;

    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Limits = Limits;
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Limits, Self::Error> {
            Ok(Limits::default())
        }
    }

    #[derive(Clone, Copy, Default)]
    pub struct Limits {}

    impl super::Limits for Limits {
        fn on_connection_attempt(
            &mut self,
            _info: &endpoint_limits::ConnectionAttempt,
        ) -> endpoint_limits::Outcome {
            endpoint_limits::Outcome::Allow
        }
    }
}
