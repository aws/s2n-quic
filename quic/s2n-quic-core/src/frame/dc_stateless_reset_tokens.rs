// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{frame::ExtensionTag, stateless_reset, varint::VarInt};
use s2n_codec::{decoder_invariant, decoder_parameterized_value, Encoder, EncoderValue};

const TAG: VarInt = VarInt::from_u32(0xdc0000);

macro_rules! dc_stateless_reset_tokens_tag {
    () => {
        0xdc0000u64
    };
}

//# DC_STATELESS_RESET_TOKENS Frame {
//#   Type (i) = 0xdc0000,
//#   Length (i),
//#   Stateless Reset Tokens [(128)],
//# }

//# DC_STATELESS_RESET_TOKENS frames contain the following fields:
//#
//# Length: A variable-length integer specifying the length of the stateless
//#     reset tokens field in this DC_STATELESS_RESET_TOKENS frame.
//# Stateless Reset Tokens: 1 or more 128-bit values that will be used
//#     for a stateless reset of dc path secrets.

#[derive(Debug, PartialEq, Eq)]
pub struct DcStatelessResetTokens<'a> {
    /// 1 or more 128-bit values
    stateless_reset_tokens: &'a [u8],
}

impl<'a> DcStatelessResetTokens<'a> {
    pub const fn tag(&self) -> ExtensionTag {
        TAG
    }

    /// Constructs a new `DcStatelessResetTokens` frame with the given `stateless_reset_tokens`
    ///
    /// `Err` if the given `stateless_reset_tokens` is empty
    pub fn new(stateless_reset_tokens: &'a [stateless_reset::Token]) -> Result<Self, &'static str> {
        ensure!(
            !stateless_reset_tokens.is_empty(),
            Err("at least one stateless reset token is required")
        );

        unsafe {
            // Safety: `stateless_reset::Token` is a [u8; 16], so a slice of [u8; 16]s
            //         should be equivalent to &[u8] with length equal to the number
            //         of tokens multiplied by 16.
            Ok(Self {
                stateless_reset_tokens: core::slice::from_raw_parts(
                    stateless_reset_tokens.as_ptr() as _,
                    stateless_reset_tokens.len() * stateless_reset::token::LEN,
                ),
            })
        }
    }
}

impl<'a> IntoIterator for DcStatelessResetTokens<'a> {
    type Item = stateless_reset::Token;
    type IntoIter =
        core::iter::Map<core::slice::ChunksExact<'a, u8>, fn(&[u8]) -> stateless_reset::Token>;

    fn into_iter(self) -> Self::IntoIter {
        self.stateless_reset_tokens
            .chunks_exact(stateless_reset::token::LEN)
            .map(|item| {
                stateless_reset::Token::try_from(item).expect(
                    "each chunk has exactly chunk_size (stateless_reset::token::LEN) elements",
                )
            })
    }
}

decoder_parameterized_value!(
    impl<'a> DcStatelessResetTokens<'a> {
        fn decode(_tag: ExtensionTag, buffer: Buffer) -> Result<Self> {
            let (stateless_reset_tokens, buffer) =
                buffer.decode_slice_with_len_prefix::<VarInt>()?;
            let stateless_reset_tokens: &[u8] = stateless_reset_tokens.into_less_safe_slice();

            let len = stateless_reset_tokens.len();
            decoder_invariant!(len > 0, "at least one stateless token must be supplied");
            decoder_invariant!(
                len % stateless_reset::token::LEN == 0,
                "invalid dc stateless token length"
            );

            let frame = DcStatelessResetTokens {
                stateless_reset_tokens,
            };

            Ok((frame, buffer))
        }
    }
);

impl<'a> EncoderValue for DcStatelessResetTokens<'a> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&TAG);
        buffer.encode_with_len_prefix::<VarInt, _>(&self.stateless_reset_tokens);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        frame::{dc_stateless_reset_tokens::TAG, DcStatelessResetTokens, ExtensionTag},
        stateless_reset,
    };
    use s2n_codec::{DecoderBuffer, DecoderParameterizedValue, EncoderValue};

    #[test]
    fn round_trip() {
        let tokens: Vec<stateless_reset::Token> = vec![
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15].into(),
            [7, 7, 7, 3, 3, 3, 4, 4, 4, 8, 8, 8, 9, 9, 9, 9].into(),
        ];
        let frame = DcStatelessResetTokens::new(tokens.as_slice()).unwrap();
        let encoded = frame.encode_to_vec();

        let buffer = DecoderBuffer::new(encoded.as_slice());
        let (tag, buffer) = buffer.decode::<ExtensionTag>().expect("decoding succeeds");
        assert_eq!(TAG, tag);
        let (frame, remaining) =
            DcStatelessResetTokens::decode_parameterized(TAG, buffer).expect("decoding succeeds");
        assert!(remaining.is_empty());
        assert_eq!(
            stateless_reset::token::LEN * tokens.len(),
            frame.stateless_reset_tokens.len()
        );
        for (index, token) in frame.into_iter().enumerate() {
            assert_eq!(tokens[index], token);
        }
    }

    #[test]
    fn invalid_token_size() {
        let frame = DcStatelessResetTokens {
            stateless_reset_tokens: &[1, 2, 3],
        };
        let encoded = frame.encode_to_vec();

        let buffer = DecoderBuffer::new(encoded.as_slice());
        let (tag, buffer) = buffer.decode::<ExtensionTag>().expect("decoding succeeds");
        assert_eq!(TAG, tag);

        assert!(DcStatelessResetTokens::decode_parameterized(TAG, buffer).is_err());
    }

    #[test]
    fn zero_tokens() {
        let frame = DcStatelessResetTokens {
            stateless_reset_tokens: &[],
        };
        let encoded = frame.encode_to_vec();

        let buffer = DecoderBuffer::new(encoded.as_slice());
        let (tag, buffer) = buffer.decode::<ExtensionTag>().expect("decoding succeeds");
        assert_eq!(TAG, tag);

        assert!(DcStatelessResetTokens::decode_parameterized(TAG, buffer).is_err());

        assert!(DcStatelessResetTokens::new(&[]).is_err());
    }
}
