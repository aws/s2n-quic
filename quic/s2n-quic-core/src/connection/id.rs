//! Defines the QUIC connection ID

use crate::transport::error::TransportError;
use core::convert::TryFrom;
use s2n_codec::{decoder_value, Encoder, EncoderValue};

//= https://tools.ietf.org/id/draft-ietf-quic-transport-24.txt#5.1
//# Each connection possesses a set of connection identifiers, or
//# connection IDs, each of which can identify the connection.
//# Connection IDs are independently selected by endpoints; each endpoint
//# selects the connection IDs that its peer uses.
//#
//# The primary function of a connection ID is to ensure that changes in
//# addressing at lower protocol layers (UDP, IP) don't cause packets for
//# a QUIC connection to be delivered to the wrong endpoint.  Each
//# endpoint selects connection IDs using an implementation-specific (and
//# perhaps deployment-specific) method which will allow packets with
//# that connection ID to be routed back to the endpoint and identified
//# by the endpoint upon receipt.
//#
//# Connection IDs MUST NOT contain any information that can be used by
//# an external observer (that is, one that does not cooperate with the
//# issuer) to correlate them with other connection IDs for the same
//# connection.  As a trivial example, this means the same connection ID
//# MUST NOT be issued more than once on the same connection.
//#
//# Packets with long headers include Source Connection ID and
//# Destination Connection ID fields.  These fields are used to set the
//# connection IDs for new connections; see Section 7.2 for details.
//#
//# Packets with short headers (Section 17.3) only include the
//# Destination Connection ID and omit the explicit length.  The length
//# of the Destination Connection ID field is expected to be known to
//# endpoints.  Endpoints using a load balancer that routes based on
//# connection ID could agree with the load balancer on a fixed length
//# for connection IDs, or agree on an encoding scheme.  A fixed portion
//# could encode an explicit length, which allows the entire connection
//# ID to vary in length and still be used by the load balancer.
//#
//# A Version Negotiation (Section 17.2.1) packet echoes the connection
//# IDs selected by the client, both to ensure correct routing toward the
//# client and to allow the client to validate that the packet is in
//# response to an Initial packet.
//#
//# A zero-length connection ID can be used when a connection ID is not
//# needed to route to the correct endpoint.  However, multiplexing
//# connections on the same local IP address and port while using zero-
//# length connection IDs will cause failures in the presence of peer
//# connection migration, NAT rebinding, and client port reuse; and
//# therefore MUST NOT be done unless an endpoint is certain that those
//# protocol features are not in use.
//#
//# When an endpoint has requested a non-zero-length connection ID, it
//# needs to ensure that the peer has a supply of connection IDs from
//# which to choose for packets sent to the endpoint.  These connection
//# IDs are supplied by the endpoint using the NEW_CONNECTION_ID frame
//# (Section 19.15).

/// The maximum size of a connection ID. In QUIC v1, this is 20 bytes.
const MAX_CONNECTION_ID_LEN: usize = 20;

/// Uniquely identifies a QUIC connection between 2 peers
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConnectionId {
    bytes: [u8; MAX_CONNECTION_ID_LEN],
    len: u8,
}

impl core::fmt::Debug for ConnectionId {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ConnectionId({:?})", self.as_bytes())
    }
}

impl ConnectionId {
    /// An empty connection ID
    pub const EMPTY: Self = Self {
        bytes: [0; MAX_CONNECTION_ID_LEN],
        len: 0,
    };

    /// Creates a `ConnectionId` from a byte array.
    ///
    /// If the passed byte array exceeds the maximum allowed length for
    /// Connection IDs (20 bytes in QUIC v1) `None` will be returned.
    /// All other input values are valid.
    pub fn try_from_bytes(bytes: &[u8]) -> Option<ConnectionId> {
        Self::try_from(bytes).ok()
    }

    /// Returns the Connection ID in byte form
    pub fn as_bytes(&self) -> &[u8] {
        self.as_ref()
    }

    /// Returns the length of the connection id
    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[derive(Debug)]
pub struct TryFromSliceError(());

impl From<TryFromSliceError> for TransportError {
    fn from(_: TryFromSliceError) -> TransportError {
        TransportError::PROTOCOL_VIOLATION.with_reason("invalid connection id")
    }
}

impl From<[u8; MAX_CONNECTION_ID_LEN]> for ConnectionId {
    fn from(bytes: [u8; MAX_CONNECTION_ID_LEN]) -> Self {
        Self {
            bytes,
            len: MAX_CONNECTION_ID_LEN as u8,
        }
    }
}

impl TryFrom<&[u8]> for ConnectionId {
    type Error = TryFromSliceError;

    fn try_from(slice: &[u8]) -> Result<Self, Self::Error> {
        let len = slice.len();
        if len > MAX_CONNECTION_ID_LEN {
            return Err(TryFromSliceError(()));
        }
        let mut bytes = [0; MAX_CONNECTION_ID_LEN];
        bytes[..len].copy_from_slice(slice);
        Ok(Self {
            bytes,
            len: len as u8,
        })
    }
}

impl AsRef<[u8]> for ConnectionId {
    fn as_ref(&self) -> &[u8] {
        &self.bytes[0..self.len as usize]
    }
}

decoder_value!(
    impl<'a> ConnectionId {
        fn decode(buffer: Buffer) -> Result<Self> {
            let len = buffer.len();
            let (value, buffer) = buffer.decode_slice(len)?;
            let value: &[u8] = value.into_less_safe_slice();
            let connection_id = ConnectionId::try_from(value).map_err(|_| {
                s2n_codec::DecoderError::UnexpectedBytes(len - MAX_CONNECTION_ID_LEN)
            })?;

            Ok((connection_id, buffer))
        }
    }
);

impl EncoderValue for ConnectionId {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.as_ref().encode(encoder)
    }
}

/// Format for connection IDs
pub trait Format: Validator + Generator {}

/// Implement Format for all types that implement the required subtraits
impl<T: Validator + Generator> Format for T {}

/// A validator for a connection ID format
pub trait Validator {
    /// Validates a connection ID from a buffer
    ///
    /// Implementations should handle situations where the buffer will include extra
    /// data after the connection ID.
    ///
    /// Returns the length of the connection id if successful, otherwise `None` is returned.
    fn validate(&self, buffer: &[u8]) -> Option<usize>;
}

impl Validator for usize {
    fn validate(&self, buffer: &[u8]) -> Option<usize> {
        if buffer.len() >= *self {
            Some(*self)
        } else {
            None
        }
    }
}

/// A generator for a connection ID format
pub trait Generator {
    /// Generates a connection ID with an optional validity duration
    fn generate(&mut self) -> (ConnectionId, Option<core::time::Duration>);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_connection_id() {
        let connection_id = ConnectionId::try_from_bytes(b"My Connection 123").unwrap();
        assert_eq!(b"My Connection 123", connection_id.as_bytes());
    }

    #[test]
    fn exceed_max_connection_id_length() {
        let connection_id_bytes = [0u8; MAX_CONNECTION_ID_LEN];
        assert!(ConnectionId::try_from_bytes(&connection_id_bytes).is_some());

        let connection_id_bytes = [0u8; MAX_CONNECTION_ID_LEN + 1];
        assert!(ConnectionId::try_from_bytes(&connection_id_bytes).is_none());
    }
}
