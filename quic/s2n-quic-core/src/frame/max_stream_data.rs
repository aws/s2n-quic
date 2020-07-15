use crate::varint::VarInt;

//=https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.1-
//# 19.10.  MAX_STREAM_DATA Frame
//#
//#    The MAX_STREAM_DATA frame (type=0x11) is used in flow control to
//#    inform a peer of the maximum amount of data that can be sent on a
//#    stream.

macro_rules! max_stream_data_tag {
    () => {
        0x11u8
    };
}

//#    A MAX_STREAM_DATA frame can be sent for streams in the Recv state
//#    (see Section 3.1).  Receiving a MAX_STREAM_DATA frame for a locally-
//#    initiated stream that has not yet been created MUST be treated as a
//#    connection error of type STREAM_STATE_ERROR.  An endpoint that
//#    receives a MAX_STREAM_DATA frame for a receive-only stream MUST
//#    terminate the connection with error STREAM_STATE_ERROR.
//#
//#    The MAX_STREAM_DATA frame is as follows:
//#
//#     0                   1                   2                   3
//#     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                        Stream ID (i)                        ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                    Maximum Stream Data (i)                  ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#    MAX_STREAM_DATA frames contain the following fields:
//#
//#    Stream ID:  The stream ID of the stream that is affected encoded as a
//#       variable-length integer.
//#
//#    Maximum Stream Data:  A variable-length integer indicating the
//#       maximum amount of data that can be sent on the identified stream,
//#       in units of bytes.
//#
//#    When counting data toward this limit, an endpoint accounts for the
//#    largest received offset of data that is sent or received on the
//#    stream.  Loss or reordering can mean that the largest received offset
//#    on a stream can be greater than the total size of data received on
//#    that stream.  Receiving STREAM frames might not increase the largest
//#    received offset.
//#
//#    The data sent on a stream MUST NOT exceed the largest maximum stream
//#    data value advertised by the receiver.  An endpoint MUST terminate a
//#    connection with a FLOW_CONTROL_ERROR error if it receives more data
//#    than the largest maximum stream data that it has sent for the
//#    affected stream, unless this is a result of a change in the initial
//#    limits (see Section 7.3.1).

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
