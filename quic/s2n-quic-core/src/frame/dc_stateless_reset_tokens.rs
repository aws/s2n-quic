// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{frame::ExtensionTag, stateless_reset, varint::VarInt};
use s2n_codec::{
    decoder_invariant,
    zerocopy::{AsBytes as _, FromBytes as _},
    DecoderError, Encoder, EncoderValue,
};

const TAG: VarInt = VarInt::from_u32(0xdc0000);

// The maximum number of stateless reset tokens that could fit in a maximum sized QUIC packet
// based on the maximum permitted UDP payload of 65527 and the long packet header size.
const MAX_STATELESS_RESET_TOKEN_COUNT: usize = 4092;

macro_rules! dc_stateless_reset_tokens_tag {
    () => {
        0xdc0000u64
    };
}

//# DC_STATELESS_RESET_TOKENS Frame {
//#   Type (i) = 0xdc0000,
//#   Count (i),
//#   Stateless Reset Tokens [(128)],
//# }

//# DC_STATELESS_RESET_TOKENS frames contain the following fields:
//#
//# Count: A variable-length integer specifying the number of stateless
//#     reset tokens in the frame.
//# Stateless Reset Tokens: 1 or more 128-bit values that will be used
//#     for a stateless reset of dc path secrets.

#[derive(Debug, PartialEq, Eq)]
pub struct DcStatelessResetTokens<'a> {
    /// 1 or more 128-bit values
    stateless_reset_tokens: &'a [stateless_reset::Token],
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

        ensure!(
            stateless_reset_tokens.len() <= MAX_STATELESS_RESET_TOKEN_COUNT,
            Err("too many stateless reset tokens")
        );

        Ok(Self {
            stateless_reset_tokens,
        })
    }
}

impl<'a> IntoIterator for DcStatelessResetTokens<'a> {
    type Item = &'a stateless_reset::Token;
    type IntoIter = core::slice::Iter<'a, stateless_reset::Token>;

    fn into_iter(self) -> Self::IntoIter {
        self.stateless_reset_tokens.iter()
    }
}

macro_rules! impl_decode_parameterized {
    ($slice_from_prefix:ident, $buffer:ident) => {{
        let (count, buffer) = $buffer.decode::<VarInt>()?;

        decoder_invariant!(
            count > VarInt::ZERO,
            "at least one stateless token must be supplied"
        );

        decoder_invariant!(
            count <= MAX_STATELESS_RESET_TOKEN_COUNT,
            "too many stateless reset tokens"
        );

        let count: usize = count
            .try_into()
            .expect("MAX_STATELESS_RESET_TOKEN_COUNT fits in usize");

        let buffer = buffer.into_less_safe_slice();
        let (stateless_reset_tokens, remaining) =
            stateless_reset::Token::$slice_from_prefix(buffer, count)
                .ok_or(DecoderError::InvariantViolation("invalid encoding"))?;

        let frame = DcStatelessResetTokens {
            stateless_reset_tokens,
        };

        Ok((frame, remaining.into()))
    }};
}

impl<'a> ::s2n_codec::DecoderParameterizedValue<'a> for DcStatelessResetTokens<'a> {
    type Parameter = ExtensionTag;

    #[inline]
    fn decode_parameterized(
        _tag: Self::Parameter,
        buffer: ::s2n_codec::DecoderBuffer<'a>,
    ) -> ::s2n_codec::DecoderBufferResult<'a, Self> {
        impl_decode_parameterized!(slice_from_prefix, buffer)
    }
}

impl<'a> ::s2n_codec::DecoderParameterizedValueMut<'a> for DcStatelessResetTokens<'a> {
    type Parameter = ExtensionTag;

    #[inline]
    fn decode_parameterized_mut(
        _tag: Self::Parameter,
        buffer: ::s2n_codec::DecoderBufferMut<'a>,
    ) -> ::s2n_codec::DecoderBufferMutResult<'a, Self> {
        impl_decode_parameterized!(mut_slice_from_prefix, buffer)
    }
}

impl EncoderValue for DcStatelessResetTokens<'_> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&TAG);
        let count =
            self.stateless_reset_tokens.len().try_into().expect(
                "count is limited to MAX_STATELESS_RESET_TOKEN_COUNT, which fits in VarInt",
            );
        buffer.encode::<VarInt>(&count);
        buffer.encode(&self.stateless_reset_tokens.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        frame::{
            dc_stateless_reset_tokens::{MAX_STATELESS_RESET_TOKEN_COUNT, TAG},
            DcStatelessResetTokens, ExtensionTag,
        },
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
        assert_eq!(tokens.len(), frame.stateless_reset_tokens.len());
        for (index, &token) in frame.into_iter().enumerate() {
            assert_eq!(tokens[index], token);
        }
    }

    #[test]
    fn invalid_token_size() {
        let frame = DcStatelessResetTokens {
            stateless_reset_tokens: &[[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15].into()],
        };
        let mut encoded = frame.encode_to_vec();
        encoded.truncate(encoded.len() - 1);

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

    #[test]
    fn maximum_sized_frame() {
        let tokens: Vec<stateless_reset::Token> =
            vec![stateless_reset::token::testing::TEST_TOKEN_1; MAX_STATELESS_RESET_TOKEN_COUNT];
        let frame = DcStatelessResetTokens::new(tokens.as_slice()).unwrap();
        let encoded = frame.encode_to_vec();

        let buffer = DecoderBuffer::new(encoded.as_slice());
        let (tag, buffer) = buffer.decode::<ExtensionTag>().expect("decoding succeeds");
        assert_eq!(TAG, tag);
        let (frame, remaining) =
            DcStatelessResetTokens::decode_parameterized(TAG, buffer).expect("decoding succeeds");
        assert!(remaining.is_empty());
        assert_eq!(tokens.len(), frame.stateless_reset_tokens.len());
    }

    #[test]
    fn too_many_tokens() {
        let frame = DcStatelessResetTokens {
            stateless_reset_tokens: &[stateless_reset::token::testing::TEST_TOKEN_1;
                MAX_STATELESS_RESET_TOKEN_COUNT + 1],
        };
        let encoded = frame.encode_to_vec();

        let buffer = DecoderBuffer::new(encoded.as_slice());
        let (tag, buffer) = buffer.decode::<ExtensionTag>().expect("decoding succeeds");
        assert_eq!(TAG, tag);

        assert!(DcStatelessResetTokens::decode_parameterized(TAG, buffer).is_err());

        assert!(DcStatelessResetTokens::new(
            &[stateless_reset::token::testing::TEST_TOKEN_1; MAX_STATELESS_RESET_TOKEN_COUNT + 1]
        )
        .is_err());
    }
}
