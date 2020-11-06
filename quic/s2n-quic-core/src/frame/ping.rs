//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.2
//# Endpoints can use PING frames (type=0x01) to verify that their peers
//# are still alive or to check reachability to the peer.

macro_rules! ping_tag {
    () => {
        0x01u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.2
//# PING Frame {
//#   Type (i) = 0x01,
//# }

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Ping;

impl Ping {
    pub const fn tag(self) -> u8 {
        ping_tag!()
    }
}

simple_frame_codec!(Ping {}, ping_tag!());
