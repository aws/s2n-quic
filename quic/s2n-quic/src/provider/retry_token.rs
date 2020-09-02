/// Provides retry token support for an endpoint
pub trait Provider {
    type RetryToken: 'static + Send;
    type Error: core::fmt::Display;

    fn start(self) -> Result<Self::RetryToken, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type RetryToken = (); // TODO
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::RetryToken, Self::Error> {
            // TODO
            Ok(())
        }
    }
}
