use crate::varint::VarInt;

//=https://quicwg.org/base-drafts/draft-ietf-quic-transport.html#rfc.section.19.16
// 19.16.  RETIRE_CONNECTION_ID Frame
//
//    An endpoint sends a RETIRE_CONNECTION_ID frame (type=0x19) to
//    indicate that it will no longer use a connection ID that was issued
//    by its peer.  This may include the connection ID provided during the
//    handshake.  Sending a RETIRE_CONNECTION_ID frame also serves as a
//    request to the peer to send additional connection IDs for future use
//    (see Section 5.1).  New connection IDs can be delivered to a peer
//    using the NEW_CONNECTION_ID frame (Section 19.15).

macro_rules! retire_connection_id_tag {
    () => {
        0x19u8
    };
}

//    Retiring a connection ID invalidates the stateless reset token
//    associated with that connection ID.
//
//    The RETIRE_CONNECTION_ID frame is as follows:
//
//     0                   1                   2                   3
//     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//    |                      Sequence Number (i)                    ...
//    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//
//    RETIRE_CONNECTION_ID frames contain the following fields:
//
//    Sequence Number:  The sequence number of the connection ID being
//       retired.  See Section 5.1.2.
//
//    Receipt of a RETIRE_CONNECTION_ID frame containing a sequence number
//    greater than any previously sent to the peer MAY be treated as a
//    connection error of type PROTOCOL_VIOLATION.
//
//    The sequence number specified in a RETIRE_CONNECTION_ID frame MUST
//    NOT refer to the Destination Connection ID field of the packet in
//    which the frame is contained.  The peer MAY treat this as a
//    connection error of type PROTOCOL_VIOLATION.
//
//    An endpoint cannot send this frame if it was provided with a zero-
//    length connection ID by its peer.  An endpoint that provides a zero-
//    length connection ID MUST treat receipt of a RETIRE_CONNECTION_ID
//    frame as a connection error of type PROTOCOL_VIOLATION.

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RetireConnectionID {
    pub sequence_number: VarInt,
}

impl RetireConnectionID {
    pub const fn tag(self) -> u8 {
        retire_connection_id_tag!()
    }
}

simple_frame_codec!(
    RetireConnectionID { sequence_number },
    retire_connection_id_tag!()
);
