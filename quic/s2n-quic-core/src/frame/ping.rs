//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.2
//# Endpoints can use PING frames (type=0x01) to verify that their peers
//# are still alive or to check reachability to the peer.  The PING frame
//# contains no additional fields.

macro_rules! ping_tag {
    () => {
        0x01u8
    };
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-27.txt#19.2
//# The receiver of a PING frame simply needs to acknowledge the packet
//# containing this frame.
//#
//# The PING frame can be used to keep a connection alive when an
//# application or application protocol wishes to prevent the connection
//# from timing out.  An application protocol SHOULD provide guidance
//# about the conditions under which generating a PING is recommended.
//# This guidance SHOULD indicate whether it is the client or the server
//# that is expected to send the PING.  Having both endpoints send PING
//# frames without coordination can produce an excessive number of
//# packets and poor performance.
//#
//# A connection will time out if no packets are sent or received for a
//# period longer than the time specified in the idle_timeout transport
//# parameter (see Section 10).  However, state in middleboxes might time
//# out earlier than that.  Though REQ-5 in [RFC4787] recommends a 2
//# minute timeout interval, experience shows that sending packets every
//# 15 to 30 seconds is necessary to prevent the majority of middleboxes
//# from losing state for UDP flows.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Ping;

impl Ping {
    pub const fn tag(self) -> u8 {
        ping_tag!()
    }
}

simple_frame_codec!(Ping {}, ping_tag!());
