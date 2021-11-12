// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::varint::VarInt;

//= https://www.rfc-editor.org/rfc/rfc9000.txt#19.12
//# A sender SHOULD send a DATA_BLOCKED frame (type=0x14) when it wishes
//# to send data, but is unable to do so due to connection-level flow
//# control; see Section 4.  DATA_BLOCKED frames can be used as input to
//# tuning of flow control algorithms; see Section 4.2.

macro_rules! data_blocked_tag {
    () => {
        0x14u8
    };
}

//= https://www.rfc-editor.org/rfc/rfc9000.txt#19.12
//# DATA_BLOCKED Frame {
//#   Type (i) = 0x14,
//#   Maximum Data (i),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000.txt#19.12
//# DATA_BLOCKED frames contain the following field:
//#
//# Maximum Data:  A variable-length integer indicating the connection-
//#    level limit at which blocking occurred.

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
