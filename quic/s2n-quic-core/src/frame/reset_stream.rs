use crate::varint::VarInt;

//=https://quicwg.org/base-drafts/draft-ietf-quic-transport.html#rfc.section.19.4
//# 19.4.  RESET_STREAM Frame
//#
//#    An endpoint uses a RESET_STREAM frame (type=0x04) to abruptly
//#    terminate the sending part of a stream.

macro_rules! reset_stream_tag {
    () => {
        0x04u8
    };
}

//#    After sending a RESET_STREAM, an endpoint ceases transmission and
//#    retransmission of STREAM frames on the identified stream.  A receiver
//#    of RESET_STREAM can discard any data that it already received on that
//#    stream.
//#
//#    An endpoint that receives a RESET_STREAM frame for a send-only stream
//#    MUST terminate the connection with error STREAM_STATE_ERROR.
//#
//#    The RESET_STREAM frame is as follows:
//#
//#     0                   1                   2                   3
//#     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                        Stream ID (i)                        ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                  Application Error Code (i)                 ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                        Final Size (i)                       ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#    RESET_STREAM frames contain the following fields:
//#
//#    Stream ID:  A variable-length integer encoding of the Stream ID of
//#       the stream being terminated.
//#
//#    Application Protocol Error Code:  A variable-length integer
//#       containing the application protocol error code (see Section 20.1)
//#       which indicates why the stream is being closed.
//#
//#    Final Size:  A variable-length integer indicating the final size of
//#       the stream by the RESET_STREAM sender, in unit of bytes.

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
