//! Default provider for the endpoint limits.
//!

use s2n_quic_core::endpoint::LimitActions;

pub trait Provider: 'static {
    type Limits: 'static + LimitActions;
    type Error: core::fmt::Display;

    /// Starts the token provider
    fn start(self) -> Result<Self::Limits, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    use s2n_quic_core::endpoint;

    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Limits = LimitActions;
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Limits, Self::Error> {
            Ok(LimitActions::default())
        }
    }

    #[derive(Clone, Copy, Default)]
    pub struct LimitActions {}

    impl super::LimitActions for LimitActions {
        fn on_connection_attempt(&mut self, _info: &endpoint::ConnectionInfo) -> endpoint::Outcome {
            endpoint::Outcome::Allow
        }
    }
}
