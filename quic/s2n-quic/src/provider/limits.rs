/// Provides limits support for an endpoint
pub trait Provider {
    type Limits: 'static + Send;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Limits, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Limits = (); // TODO
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Limits, Self::Error> {
            // TODO
            Ok(())
        }
    }
}
