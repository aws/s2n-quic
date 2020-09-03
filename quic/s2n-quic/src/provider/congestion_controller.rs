/// Provides congestion controller support for an endpoint
pub trait Provider {
    type CongestionController: 'static + Send;
    type Error: core::fmt::Display;

    fn start(self) -> Result<Self::CongestionController, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type CongestionController = (); // TODO
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::CongestionController, Self::Error> {
            // TODO
            Ok(())
        }
    }
}
