use crate::{frame::Tag, varint::VarInt};
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.7
//# A server sends a NEW_TOKEN frame (type=0x07) to provide the client
//# with a token to send in the header of an Initial packet for a future
//# connection.

macro_rules! new_token_tag {
    () => {
        0x07u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.7
//# NEW_TOKEN Frame {
//#   Type (i) = 0x07,
//#   Token Length (i),
//#   Token (..),
//# }

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.7
//# NEW_TOKEN frames contain the following fields:
//#
//# Token Length:  A variable-length integer specifying the length of the
//#    token in bytes.
//#
//# Token:  An opaque blob that the client may use with a future Initial
//#    packet.  The token MUST NOT be empty.  An endpoint MUST treat
//#    receipt of a NEW_TOKEN frame with an empty Token field as a
//#    connection error of type FRAME_ENCODING_ERROR.

#[derive(Debug, PartialEq, Eq)]
pub struct NewToken<'a> {
    /// An opaque blob that the client may use with a future Initial packet.
    pub token: &'a [u8],
}

impl<'a> NewToken<'a> {
    pub const fn tag(&self) -> u8 {
        new_token_tag!()
    }
}

decoder_parameterized_value!(
    impl<'a> NewToken<'a> {
        fn decode(_tag: Tag, buffer: Buffer) -> Result<Self> {
            let (token, buffer) = buffer.decode_slice_with_len_prefix::<VarInt>()?;
            let token = token.into_less_safe_slice();

            let frame = NewToken { token };

            Ok((frame, buffer))
        }
    }
);

impl<'a> EncoderValue for NewToken<'a> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode_with_len_prefix::<VarInt, _>(&self.token);
    }
}
