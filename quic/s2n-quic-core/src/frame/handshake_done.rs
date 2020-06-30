//= https://tools.ietf.org/html/draft-ietf-quic-transport-25#section-19.20
//# 19.20.  HANDSHAKE_DONE frame
//#
//#    The server uses the HANDSHAKE_DONE frame (type=0x1e) to signal
//#    confirmation of the handshake to the client.  The HANDSHAKE_DONE
//#    frame contains no additional fields.

macro_rules! handshake_done_tag {
    () => {
        0x1eu8
    };
}

//#    This frame can only be sent by the server.  Servers MUST NOT send a
//#    HANDSHAKE_DONE frame before completing the handshake.  A server MUST
//#    treat receipt of a HANDSHAKE_DONE frame as a connection error of type
//#    PROTOCOL_VIOLATION.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HandshakeDone;

impl HandshakeDone {
    pub const fn tag(self) -> u8 {
        handshake_done_tag!()
    }
}

simple_frame_codec!(HandshakeDone {}, handshake_done_tag!());
