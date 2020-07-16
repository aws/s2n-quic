use crate::varint::VarInt;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.4
//# An endpoint uses a RESET_STREAM frame (type=0x04) to abruptly
//# terminate the sending part of a stream.

macro_rules! reset_stream_tag {
    () => {
        0x04u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.4
//# The RESET_STREAM frame is shown in Figure 27.
//#
//# RESET_STREAM Frame {
//#   Type (i) = 0x04,
//#   Stream ID (i),
//#   Application Protocol Error Code (i),
//#   Final Size (i),
//# }
//#
//#                  Figure 27: RESET_STREAM Frame Format
//#
//# RESET_STREAM frames contain the following fields:
//#
//# Stream ID:  A variable-length integer encoding of the Stream ID of
//#    the stream being terminated.
//#
//# Application Protocol Error Code:  A variable-length integer
//#    containing the application protocol error code (see Section 20.1)
//#    which indicates why the stream is being closed.
//#
//# Final Size:  A variable-length integer indicating the final size of
//#    the stream by the RESET_STREAM sender, in unit of bytes.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ResetStream {
    /// A variable-length integer encoding of the Stream ID of the
    /// stream being terminated.
    pub stream_id: VarInt,

    /// A variable-length integer containing the application protocol
    /// error code which indicates why the stream is being closed.
    pub application_error_code: VarInt,

    /// A variable-length integer indicating the final size of
    /// the stream by the RESET_STREAM sender, in unit of bytes.
    pub final_size: VarInt,
}

impl ResetStream {
    pub const fn tag(&self) -> u8 {
        reset_stream_tag!()
    }
}

simple_frame_codec!(
    ResetStream {
        stream_id,
        application_error_code,
        final_size
    },
    reset_stream_tag!()
);
