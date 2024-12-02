// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::frame::Tag;
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.18
//# A PATH_RESPONSE frame (type=0x1b) is sent in response to a
//# PATH_CHALLENGE frame.

macro_rules! path_response_tag {
    () => {
        0x1bu8
    };
}
use crate::frame::path_challenge::{PathChallenge, DATA_LEN};

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.18
//# PATH_RESPONSE Frame {
//#   Type (i) = 0x1b,
//#   Data (64),
//# }

#[derive(Debug, PartialEq, Eq)]
pub struct PathResponse<'a> {
    /// This 8-byte field contains arbitrary data.
    pub data: &'a [u8; DATA_LEN],
}

impl PathResponse<'_> {
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

impl EncoderValue for PathResponse<'_> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode(&self.data.as_ref());
    }
}
