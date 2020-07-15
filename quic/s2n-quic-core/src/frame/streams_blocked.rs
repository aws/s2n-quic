use crate::{frame::Tag, stream::StreamType, varint::VarInt};
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.14
//# A sender SHOULD send a STREAMS_BLOCKED frame (type=0x16 or 0x17) when
//# it wishes to open a stream, but is unable to due to the maximum
//# stream limit set by its peer; see Section 19.11.  A STREAMS_BLOCKED
//# frame of type 0x16 is used to indicate reaching the bidirectional
//# stream limit, and a STREAMS_BLOCKED frame of type 0x17 indicates
//# reaching the unidirectional stream limit.

macro_rules! streams_blocked_tag {
    () => {
        0x16u8..=0x17u8
    };
}
const BIDIRECTIONAL_TAG: u8 = 0x16;
const UNIDIRECTIONAL_TAG: u8 = 0x17;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.14
//# The STREAMS_BLOCKED frames are shown in Figure 37.
//#
//# STREAMS_BLOCKED Frame {
//#   Type (i) = 0x16..0x17,
//#   Maximum Streams (i),
//# }
//#
//#                Figure 37: STREAMS_BLOCKED Frame Format
//#
//# STREAMS_BLOCKED frames contain the following fields:
//#
//# Maximum Streams:  A variable-length integer indicating the maximum
//#    number of streams allowed at the time the frame was sent.  This
//#    value cannot exceed 2^60, as it is not possible to encode stream
//#    IDs larger than 2^62-1.  Receipt of a frame that encodes a larger
//#    stream ID MUST be treated as a STREAM_LIMIT_ERROR or a
//#    FRAME_ENCODING_ERROR.

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

            let (stream_limit, buffer) = buffer.decode()?;

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
