/// Provides logging support for an endpoint
pub trait Provider {
    type Log: 'static + Send;
    type Error: 'static + core::fmt::Display;

    fn start(self) -> Result<Self::Log, Self::Error>;
}

pub use default::Provider as Default;

impl_provider_utils!();

pub mod default {
    #[derive(Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Log = (); // TODO
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Log, Self::Error> {
            // TODO
            Ok(())
        }
    }
}
