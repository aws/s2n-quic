pub use s2n_quic_core::stateless_reset::UnpredictableBits;

/// Provides stateless reset unpredictable bits support for an endpoint
pub trait Provider: 'static {
    type UnpredictableBits: 'static + UnpredictableBits;
    type Error: core::fmt::Display;

    fn start(self) -> Result<Self::UnpredictableBits, Self::Error>;
}

cfg_if::cfg_if! {
    if #[cfg(feature = "rand")] {
        pub use random::Provider as Default;
    } else {
        // TODO implement stub that panics
    }
}

impl_provider_utils!();

#[cfg(feature = "rand")]
pub mod random {
    use core::convert::Infallible;
    use rand::prelude::*;
    use s2n_quic_core::stateless_reset;

    #[derive(Debug, Default)]
    pub struct Provider(Generator);

    impl super::Provider for Provider {
        type UnpredictableBits = Generator;
        type Error = Infallible;

        fn start(self) -> Result<Self::UnpredictableBits, Self::Error> {
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

    impl stateless_reset::UnpredictableBits for Generator {
        fn fill(&mut self, dest: &mut [u8]) {
            rand::thread_rng().fill_bytes(dest)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use s2n_quic_core::stateless_reset::UnpredictableBits;

        #[test]
        fn unpredictable_bits_test() {
            let mut generator = Generator::default();

            let mut dest_1 = [0; 20];
            let mut dest_2 = [0; 20];

            generator.fill(&mut dest_1);
            generator.fill(&mut dest_2);

            assert_ne!(dest_1, dest_2);
        }
    }
}
