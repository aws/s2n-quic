// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{frame::Tag, stream::StreamType, varint::VarInt};
use s2n_codec::{decoder_invariant, decoder_parameterized_value, Encoder, EncoderValue};

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.14
//# A STREAMS_BLOCKED
//# frame of type 0x16 is used to indicate reaching the bidirectional
//# stream limit, and a STREAMS_BLOCKED frame of type 0x17 is used to
//# indicate reaching the unidirectional stream limit.

macro_rules! streams_blocked_tag {
    () => {
        0x16u8..=0x17u8
    };
}
const BIDIRECTIONAL_TAG: u8 = 0x16;
const UNIDIRECTIONAL_TAG: u8 = 0x17;

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.14
//# STREAMS_BLOCKED Frame {
//#   Type (i) = 0x16..0x17,
//#   Maximum Streams (i),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.14
//# STREAMS_BLOCKED frames contain the following field:
//#
//# Maximum Streams:  A variable-length integer indicating the maximum
//#    number of streams allowed at the time the frame was sent.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StreamsBlocked {
    pub stream_type: StreamType,

    /// A variable-length integer indicating the stream limit at the
    /// time the frame was sent.
    pub stream_limit: VarInt,
}

impl StreamsBlocked {
    pub fn tag(&self) -> u8 {
        match self.stream_type {
            StreamType::Bidirectional => BIDIRECTIONAL_TAG,
            StreamType::Unidirectional => UNIDIRECTIONAL_TAG,
        }
    }
}

decoder_parameterized_value!(
    impl<'a> StreamsBlocked {
        fn decode(tag: Tag, buffer: Buffer) -> Result<Self> {
            let stream_type = if BIDIRECTIONAL_TAG == tag {
                StreamType::Bidirectional
            } else {
                StreamType::Unidirectional
            };

            let (stream_limit, buffer) = buffer.decode::<VarInt>()?;

            //= https://www.rfc-editor.org/rfc/rfc9000#section-19.14
            //# This
            //# value cannot exceed 2^60, as it is not possible to encode stream
            //# IDs larger than 2^62-1.  Receipt of a frame that encodes a larger
            //# stream ID MUST be treated as a connection error of type
            //# STREAM_LIMIT_ERROR or FRAME_ENCODING_ERROR.
            decoder_invariant!(
                *stream_limit <= 2u64.pow(60),
                "maximum streams cannot exceed 2^60"
            );

            let frame = StreamsBlocked {
                stream_type,
                stream_limit,
            };

            Ok((frame, buffer))
        }
    }
);

impl EncoderValue for StreamsBlocked {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode(&self.stream_limit);
    }
}
