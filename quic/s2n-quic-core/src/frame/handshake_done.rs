//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.20
//# The server uses a HANDSHAKE_DONE frame (type=0x1e) to signal
//# confirmation of the handshake to the client.

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
