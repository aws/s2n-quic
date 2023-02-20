// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

pub use s2n_quic_core::random::Generator;

/// Provides random number generation support for an endpoint
pub trait Provider: 'static {
    type Generator: 'static + Generator;
    type Error: core::fmt::Display;

    fn start(self) -> Result<Self::Generator, Self::Error>;
}

pub use self::rand::Provider as Default;

impl_provider_utils!();

mod rand {
    use core::convert::Infallible;
    use rand::{
        prelude::*,
        rngs::{adapter::ReseedingRng, OsRng},
    };
    use rand_chacha::ChaChaCore;
    use s2n_quic_core::random;

    // Number of generated bytes after which to reseed the public and private random
    // generators. This value is based on THREAD_RNG_RESEED_THRESHOLD from rand::rngs::thread.rs
    const RESEED_THRESHOLD: u64 = 1024 * 64;

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
    #[derive(Debug)]
    pub struct Generator {
        public: ReseedingRng<ChaChaCore, OsRng>,
        private: ReseedingRng<ChaChaCore, OsRng>,
    }

    impl Default for Generator {
        fn default() -> Self {
            Self {
                public: build_rng(),
                private: build_rng(),
            }
        }
    }

    // Constructs a `ReseedingRng` with a ChaCha RNG initially seeded from the OS,
    // that will reseed from the OS after RESEED_THRESHOLD is exceeded
    fn build_rng() -> ReseedingRng<ChaChaCore, OsRng> {
        let prng = ChaChaCore::from_rng(OsRng::default())
            .unwrap_or_else(|err| panic!("could not initialize random generator: {err}"));
        ReseedingRng::new(prng, RESEED_THRESHOLD, OsRng::default())
    }

    impl random::Generator for Generator {
        fn public_random_fill(&mut self, dest: &mut [u8]) {
            self.public.fill_bytes(dest)
        }

        fn private_random_fill(&mut self, dest: &mut [u8]) {
            self.private.fill_bytes(dest)
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
