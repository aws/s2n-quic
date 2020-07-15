use crate::varint::VarInt;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.5
//# An endpoint uses a STOP_SENDING frame (type=0x05) to communicate that
//# incoming data is being discarded on receipt at application request.
//# STOP_SENDING requests that a peer cease transmission on a stream.

macro_rules! stop_sending_tag {
    () => {
        0x05u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.5
//# A STOP_SENDING frame can be sent for streams in the Recv or Size
//# Known states (see Section 3.1).  Receiving a STOP_SENDING frame for a
//# locally-initiated stream that has not yet been created MUST be
//# treated as a connection error of type STREAM_STATE_ERROR.  An
//# endpoint that receives a STOP_SENDING frame for a receive-only stream
//# MUST terminate the connection with error STREAM_STATE_ERROR.
//#
//# The STOP_SENDING frame is as follows:
//#
//#  0                   1                   2                   3
//#  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                        Stream ID (i)                        ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                  Application Error Code (i)                 ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//# STOP_SENDING frames contain the following fields:
//#
//# Stream ID:  A variable-length integer carrying the Stream ID of the
//#    stream being ignored.
//#
//# Application Error Code:  A variable-length integer containing the
//#    application-specified reason the sender is ignoring the stream
//#    (see Section 20.1).

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
