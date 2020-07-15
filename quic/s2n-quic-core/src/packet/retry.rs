use crate::packet::{
    decoding::HeaderDecoder,
    long::{DestinationConnectionIDLen, SourceConnectionIDLen, Version},
    Tag,
};
use s2n_codec::{DecoderBufferMut, DecoderBufferMutResult, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-22.txt#17.2.5
//# 17.2.5.  Retry Packet
//#
//#    A Retry packet uses a long packet header with a type value of 0x3.
//#    It carries an address validation token created by the server.  It is
//#    used by a server that wishes to perform a retry (see Section 8.1).
//#
//#     0                   1                   2                   3
//#     0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//#    +-+-+-+-+-+-+-+-+
//#    |1|1| 3 | Unused|
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                         Version (32)                          |
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    | DCID Len (8)  |
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |               Destination Connection ID (0..160)            ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    | SCID Len (8)  |
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                 Source Connection ID (0..160)               ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    | ODCID Len (8) |
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |          Original Destination Connection ID (0..160)        ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#    |                        Retry Token (*)                      ...
//#    +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//#
//#                           Figure 13: Retry Packet

macro_rules! retry_tag {
    () => {
        0b1111u8
    };
}

//#    A Retry packet (shown in Figure 13) does not contain any protected
//#    fields.  The value in the Unused field is selected randomly by the
//#    server.  In addition to the long header, it contains these additional
//#    fields:
//#
//#    ODCID Len:  The ODCID Len contains the length in bytes of the
//#       Original Destination Connection ID field that follows it.  This
//#       length is encoded as a 8-bit unsigned integer.  In QUIC version 1,
//#       this value MUST NOT exceed 20 bytes.  Clients that receive a
//#       version 1 Retry Packet with a value larger than 20 MUST drop the
//#       packet.
//#
//#    Original Destination Connection ID:  The Original Destination
//#       Connection ID contains the value of the Destination Connection ID
//#       from the Initial packet that this Retry is in response to.  The
//#       length of this field is given in ODCID Len.
//#
//#    Retry Token:  An opaque token that the server can use to validate the
//#       client's address.
//#
//#    The server populates the Destination Connection ID with the
//#    connection ID that the client included in the Source Connection ID of
//#    the Initial packet.
//#
//#    The server includes a connection ID of its choice in the Source
//#    Connection ID field.  This value MUST not be equal to the Destination
//#    Connection ID field of the packet sent by the client.  The client
//#    MUST use this connection ID in the Destination Connection ID of
//#    subsequent packets that it sends.
//#
//#    A server MAY send Retry packets in response to Initial and 0-RTT
//#    packets.  A server can either discard or buffer 0-RTT packets that it
//#    receives.  A server can send multiple Retry packets as it receives
//#    Initial or 0-RTT packets.  A server MUST NOT send more than one Retry
//#    packet in response to a single UDP datagram.
//#
//#    A client MUST accept and process at most one Retry packet for each
//#    connection attempt.  After the client has received and processed an
//#    Initial or Retry packet from the server, it MUST discard any
//#    subsequent Retry packets that it receives.
//#
//#    Clients MUST discard Retry packets that contain an Original
//#    Destination Connection ID field that does not match the Destination
//#    Connection ID from its Initial packet.  This prevents an off-path
//#    attacker from injecting a Retry packet.
//#
//#    The client responds to a Retry packet with an Initial packet that
//#    includes the provided Retry Token to continue connection
//#    establishment.
//#
//#    A client sets the Destination Connection ID field of this Initial
//#    packet to the value from the Source Connection ID in the Retry
//#    packet.  Changing Destination Connection ID also results in a change
//#    to the keys used to protect the Initial packet.  It also sets the
//#    Token field to the token provided in the Retry.  The client MUST NOT
//#    change the Source Connection ID because the server could include the
//#    connection ID as part of its token validation logic (see
//#    Section 8.1.3).
//#
//#    The next Initial packet from the client uses the connection ID and
//#    token values from the Retry packet (see Section 7.2).  Aside from
//#    this, the Initial packet sent by the client is subject to the same
//#    restrictions as the first Initial packet.  A client MUST use the same
//#    cryptographic handshake message it includes in this packet.  A server
//#    MAY treat a packet that contains a different cryptographic handshake
//#    message as a connection error or discard it.
//#
//#    A client MAY attempt 0-RTT after receiving a Retry packet by sending
//#    0-RTT packets to the connection ID provided by the server.  A client
//#    MUST NOT change the cryptographic handshake message it sends in
//#    response to receiving a Retry.
//#
//#    A client MUST NOT reset the packet number for any packet number space
//#    after processing a Retry packet; Section 17.2.3 contains more
//#    information on this.
//#
//#    A server acknowledges the use of a Retry packet for a connection
//#    using the original_connection_id transport parameter (see
//#    Section 18.1).  If the server sends a Retry packet, it MUST include
//#    the value of the Original Destination Connection ID field of the
//#    Retry packet (that is, the Destination Connection ID field from the
//#    client's first Initial packet) in the transport parameter.
//#
//#    If the client received and processed a Retry packet, it MUST validate
//#    that the original_connection_id transport parameter is present and
//#    correct; otherwise, it MUST validate that the transport parameter is
//#    absent.  A client MUST treat a failed validation as a connection
//#    error of type TRANSPORT_PARAMETER_ERROR.
//#
//#    A Retry packet does not include a packet number and cannot be
//#    explicitly acknowledged by a client.

#[derive(Debug)]
pub struct Retry<'a> {
    pub version: Version,
    pub destination_connection_id: &'a [u8],
    pub source_connection_id: &'a [u8],
    pub original_destination_connection_id: &'a [u8],
    pub retry_token: &'a [u8],
}

