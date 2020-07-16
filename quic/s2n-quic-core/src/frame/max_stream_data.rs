use crate::varint::VarInt;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.10
//# The MAX_STREAM_DATA frame (type=0x11) is used in flow control to
//# inform a peer of the maximum amount of data that can be sent on a
//# stream.

macro_rules! max_stream_data_tag {
    () => {
        0x11u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.10
//# The MAX_STREAM_DATA frame is shown in Figure 33.
//#
//# MAX_STREAM_DATA Frame {
//#   Type (i) = 0x11,
//#   Stream ID (i),
//#   Maximum Stream Data (i),
//# }
//#
//#                Figure 33: MAX_STREAM_DATA Frame Format
//#
//# MAX_STREAM_DATA frames contain the following fields:
//#
//# Stream ID:  The stream ID of the stream that is affected encoded as a
//#    variable-length integer.
//#
//# Maximum Stream Data:  A variable-length integer indicating the
//#    maximum amount of data that can be sent on the identified stream,
//#    in units of bytes.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MaxStreamData {
    /// The stream ID of the stream that is affected encoded as a
    /// variable-length integer.
    pub stream_id: VarInt,

    /// A variable-length integer indicating the maximum amount of data
    /// that can be sent on the identified stream, in units of bytes.
    pub maximum_stream_data: VarInt,
}

impl MaxStreamData {
    pub const fn tag(&self) -> u8 {
        max_stream_data_tag!()
    }
}

simple_frame_codec!(
    MaxStreamData {
        stream_id,
        maximum_stream_data
    },
    max_stream_data_tag!()
);
