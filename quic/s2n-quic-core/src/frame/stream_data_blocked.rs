// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::varint::VarInt;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.13
//# This frame is analogous to DATA_BLOCKED (Section 19.12).

macro_rules! stream_data_blocked_tag {
    () => {
        0x15u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.13
//# STREAM_DATA_BLOCKED Frame {
//#   Type (i) = 0x15,
//#   Stream ID (i),
//#   Maximum Stream Data (i),
//# }

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.13
//# STREAM_DATA_BLOCKED frames contain the following fields:
//#
//# Stream ID:  A variable-length integer indicating the stream that is
//#    blocked due to flow control.
//#
//# Maximum Stream Data:  A variable-length integer indicating the offset
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
