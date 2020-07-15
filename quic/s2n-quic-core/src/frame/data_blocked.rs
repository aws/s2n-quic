use crate::varint::VarInt;

//=https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.12
//# 19.12.  DATA_BLOCKED Frame
//#
//#    A sender SHOULD send a DATA_BLOCKED frame (type=0x14) when it wishes
//#    to send data, but is unable to due to connection-level flow control
//#    (see Section 4).  DATA_BLOCKED frames can be used as input to tuning
//#    of flow control algorithms (see Section 4.2).

macro_rules! data_blocked_tag {
    () => {
        0x14u8
    };
}

//#    The DATA_BLOCKED frame is as follows:
//#
//#     0                   1                   2                   3
//#     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                       Data Limit (i)                        ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#    DATA_BLOCKED frames contain the following fields:
//#
//#    Data Limit:  A variable-length integer indicating the connection-
//#       level limit at which blocking occurred.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DataBlocked {
    /// A variable-length integer indicating the connection-level limit
    /// at which blocking occurred.
    pub data_limit: VarInt,
}

impl DataBlocked {
    pub const fn tag(self) -> u8 {
        data_blocked_tag!()
    }
}

simple_frame_codec!(DataBlocked { data_limit }, data_blocked_tag!());
