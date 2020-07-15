use crate::varint::VarInt;

//=https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.9
//# 19.9.  MAX_DATA Frame
//#
//#    The MAX_DATA frame (type=0x10) is used in flow control to inform the
//#    peer of the maximum amount of data that can be sent on the connection
//#    as a whole.

macro_rules! max_data_tag {
    () => {
        0x10u8
    };
}

//#    The MAX_DATA frame is as follows:
//#
//#     0                   1                   2                   3
//#     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                        Maximum Data (i)                     ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#    MAX_DATA frames contain the following fields:
//#
//#    Maximum Data:  A variable-length integer indicating the maximum
//#       amount of data that can be sent on the entire connection, in units
//#       of bytes.
//#
//#    All data sent in STREAM frames counts toward this limit.  The sum of
//#    the largest received offsets on all streams - including streams in
//#    terminal states - MUST NOT exceed the value advertised by a receiver.
//#    An endpoint MUST terminate a connection with a FLOW_CONTROL_ERROR
//#    error if it receives more data than the maximum data value that it
//#    has sent, unless this is a result of a change in the initial limits
//#    (see Section 7.3.1).

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MaxData {
    /// A variable-length integer indicating the maximum amount of data
    /// that can be sent on the entire connection, in units of bytes.
    pub maximum_data: VarInt,
}

impl MaxData {
    pub const fn tag(self) -> u8 {
        max_data_tag!()
    }
}

simple_frame_codec!(MaxData { maximum_data }, max_data_tag!());
