//! Defines the Stateless Reset token

use crate::connection::LocalId;
use core::convert::{TryFrom, TryInto};
use s2n_codec::{decoder_value, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
//# Stateless Reset {
//#   Fixed Bits (2) = 1,
//#   Unpredictable Bits (38..),
//#   Stateless Reset Token (128),
//# }

const STATELESS_RESET_TOKEN_LEN: usize = 128 / 8;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StatelessResetToken([u8; STATELESS_RESET_TOKEN_LEN]);

impl StatelessResetToken {
    /// A zeroed out stateless reset token
    pub const ZEROED: Self = Self([0; STATELESS_RESET_TOKEN_LEN]);
}

impl From<[u8; STATELESS_RESET_TOKEN_LEN]> for StatelessResetToken {
    fn from(bytes: [u8; STATELESS_RESET_TOKEN_LEN]) -> Self {
        Self(bytes)
    }
}

impl TryFrom<&[u8]> for StatelessResetToken {
    type Error = core::array::TryFromSliceError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let bytes = bytes.try_into()?;
        Ok(Self(bytes))
    }
}

impl AsRef<[u8]> for StatelessResetToken {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

decoder_value!(
    impl<'a> StatelessResetToken {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (value, buffer) = buffer.decode_slice(STATELESS_RESET_TOKEN_LEN)?;
            let value: &[u8] = value.into_less_safe_slice();
            let connection_id =
                StatelessResetToken::try_from(value).expect("slice len already verified");

            Ok((connection_id, buffer))
        }
    }
);

impl EncoderValue for StatelessResetToken {
    fn encoding_size(&self) -> usize {
        STATELESS_RESET_TOKEN_LEN
    }

    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.as_ref().encode(encoder)
    }
}

/// A generator for a stateless reset token
pub trait Generator {
    /// If enabled, a stateless reset packet containing the token generated
    /// by this Generator will be sent when a packet is received that cannot
    /// be matched to an existing connection. Otherwise, the packet will be
    /// dropped with no further action.
    const ENABLED: bool = true;

    /// Generates a stateless reset token.
    ///
    /// The stateless reset token MUST be difficult to guess.
    ///
    /// To enable stateless reset functionality, the stateless reset token must
    /// be generated the same for a given `LocalId` before and after loss of state.
    fn generate(&mut self, connection_id: &LocalId) -> StatelessResetToken;
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::{
        connection::LocalId,
        stateless_reset_token,
        stateless_reset_token::{StatelessResetToken, STATELESS_RESET_TOKEN_LEN},
    };

    pub const TEST_TOKEN_1: StatelessResetToken = StatelessResetToken {
        0: 11111111123456578987654321u128.to_be_bytes(),
    };
    pub const TEST_TOKEN_2: StatelessResetToken = StatelessResetToken {
        0: 222222222123456578987654321u128.to_be_bytes(),
    };
    pub const TEST_TOKEN_3: StatelessResetToken = StatelessResetToken {
        0: 333333333123456578987654321u128.to_be_bytes(),
    };
    pub const TEST_TOKEN_4: StatelessResetToken = StatelessResetToken {
        0: 444444444123456578987654321u128.to_be_bytes(),
    };

    const KEY: u8 = 123;

    #[derive(Debug, Default)]
    pub struct Generator();

    impl stateless_reset_token::Generator for Generator {
        fn generate(&mut self, connection_id: &LocalId) -> StatelessResetToken {
            let mut token = [0; STATELESS_RESET_TOKEN_LEN];

            for (index, byte) in connection_id.as_ref().iter().enumerate() {
                token[index] = byte ^ KEY;
            }

            token.into()
        }
    }
}