pub type ProtectedRetry<'a> = Retry<'a>;
pub type EncryptedRetry<'a> = Retry<'a>;
pub type CleartextRetry<'a> = Retry<'a>;

impl<'a> Retry<'a> {
    #[inline]
    pub(crate) fn decode(
        _tag: Tag,
        version: Version,
        buffer: DecoderBufferMut,
    ) -> DecoderBufferMutResult<Retry> {
        let mut decoder = HeaderDecoder::new_long(&buffer);

        let destination_connection_id = decoder.decode_destination_connection_id(&buffer)?;
        let source_connection_id = decoder.decode_source_connection_id(&buffer)?;
        let original_destination_connection_id =
            decoder.decode_destination_connection_id(&buffer)?;

        // split header and payload
        let header_len = decoder.decoded_len();
        let (header, buffer) = buffer.decode_slice(header_len)?;
        let header: &[u8] = header.into_less_safe_slice();

        // read borrowed slices
        let destination_connection_id = destination_connection_id.get(header);
        let source_connection_id = source_connection_id.get(header);
        let original_destination_connection_id = original_destination_connection_id.get(header);

        let buffer_len = buffer.len();
        let (retry_token, buffer) = buffer.decode_slice(buffer_len)?;
        let retry_token: &[u8] = retry_token.into_less_safe_slice();

        let packet = Retry {
            version,
            destination_connection_id,
            source_connection_id,
            original_destination_connection_id,
            retry_token,
        };

        Ok((packet, buffer))
    }

    #[inline]
    pub fn destination_connection_id(&self) -> &[u8] {
        &self.destination_connection_id
    }

    #[inline]
    pub fn source_connection_id(&self) -> &[u8] {
        &self.source_connection_id
    }

    #[inline]
    pub fn original_destination_connection_id(&self) -> &[u8] {
        &self.original_destination_connection_id
    }

    #[inline]
    pub fn retry_token(&self) -> &[u8] {
        &self.retry_token
    }
}

impl<'a> EncoderValue for Retry<'a> {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        let tag: u8 = retry_tag!() << 4;
        tag.encode(encoder);

        self.version.encode(encoder);

        self.destination_connection_id
            .encode_with_len_prefix::<DestinationConnectionIDLen, E>(encoder);
        self.source_connection_id
            .encode_with_len_prefix::<SourceConnectionIDLen, E>(encoder);
        self.original_destination_connection_id
            .encode_with_len_prefix::<DestinationConnectionIDLen, E>(encoder);
        self.retry_token.encode(encoder);
    }
}
