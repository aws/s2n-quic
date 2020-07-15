//= https://tools.ietf.org/id/draft-ietf-quic-transport-25.txt#19.20
//# The server uses the HANDSHAKE_DONE frame (type=0x1e) to signal
//# confirmation of the handshake to the client.  The HANDSHAKE_DONE
//# frame contains no additional fields.

macro_rules! handshake_done_tag {
    () => {
        0x1eu8
    };
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HandshakeDone;

impl HandshakeDone {
    pub const fn tag(self) -> u8 {
        handshake_done_tag!()
    }
}

simple_frame_codec!(HandshakeDone {}, handshake_done_tag!());
