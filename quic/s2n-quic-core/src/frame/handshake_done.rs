//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#19.20
//# The server uses the HANDSHAKE_DONE frame (type=0x1e) to signal
//# confirmation of the handshake to the client.  As shown in Figure 43,
//# a HANDSHAKE_DONE frame has no content.

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
