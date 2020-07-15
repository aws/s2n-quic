use crate::{frame::Tag, varint::VarInt};
use core::{convert::TryInto, mem::size_of};
use s2n_codec::{decoder_invariant, decoder_parameterized_value, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.15
//# An endpoint sends a NEW_CONNECTION_ID frame (type=0x18) to provide
//# its peer with alternative connection IDs that can be used to break
//# linkability when migrating connections (see Section 9.5).

macro_rules! new_connection_id_tag {
    () => {
        0x18u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.15
//# The NEW_CONNECTION_ID frame is as follows:
//#
//#  0                   1                   2                   3
//#  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                      Sequence Number (i)                    ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                      Retire Prior To (i)                    ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |   Length (8)  |                                               |
//# +-+-+-+-+-+-+-+-+       Connection ID (8..160)                  +
//# |                                                             ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                                                               |
//# +                                                               +
//# |                                                               |
//# +                   Stateless Reset Token (128)                 +
//# |                                                               |
//# +                                                               +
//# |                                                               |
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//# NEW_CONNECTION_ID frames contain the following fields:
//#
//# Sequence Number:  The sequence number assigned to the connection ID
//#    by the sender.  See Section 5.1.1.
//#
//# Retire Prior To:  A variable-length integer indicating which
//#    connection IDs should be retired.  See Section 5.1.2.
//#
//# Length:  An 8-bit unsigned integer containing the length of the
//#    connection ID.  Values less than 1 and greater than 20 are invalid
//#    and MUST be treated as a connection error of type
//#    PROTOCOL_VIOLATION.
//#
//# Connection ID:  A connection ID of the specified length.
//#
//# Stateless Reset Token:  A 128-bit value that will be used for a
//#    stateless reset when the associated connection ID is used (see
//#    Section 10.4).

const STATELESS_RESET_TOKEN_LEN: usize = size_of::<u128>();

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.15
//# An endpoint MUST NOT send this frame if it currently requires that
//# its peer send packets with a zero-length Destination Connection ID.
//# Changing the length of a connection ID to or from zero-length makes
//# it difficult to identify when the value of the connection ID changed.
//# An endpoint that is sending packets with a zero-length Destination
//# Connection ID MUST treat receipt of a NEW_CONNECTION_ID frame as a
//# connection error of type PROTOCOL_VIOLATION.
//#
//# Transmission errors, timeouts and retransmissions might cause the
//# same NEW_CONNECTION_ID frame to be received multiple times.  Receipt
//# of the same frame multiple times MUST NOT be treated as a connection
//# error.  A receiver can use the sequence number supplied in the
//# NEW_CONNECTION_ID frame to identify new connection IDs from old ones.
//#
//# If an endpoint receives a NEW_CONNECTION_ID frame that repeats a
//# previously issued connection ID with a different Stateless Reset
//# Token or a different sequence number, or if a sequence number is used
//# for different connection IDs, the endpoint MAY treat that receipt as
//# a connection error of type PROTOCOL_VIOLATION.
//#
//# The Retire Prior To field is a request for the peer to retire all
//# connection IDs with a sequence number less than the specified value.
//# This includes the initial and preferred_address transport parameter
//# connection IDs.  The peer SHOULD retire the corresponding connection
//# IDs and send the corresponding RETIRE_CONNECTION_ID frames in a
//# timely manner.
//#
//# The Retire Prior To field MUST be less than or equal to the Sequence
//# Number field.  Receiving a value greater than the Sequence Number
//# MUST be treated as a connection error of type PROTOCOL_VIOLATION.
//#
//# Once a sender indicates a Retire Prior To value, smaller values sent
//# in subsequent NEW_CONNECTION_ID frames have no effect.  A receiver
//# MUST ignore any Retire Prior To fields that do not increase the
//# largest received Retire Prior To value.

#[derive(Debug, PartialEq, Eq)]
pub struct NewConnectionID<'a> {
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

impl<'a> NewConnectionID<'a> {
    pub const fn tag(&self) -> u8 {
        new_connection_id_tag!()
    }
}

decoder_parameterized_value!(
    impl<'a> NewConnectionID<'a> {
        fn decode(_tag: Tag, buffer: Buffer) -> Result<Self> {
            let (sequence_number, buffer) = buffer.decode()?;
            let (retire_prior_to, buffer) = buffer.decode()?;

            let (connection_id_len, buffer) = buffer.decode::<u8>()?;

            decoder_invariant!(
                (1..20).contains(&connection_id_len),
                "invalid connection id length"
            );

            let (connection_id, buffer) = buffer.decode_slice(connection_id_len.into())?;
            let connection_id = connection_id.into_less_safe_slice();

            let (stateless_reset_token, buffer) = buffer.decode_slice(STATELESS_RESET_TOKEN_LEN)?;
            let stateless_reset_token: &[u8] = stateless_reset_token.into_less_safe_slice();
            let stateless_reset_token = stateless_reset_token
                .try_into()
                .expect("Length has been already verified");

            let frame = NewConnectionID {
                sequence_number,
                retire_prior_to,
                connection_id,
                stateless_reset_token,
            };

            Ok((frame, buffer))
        }
    }
);

impl<'a> EncoderValue for NewConnectionID<'a> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode(&self.sequence_number);
        buffer.encode(&self.retire_prior_to);
        buffer.encode_with_len_prefix::<u8, _>(&self.connection_id);
        buffer.encode(&self.stateless_reset_token.as_ref());
    }
}
