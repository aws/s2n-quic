use crate::varint::VarInt;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.9
//# The MAX_DATA frame (type=0x10) is used in flow control to inform the
//# peer of the maximum amount of data that can be sent on the connection
//# as a whole.

macro_rules! max_data_tag {
    () => {
        0x10u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.9
//# The MAX_DATA frame (type=0x10) is used in flow control to inform the
//# peer of the maximum amount of data that can be sent on the connection
//# as a whole.
//#
//# The MAX_DATA frame is shown in Figure 32.
//#
//# MAX_DATA Frame {
//#   Type (i) = 0x10,
//#   Maximum Data (i),
//# }
//#
//#                    Figure 32: MAX_DATA Frame Format
//#
//# MAX_DATA frames contain the following fields:
//#
//# Maximum Data:  A variable-length integer indicating the maximum
//#    amount of data that can be sent on the entire connection, in units
//#    of bytes.

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
