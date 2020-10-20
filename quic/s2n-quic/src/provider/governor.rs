//! Default provider for the endpoint governor.
//!

use s2n_quic_core::endpoint::Format;

pub trait Provider: 'static {
    type Format: 'static + Format;
    type Error: core::fmt::Display;

    /// Starts the token provider
    fn start(self) -> Result<Self::Format, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    use s2n_quic_core::endpoint;

    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Format = Format;
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Format, Self::Error> {
            Ok(Format::default())
        }
    }

    #[derive(Clone, Copy, Default)]
    pub struct Format {}

    impl super::Format for Format {
        fn on_connection_attempt(&mut self, _info: &endpoint::ConnectionInfo) -> endpoint::Outcome {
            endpoint::Outcome::Allow
        }
    }
}
