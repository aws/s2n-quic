// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::frame::Tag;
use core::convert::TryInto;
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.17
//# Endpoints can use PATH_CHALLENGE frames (type=0x1a) to check
//# reachability to the peer and for path validation during connection
//# migration.

macro_rules! path_challenge_tag {
    () => {
        0x1au8
    };
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.17
//# PATH_CHALLENGE Frame {
//#   Type (i) = 0x1a,
//#   Data (64),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.17
//# PATH_CHALLENGE frames contain the following fields:
//#
//# Data:  This 8-byte field contains arbitrary data.

pub const DATA_LEN: usize = 8;

#[derive(Debug, PartialEq, Eq)]
pub struct PathChallenge<'a> {
    /// This 8-byte field contains arbitrary data.
    pub data: &'a [u8; DATA_LEN],
}

impl PathChallenge<'_> {
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

impl EncoderValue for PathChallenge<'_> {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode(&self.data.as_ref());
    }
}
