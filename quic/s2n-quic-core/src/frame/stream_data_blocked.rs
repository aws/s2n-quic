use crate::varint::VarInt;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.13
//# A sender SHOULD send a STREAM_DATA_BLOCKED frame (type=0x15) when it
//# wishes to send data, but is unable to due to stream-level flow
//# control.  This frame is analogous to DATA_BLOCKED (Section 19.12).

macro_rules! stream_data_blocked_tag {
    () => {
        0x15u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.13
//# An endpoint that receives a STREAM_DATA_BLOCKED frame for a send-only
//# stream MUST terminate the connection with error STREAM_STATE_ERROR.
//#
//# The STREAM_DATA_BLOCKED frame is as follows:
//#
//#  0                   1                   2                   3
//#  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                        Stream ID (i)                        ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                    Stream Data Limit (i)                    ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//# STREAM_DATA_BLOCKED frames contain the following fields:
//#
//# Stream ID:  A variable-length integer indicating the stream which is
//#    flow control blocked.
//#
//# Stream Data Limit:  A variable-length integer indicating the offset
//#    of the stream at which the blocking occurred.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StreamDataBlocked {
    /// A variable-length integer indicating the stream which
    /// is flow control blocked.
    pub stream_id: VarInt,

    /// A variable-length integer indicating the offset of the stream at
    // which the blocking occurred.
    pub stream_data_limit: VarInt,
}

impl StreamDataBlocked {
    pub const fn tag(&self) -> u8 {
        stream_data_blocked_tag!()
    }
}

simple_frame_codec!(
    StreamDataBlocked {
        stream_id,
        stream_data_limit
    },
    stream_data_blocked_tag!()
);
