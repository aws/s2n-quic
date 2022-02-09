// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::varint::VarInt;

//= https://www.rfc-editor.org/rfc/rfc9000#19.5
//# An endpoint uses a STOP_SENDING frame (type=0x05) to communicate that
//# incoming data is being discarded on receipt per application request.
//# STOP_SENDING requests that a peer cease transmission on a stream.

macro_rules! stop_sending_tag {
    () => {
        0x05u8
    };
}

//= https://www.rfc-editor.org/rfc/rfc9000#19.5
//# STOP_SENDING Frame {
//#   Type (i) = 0x05,
//#   Stream ID (i),
//#   Application Protocol Error Code (i),
//# }

//= https://www.rfc-editor.org/rfc/rfc9000#19.5
//# STOP_SENDING frames contain the following fields:
//#
//# Stream ID:  A variable-length integer carrying the stream ID of the
//# stream being ignored.
//#
//# Application Protocol Error Code:  A variable-length integer
//# containing the application-specified reason the sender is ignoring
//# the stream; see Section 20.2.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StopSending {
    /// A variable-length integer carrying the Stream ID of the
    /// stream being ignored.
    pub stream_id: VarInt,

    /// A variable-length integer containing the application-specified
    /// reason the sender is ignoring the stream
    pub application_error_code: VarInt,
}

impl StopSending {
    pub const fn tag(&self) -> u8 {
        stop_sending_tag!()
    }
}

simple_frame_codec!(
    StopSending {
        stream_id,
        application_error_code
    },
    stop_sending_tag!()
);
