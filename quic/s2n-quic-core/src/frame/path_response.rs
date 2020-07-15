use crate::frame::Tag;
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.18
//# The PATH_RESPONSE frame (type=0x1b) is sent in response to a
//# PATH_CHALLENGE frame.  Its format is identical to the PATH_CHALLENGE
//# frame (Section 19.17).

macro_rules! path_response_tag {
    () => {
        0x1bu8
    };
}
use crate::frame::path_challenge::{PathChallenge, DATA_LEN};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.18
//# If the content of a PATH_RESPONSE frame does not match the content of
//# a PATH_CHALLENGE frame previously sent by the endpoint, the endpoint
//# MAY generate a connection error of type PROTOCOL_VIOLATION.

#[derive(Debug, PartialEq, Eq)]
pub struct PathResponse<'a> {
    /// This 8-byte field contains arbitrary data.
    pub data: &'a [u8; DATA_LEN],
}

impl<'a> PathResponse<'a> {
    pub const fn tag(&self) -> u8 {
        path_response_tag!()
    }
}

impl<'a> From<PathChallenge<'a>> for PathResponse<'a> {
    fn from(path_challenge: PathChallenge<'a>) -> Self {
        Self {
            data: path_challenge.data,
        }
    }
}

decoder_parameterized_value!(
    impl<'a> PathResponse<'a> {
        fn decode(_tag: Tag, buffer: Buffer) -> Result<Self> {
            let (path_challenge, buffer) =
                buffer.decode_parameterized::<PathChallenge>(path_challenge_tag!())?;
            Ok((path_challenge.into(), buffer))
        }
    }
);

impl<'a> EncoderValue for PathResponse<'a> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode(&self.data.as_ref());
    }
}
