use crate::{frame::Tag, stream::StreamType, varint::VarInt};
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.11
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

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.11
//# The MAX_STREAMS frames are as follows:
//#
//#  0                   1                   2                   3
//#  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                     Maximum Streams (i)                     ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//# MAX_STREAMS frames contain the following fields:
//#
//# Maximum Streams:  A count of the cumulative number of streams of the
//#    corresponding type that can be opened over the lifetime of the
//#    connection.
//#
//# Loss or reordering can cause a MAX_STREAMS frame to be received which
//# states a lower stream limit than an endpoint has previously received.
//# MAX_STREAMS frames which do not increase the stream limit MUST be
//# ignored.
//#
//# An endpoint MUST NOT open more streams than permitted by the current
//# stream limit set by its peer.  For instance, a server that receives a
//# unidirectional stream limit of 3 is permitted to open stream 3, 7,
//# and 11, but not stream 15.  An endpoint MUST terminate a connection
//# with a STREAM_LIMIT_ERROR error if a peer opens more streams than was
//# permitted.
//#
//# Note that these frames (and the corresponding transport parameters)
//# do not describe the number of streams that can be opened
//# concurrently.  The limit includes streams that have been closed as
//# well as those that are open.

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
