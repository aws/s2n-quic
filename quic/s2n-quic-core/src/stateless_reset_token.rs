//! Defines the Stateless Reset token

use core::convert::{TryFrom, TryInto};
use s2n_codec::{decoder_value, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
//# Stateless Reset {
//#   Fixed Bits (2) = 1,
//#   Unpredictable Bits (38..),
//#   Stateless Reset Token (128),
//# }

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#10.3
//# Endpoints MUST send stateless reset packets formatted as a packet
//# with a short header.

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
