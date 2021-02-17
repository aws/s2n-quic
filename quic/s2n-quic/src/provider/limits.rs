pub use s2n_quic_core::connection::limits::{ConnectionInfo, Limiter, Limits};

/// Provides limits support for an endpoint
pub trait Provider {
    type Limits: 'static + Send + Limiter;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Limits, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Limits = super::Limits;
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Limits, Self::Error> {
            Ok(Self::Limits::default())
        }
    }
}
