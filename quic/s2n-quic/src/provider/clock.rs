pub use s2n_quic_core::time::Clock;

pub trait Provider: 'static {
    type Clock: 'static + Clock + Send;
    type Error: core::fmt::Display;

    fn start(self) -> Result<Self::Clock, Self::Error>;
}

pub use platform::Provider as Default;

impl_provider_utils!();

mod platform {
    use s2n_quic_core::time::Timestamp;
    use s2n_quic_platform::time::now;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct Provider;

    impl super::Provider for Provider {
        type Clock = Clock;
        type Error = core::convert::Infallible;

        fn start(self) -> Result<Self::Clock, Self::Error> {
            Ok(Clock)
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct Clock;

    impl super::Clock for Clock {
        fn get_time(&self) -> Timestamp {
            now()
        }
    }
}
