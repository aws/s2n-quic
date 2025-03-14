// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines the Stateless Reset token

use s2n_codec::{
    decoder_value,
    zerocopy::{FromBytes, Immutable, IntoBytes, Unaligned},
    Encoder, EncoderValue,
};
use subtle::ConstantTimeEq;

//= https://www.rfc-editor.org/rfc/rfc9000#section-10.3
//# Stateless Reset {
//#   Fixed Bits (2) = 1,
//#   Unpredictable Bits (38..),
//#   Stateless Reset Token (128),
//# }
pub const LEN: usize = 128 / 8;

// The implemented PartialEq will have the same results as
// a derived version, except it is constant-time. Therefore
// Hash can still be derived.
#[allow(clippy::derived_hash_with_manual_eq)]
#[derive(Copy, Clone, Debug, Eq, Hash, FromBytes, IntoBytes, Unaligned, Immutable)]
#[cfg_attr(
    any(test, feature = "generator"),
    derive(bolero_generator::TypeGenerator)
)]
#[repr(C)]
pub struct Token([u8; LEN]);

impl Token {
    /// A zeroed out stateless reset token
    pub const ZEROED: Self = Self([0; LEN]);

    /// Unwraps this token, returning the underlying array
    pub fn into_inner(self) -> [u8; LEN] {
        self.0
    }
}

impl From<[u8; LEN]> for Token {
    fn from(bytes: [u8; LEN]) -> Self {
        Self(bytes)
    }
}

impl TryFrom<&[u8]> for Token {
    type Error = core::array::TryFromSliceError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let bytes = bytes.try_into()?;
        Ok(Self(bytes))
    }
}

impl AsRef<[u8]> for Token {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

impl AsMut<[u8]> for Token {
    fn as_mut(&mut self) -> &mut [u8] {
        self.0.as_mut()
    }
}

impl PartialEq for Token {
    //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.1
    //# When comparing a datagram to stateless reset token values, endpoints
    //# MUST perform the comparison without leaking information about the
    //# value of the token.
    fn eq(&self, other: &Self) -> bool {
        self.0.ct_eq(&other.0).into()
    }
}

decoder_value!(
    impl<'a> Token {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (value, buffer) = buffer.decode_slice(LEN)?;
            let value: &[u8] = value.into_less_safe_slice();
            let token = Token::try_from(value).expect("slice len already verified");

            Ok((token, buffer))
        }
    }
);

impl EncoderValue for Token {
    fn encoding_size(&self) -> usize {
        LEN
    }

    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.as_ref().encode(encoder)
    }
}

// A generator for a stateless reset token
pub trait Generator: 'static + Send {
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
    /// be generated the same for a given `local_connection_id` before and after loss of state.
    fn generate(&mut self, local_connection_id: &[u8]) -> Token;
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use crate::{
        stateless_reset,
        stateless_reset::token::{Token, LEN},
    };

    pub const TEST_TOKEN_1: Token = Token(11111111123456578987654321u128.to_be_bytes());
    pub const TEST_TOKEN_2: Token = Token(222222222123456578987654321u128.to_be_bytes());
    pub const TEST_TOKEN_3: Token = Token(333333333123456578987654321u128.to_be_bytes());
    pub const TEST_TOKEN_4: Token = Token(444444444123456578987654321u128.to_be_bytes());

    const KEY: u8 = 123;

    #[derive(Debug, Default)]
    pub struct Generator();

    impl stateless_reset::token::Generator for Generator {
        fn generate(&mut self, connection_id: &[u8]) -> Token {
            let mut token = [0; LEN];

            for (index, byte) in connection_id.as_ref().iter().enumerate() {
                token[index] = byte ^ KEY;
            }

            token.into()
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::stateless_reset::token::{testing::TEST_TOKEN_1, LEN};

    //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.1
    //= type=test
    //# When comparing a datagram to stateless reset token values, endpoints
    //# MUST perform the comparison without leaking information about the
    //# value of the token.
    #[test]
    fn equality_test() {
        let token_1 = TEST_TOKEN_1;
        let token_2 = TEST_TOKEN_1;

        assert_eq!(token_1, token_2);

        for i in 0..LEN {
            let mut token = TEST_TOKEN_1;
            token.0[i] = !TEST_TOKEN_1.0[i];

            assert_ne!(TEST_TOKEN_1, token);
        }
    }
}
