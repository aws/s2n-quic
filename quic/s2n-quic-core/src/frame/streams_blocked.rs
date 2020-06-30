use crate::{frame::Tag, stream::StreamType, varint::VarInt};
use s2n_codec::{decoder_parameterized_value, Encoder, EncoderValue};

//=https://quicwg.org/base-drafts/draft-ietf-quic-transport.html#rfc.section.19.14
//# 19.14.  STREAMS_BLOCKED Frames
//#
//#    A sender SHOULD send a STREAMS_BLOCKED frame (type=0x16 or 0x17) when
//#    it wishes to open a stream, but is unable to due to the maximum
//#    stream limit set by its peer (see Section 19.11).  A STREAMS_BLOCKED
//#    frame of type 0x16 is used to indicate reaching the bidirectional
//#    stream limit, and a STREAMS_BLOCKED frame of type 0x17 indicates
//#    reaching the unidirectional stream limit.

macro_rules! streams_blocked_tag {
    () => {
        0x16u8..=0x17u8
    };
}
const BIDIRECTIONAL_TAG: u8 = 0x16;
const UNIDIRECTIONAL_TAG: u8 = 0x17;

//#    A STREAMS_BLOCKED frame does not open the stream, but informs the
//#    peer that a new stream was needed and the stream limit prevented the
//#    creation of the stream.
//#
//#    The STREAMS_BLOCKED frames are as follows:
//#
//#     0                   1                   2                   3
//#     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                        Stream Limit (i)                     ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#    STREAMS_BLOCKED frames contain the following fields:
//#
//#    Stream Limit:  A variable-length integer indicating the stream limit
//#       at the time the frame was sent.

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
