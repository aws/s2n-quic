use crate::{frame::Tag, stream::StreamType, varint::VarInt};
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.11
//# The MAX_STREAMS frames (type=0x12 and 0x13) inform the peer of the
//# cumulative number of streams of a given type it is permitted to open.
//# A MAX_STREAMS frame with a type of 0x12 applies to bidirectional
//# streams, and a MAX_STREAMS frame with a type of 0x13 applies to
//# unidirectional streams.

macro_rules! max_streams_tag {
    () => {
        0x12u8..=0x13u8
    };
}
const BIDIRECTIONAL_TAG: u8 = 0x12;
const UNIDIRECTIONAL_TAG: u8 = 0x13;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.11
//# The MAX_STREAMS frames are shown in Figure 34;
//#
//# MAX_STREAMS Frame {
//#   Type (i) = 0x12..0x13,
//#   Maximum Streams (i),
//# }
//#
//#                  Figure 34: MAX_STREAMS Frame Format
//#
//# MAX_STREAMS frames contain the following fields:
//#
//# Maximum Streams:  A count of the cumulative number of streams of the
//#    corresponding type that can be opened over the lifetime of the
//#    connection.  This value cannot exceed 2^60, as it is not possible
//#    to encode stream IDs larger than 2^62-1.  Receipt of a frame that
//#    permits opening of a stream larger than this limit MUST be treated
//#    as a FRAME_ENCODING_ERROR.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MaxStreams {
    pub stream_type: StreamType,

    /// A count of the cumulative number of streams of the corresponding
    /// type that can be opened over the lifetime of the connection.
    pub maximum_streams: VarInt,
}

impl MaxStreams {
    pub fn tag(&self) -> u8 {
        match self.stream_type {
            StreamType::Bidirectional => BIDIRECTIONAL_TAG,
            StreamType::Unidirectional => UNIDIRECTIONAL_TAG,
        }
    }
}

decoder_parameterized_value!(
    impl<'a> MaxStreams {
        fn decode(tag: Tag, buffer: Buffer) -> Result<Self> {
            let stream_type = if BIDIRECTIONAL_TAG == tag {
                StreamType::Bidirectional
            } else {
                StreamType::Unidirectional
            };

            let (maximum_streams, buffer) = buffer.decode()?;

            let frame = MaxStreams {
                stream_type,
                maximum_streams,
            };

            Ok((frame, buffer))
        }
    }
);

impl EncoderValue for MaxStreams {
    fn encode<E: Encoder>(&self, buffer: &mut E) {
        buffer.encode(&self.tag());
        buffer.encode(&self.maximum_streams);
    }
}
