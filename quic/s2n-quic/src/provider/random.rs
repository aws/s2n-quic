pub use s2n_quic_core::random::Generator;

/// Provides random number generation support for an endpoint
pub trait Provider: 'static {
    type Generator: 'static + Generator;
    type Error: core::fmt::Display;

    fn start(self) -> Result<Self::Generator, Self::Error>;
}

cfg_if::cfg_if! {
    if #[cfg(feature = "rand")] {
        pub use thread_local::Provider as Default;
    } else {
        // TODO implement stub that panics
    }
}

impl_provider_utils!();

#[cfg(feature = "rand")]
pub mod thread_local {
    use core::convert::Infallible;
    use rand::prelude::*;
    use s2n_quic_core::random;

    #[derive(Debug, Default)]
    pub struct Provider(Generator);

    impl super::Provider for Provider {
        type Generator = Generator;
        type Error = Infallible;

        fn start(self) -> Result<Self::Generator, Self::Error> {
            Ok(self.0)
        }
    }

    impl super::TryInto for Generator {
        type Provider = Provider;
        type Error = Infallible;

        fn try_into(self) -> Result<Self::Provider, Self::Error> {
            Ok(Provider(self))
        }
    }

    /// Randomly generated bits.
    #[derive(Debug, Default)]
    pub struct Generator {}

    impl random::Generator for Generator {
        fn public_random_fill(&mut self, dest: &mut [u8]) {
            rand::thread_rng().fill_bytes(dest)
        }

        fn private_random_fill(&mut self, dest: &mut [u8]) {
            rand::thread_rng().fill_bytes(dest)
        }
    }

    #[cfg(test)]
    mod tests {
        use s2n_quic_core::random::Generator;

        #[test]
        fn generator_test() {
            let mut generator = super::Generator::default();

            let mut dest_1 = [0; 20];
            let mut dest_2 = [0; 20];

            generator.public_random_fill(&mut dest_1);
            generator.public_random_fill(&mut dest_2);

            assert_ne!(dest_1, dest_2);

            generator.private_random_fill(&mut dest_1);
            generator.private_random_fill(&mut dest_2);

            assert_ne!(dest_1, dest_2);
        }
    }
}
