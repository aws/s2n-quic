//! Defines the Stateless Reset token

use core::convert::{TryFrom, TryInto};
use s2n_codec::{decoder_value, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#10.4
//# A stateless reset is provided as an option of last resort for an
//# endpoint that does not have access to the state of a connection.  A
//# crash or outage might result in peers continuing to send data to an
//# endpoint that is unable to properly continue the connection.  An
//# endpoint MAY send a stateless reset in response to receiving a packet
//# that it cannot associate with an active connection.
//#
//# A stateless reset is not appropriate for signaling error conditions.
//# An endpoint that wishes to communicate a fatal connection error MUST
//# use a CONNECTION_CLOSE frame if it has sufficient state to do so.
//#
//# To support this process, a token is sent by endpoints.  The token is
//# carried in the Stateless Reset Token field of a NEW_CONNECTION_ID
//# frame.  Servers can also specify a stateless_reset_token transport
//# parameter during the handshake that applies to the connection ID that
//# it selected during the handshake; clients cannot use this transport
//# parameter because their transport parameters don't have
//# confidentiality protection.  These tokens are protected by
//# encryption, so only client and server know their value.  Tokens are
//# invalidated when their associated connection ID is retired via a
//# RETIRE_CONNECTION_ID frame (Section 19.16).
//#
//# An endpoint that receives packets that it cannot process sends a
//# packet in the following layout:
//#
//#  0                   1                   2                   3
//#  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |0|1|               Unpredictable Bits (38 ..)                ...
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//# |                                                               |
//# +                                                               +
//# |                                                               |
//# +                   Stateless Reset Token (128)                 +
//# |                                                               |
//# +                                                               +
//# |                                                               |
//# +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+

const STATELESS_RESET_TOKEN_LEN: usize = 128 / 8;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#10.4
//#                   Figure 6: Stateless Reset Packet
//#
//# This design ensures that a stateless reset packet is - to the extent
//# possible - indistinguishable from a regular packet with a short
//# header.
//#
//# A stateless reset uses an entire UDP datagram, starting with the
//# first two bits of the packet header.  The remainder of the first byte
//# and an arbitrary number of bytes following it that are set to
//# unpredictable values.  The last 16 bytes of the datagram contain a
//# Stateless Reset Token.
//#
//# To entities other than its intended recipient, a stateless reset will
//# appear to be a packet with a short header.  For the stateless reset
//# to appear as a valid QUIC packet, the Unpredictable Bits field needs
//# to include at least 38 bits of data (or 5 bytes, less the two fixed
//# bits).
//#
//# A minimum size of 21 bytes does not guarantee that a stateless reset
//# is difficult to distinguish from other packets if the recipient
//# requires the use of a connection ID.  To prevent a resulting
//# stateless reset from being trivially distinguishable from a valid
//# packet, all packets sent by an endpoint SHOULD be padded to at least
//# 22 bytes longer than the minimum connection ID that the endpoint
//# might use.  An endpoint that sends a stateless reset in response to
//# packet that is 43 bytes or less in length SHOULD send a stateless
//# reset that is one byte shorter than the packet it responds to.
//#
//# These values assume that the Stateless Reset Token is the same as the
//# minimum expansion of the packet protection AEAD.  Additional
//# unpredictable bytes are necessary if the endpoint could have
//# negotiated a packet protection scheme with a larger minimum
//# expansion.
//#
//# An endpoint MUST NOT send a stateless reset that is three times or
//# more larger than the packet it receives to avoid being used for
//# amplification.  Section 10.4.3 describes additional limits on
//# stateless reset size.
//#
//# Endpoints MUST discard packets that are too small to be valid QUIC
//# packets.  With the set of AEAD functions defined in [QUIC-TLS],
//# packets that are smaller than 21 bytes are never valid.
//#
//# Endpoints MUST send stateless reset packets formatted as a packet
//# with a short header.  However, endpoints MUST treat any packet ending
//# in a valid stateless reset token as a stateless reset, as other QUIC
//# versions might allow the use of a long header.
//#
//# An endpoint MAY send a stateless reset in response to a packet with a
//# long header.  Sending a stateless reset is not effective prior to the
//# stateless reset token being available to a peer.  In this QUIC
//# version, packets with a long header are only used during connection
//# establishment.  Because the stateless reset token is not available
//# until connection establishment is complete or near completion,
//# ignoring an unknown packet with a long header might be as effective
//# as sending a stateless reset.
//#
//# An endpoint cannot determine the Source Connection ID from a packet
//# with a short header, therefore it cannot set the Destination
//# Connection ID in the stateless reset packet.  The Destination
//# Connection ID will therefore differ from the value used in previous
//# packets.  A random Destination Connection ID makes the connection ID
//# appear to be the result of moving to a new connection ID that was
//# provided using a NEW_CONNECTION_ID frame (Section 19.15).
//#
//# Using a randomized connection ID results in two problems:
//#
//# o  The packet might not reach the peer.  If the Destination
//#    Connection ID is critical for routing toward the peer, then this
//#    packet could be incorrectly routed.  This might also trigger
//#    another Stateless Reset in response; see Section 10.4.3.  A
//#    Stateless Reset that is not correctly routed is an ineffective
//#    error detection and recovery mechanism.  In this case, endpoints
//#    will need to rely on other methods - such as timers - to detect
//#    that the connection has failed.
//#
//# o  The randomly generated connection ID can be used by entities other
//#    than the peer to identify this as a potential stateless reset.  An
//#    endpoint that occasionally uses different connection IDs might
//#    introduce some uncertainty about this.
//#
//# This stateless reset design is specific to QUIC version 1.  An
//# endpoint that supports multiple versions of QUIC needs to generate a
//# stateless reset that will be accepted by peers that support any
//# version that the endpoint might support (or might have supported
//# prior to losing state).  Designers of new versions of QUIC need to be
//# aware of this and either reuse this design, or use a portion of the
//# packet other than the last 16 bytes for carrying data.

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StatelessResetToken([u8; STATELESS_RESET_TOKEN_LEN]);

impl StatelessResetToken {
    /// A zeroed out stateless reset token
    pub const ZEROED: Self = Self([0; STATELESS_RESET_TOKEN_LEN]);
}

impl From<[u8; STATELESS_RESET_TOKEN_LEN]> for StatelessResetToken {
    fn from(bytes: [u8; STATELESS_RESET_TOKEN_LEN]) -> Self {
        Self(bytes)
    }
}

impl TryFrom<&[u8]> for StatelessResetToken {
    type Error = core::array::TryFromSliceError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        let bytes = bytes.try_into()?;
        Ok(Self(bytes))
    }
}

impl AsRef<[u8]> for StatelessResetToken {
    fn as_ref(&self) -> &[u8] {
        &self.0[..]
    }
}

decoder_value!(
    impl<'a> StatelessResetToken {
        fn decode(buffer: Buffer) -> Result<Self> {
            let (value, buffer) = buffer.decode_slice(STATELESS_RESET_TOKEN_LEN)?;
            let value: &[u8] = value.into_less_safe_slice();
            let connection_id =
                StatelessResetToken::try_from(value).expect("slice len already verified");

            Ok((connection_id, buffer))
        }
    }
);

impl EncoderValue for StatelessResetToken {
    fn encoding_size(&self) -> usize {
        STATELESS_RESET_TOKEN_LEN
    }

    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.as_ref().encode(encoder)
    }
}
