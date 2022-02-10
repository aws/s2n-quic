// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::varint::VarInt;

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.9
//# A MAX_DATA frame (type=0x10) is used in flow control to inform the
//# peer of the maximum amount of data that can be sent on the connection
//# as a whole.

macro_rules! max_data_tag {
    () => {
        0x10u8
    };
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.9
//# MAX_DATA Frame {
//#   Type (i) = 0x10,
//#   Maximum Data (i),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.9
//# MAX_DATA frames contain the following field:
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
