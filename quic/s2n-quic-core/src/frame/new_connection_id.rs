// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{frame::Tag, varint::VarInt};
use core::{convert::TryInto, mem::size_of};
use s2n_codec::{decoder_invariant, decoder_parameterized_value, Encoder, EncoderValue};

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
//# An endpoint sends a NEW_CONNECTION_ID frame (type=0x18) to provide
//# its peer with alternative connection IDs that can be used to break
//# linkability when migrating connections; see Section 9.5.

macro_rules! new_connection_id_tag {
    () => {
        0x18u8
    };
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
//# NEW_CONNECTION_ID Frame {
//#   Type (i) = 0x18,
//#   Sequence Number (i),
//#   Retire Prior To (i),
//#   Length (8),
//#   Connection ID (8..160),
//#   Stateless Reset Token (128),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
//# NEW_CONNECTION_ID frames contain the following fields:
//#
//# Sequence Number:  The sequence number assigned to the connection ID
//#    by the sender, encoded as a variable-length integer; see
//#    Section 5.1.1.
//#
//# Retire Prior To:  A variable-length integer indicating which
//#    connection IDs should be retired; see Section 5.1.2.
//#
//# Length:  An 8-bit unsigned integer containing the length of the
//#    connection ID.  Values less than 1 and greater than 20 are invalid
//#    and MUST be treated as a connection error of type
//#    FRAME_ENCODING_ERROR.
//#
//# Connection ID:  A connection ID of the specified length.
//#
//# Stateless Reset Token:  A 128-bit value that will be used for a
//#    stateless reset when the associated connection ID is used; see
//#    Section 10.3.

pub const STATELESS_RESET_TOKEN_LEN: usize = size_of::<u128>();

#[derive(Debug, PartialEq, Eq)]
pub struct NewConnectionId<'a> {
    /// The sequence number assigned to the connection ID by the sender
    pub sequence_number: VarInt,

    /// A variable-length integer indicating which connection IDs
    /// should be retired
    pub retire_prior_to: VarInt,

    /// The new connection ID
    pub connection_id: &'a [u8],

    /// A 128-bit value that will be used for a stateless reset when
    /// the associated connection ID is used
    pub stateless_reset_token: &'a [u8; STATELESS_RESET_TOKEN_LEN],
}

impl NewConnectionId<'_> {
    pub const fn tag(&self) -> u8 {
        new_connection_id_tag!()
    }
}

decoder_parameterized_value!(
    impl<'a> NewConnectionId<'a> {
        fn decode(_tag: Tag, buffer: Buffer) -> Result<Self> {
            let (sequence_number, buffer) = buffer.decode()?;
            let (retire_prior_to, buffer) = buffer.decode()?;

            //= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
            //# The value in the Retire Prior To field
            //# MUST be less than or equal to the value in the Sequence Number field.

            //= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
            //# Receiving a value in the Retire Prior To field that is greater than
            //# that in the Sequence Number field MUST be treated as a connection
            //# error of type FRAME_ENCODING_ERROR.
            decoder_invariant!(
                retire_prior_to <= sequence_number,
                "invalid retire prior to value"
            );

            let (connection_id_len, buffer) = buffer.decode::<u8>()?;

            //= https://www.rfc-editor.org/rfc/rfc9000#section-19.15
            //# Values less than 1 and greater than 20 are invalid
            //# and MUST be treated as a connection error of type
            //# FRAME_ENCODING_ERROR.
            decoder_invariant!(
                (1..=20).contains(&connection_id_len),
                "invalid connection id length"
            );

            let (connection_id, buffer) = buffer.decode_slice(connection_id_len.into())?;
            let connection_id = connection_id.into_less_safe_slice();

            let (stateless_reset_token, buffer) = buffer.decode_slice(STATELESS_RESET_TOKEN_LEN)?;
            let stateless_reset_token: &[u8] = stateless_reset_token.into_less_safe_slice();
            let stateless_reset_token = stateless_reset_token
                .try_into()
                .expect("Length has been already verified");

            let frame = NewConnectionId {
                sequence_number,
                retire_prior_to,
                connection_id,
                stateless_reset_token,
            };

            Ok((frame, buffer))
        }
    }
);

impl EncoderValue for NewConnectionId<'_> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode(&self.sequence_number);
        buffer.encode(&self.retire_prior_to);
        buffer.encode_with_len_prefix::<u8, _>(&self.connection_id);
        buffer.encode(&self.stateless_reset_token.as_ref());
    }
}
