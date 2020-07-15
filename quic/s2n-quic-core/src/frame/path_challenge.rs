use crate::frame::Tag;
use core::convert::TryInto;
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.17
//# Endpoints can use PATH_CHALLENGE frames (type=0x1a) to check
//# reachability to the peer and for path validation during connection
//# migration.

macro_rules! path_challenge_tag {
    () => {
        0x1au8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.17
//# The PATH_CHALLENGE frames are as follows:
//#
//#  0                   1                   2                   3
//#  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                                                               |
//# +                           Data (64)                           +
//# |                                                               |
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//# PATH_CHALLENGE frames contain the following fields:
//#
//# Data:  This 8-byte field contains arbitrary data.

pub(crate) const DATA_LEN: usize = 8;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.17
//# A PATH_CHALLENGE frame containing 8 bytes that are hard to guess is
//# sufficient to ensure that it is easier to receive the packet than it
//# is to guess the value correctly.
//#
//# The recipient of this frame MUST generate a PATH_RESPONSE frame
//# (Section 19.18) containing the same Data.

#[derive(Debug, PartialEq, Eq)]
pub struct PathChallenge<'a> {
    /// This 8-byte field contains arbitrary data.
    pub data: &'a [u8; DATA_LEN],
}

impl<'a> PathChallenge<'a> {
    pub const fn tag(&self) -> u8 {
        path_challenge_tag!()
    }
}

decoder_parameterized_value!(
    impl<'a> PathChallenge<'a> {
        fn decode(_tag: Tag, buffer: Buffer) -> Result<Self> {
            let (data, buffer) = buffer.decode_slice(DATA_LEN)?;
            let data: &[u8] = data.into_less_safe_slice();

            let data = data.try_into().expect("Length has been already verified");

            let frame = PathChallenge { data };

            Ok((frame, buffer))
        }
    }
);

impl<'a> EncoderValue for PathChallenge<'a> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode(&self.data.as_ref());
    }
}
