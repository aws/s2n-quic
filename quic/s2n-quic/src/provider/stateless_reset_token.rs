pub use s2n_quic_core::stateless_reset_token::Generator;

/// Provides stateless reset token support for an endpoint
pub trait Provider: 'static {
    type Generator: 'static + Generator;
    type Error: core::fmt::Display;

    fn start(self) -> Result<Self::Generator, Self::Error>;
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
    use s2n_quic_core::{
        connection, frame::new_connection_id::STATELESS_RESET_TOKEN_LEN, stateless_reset_token,
        stateless_reset_token::StatelessResetToken,
    };

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

    /// Randomly generated stateless reset token.
    #[derive(Debug, Default)]
    pub struct Generator {}

    impl stateless_reset_token::Generator for Generator {
        /// Since a random stateless reset token will not be recognized by the peer, this generator
        /// is not enabled and no stateless reset packet will be sent to the peer.
        const ENABLED: bool = false;
        /// This stateless reset token generator produces a random token on each call, and
        /// thus does not enable stateless reset functionality, as the token provided to the
        /// peer with a new connection ID will be different than the token sent in a stateless
        /// reset. To enable stateless reset functionality, the stateless reset token must
        /// be generated the same for a given `LocalId` before and after loss of state.
        fn generate(&mut self, _connection_id: &connection::LocalId) -> StatelessResetToken {
            let mut token = [0u8; STATELESS_RESET_TOKEN_LEN];
            rand::thread_rng().fill_bytes(&mut token);
            token.into()
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use s2n_quic_core::stateless_reset_token::Generator as _;

        #[test]
        fn stateless_reset_token_test() {
            let mut generator = Generator::default();
            let id = connection::LocalId::try_from_bytes(b"id01").unwrap();

            let token_1 = generator.generate(&id);
            let token_2 = generator.generate(&id);

            assert_ne!(token_1, token_2);
        }
    }
}
