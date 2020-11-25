//! Defines the QUIC connection ID

use crate::{inet::SocketAddress, transport::error::TransportError};
use core::convert::TryFrom;
use s2n_codec::{decoder_value, Encoder, EncoderValue};
use std::ops::Add;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#5.1
//# Each connection possesses a set of connection identifiers, or
//# connection IDs, each of which can identify the connection.
//# Connection IDs are independently selected by endpoints; each endpoint
//# selects the connection IDs that its peer uses.

/// The maximum size of a connection ID.
pub const MAX_LEN: usize = crate::packet::long::DESTINATION_CONNECTION_ID_MAX_LEN;

/// Uniquely identifies a QUIC connection between 2 peers
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id {
    bytes: [u8; MAX_LEN],
    len: u8,
}

impl core::fmt::Debug for Id {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ConnectionId({:?})", self.as_bytes())
    }
}

impl Id {
    /// An empty connection ID
    pub const EMPTY: Self = Self {
        bytes: [0; MAX_LEN],
        len: 0,
    };

    /// Creates a `ConnectionId` from a byte array.
    ///
    /// If the passed byte array exceeds the maximum allowed length for
    /// Connection IDs (20 bytes in QUIC v1) `None` will be returned.
    /// All other input values are valid.
    pub fn try_from_bytes(bytes: &[u8]) -> Option<Id> {
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
pub struct Error;

impl From<Error> for TransportError {
    fn from(_: Error) -> TransportError {
        TransportError::PROTOCOL_VIOLATION.with_reason("invalid connection id")
    }
}

impl From<[u8; MAX_LEN]> for Id {
    fn from(bytes: [u8; MAX_LEN]) -> Self {
        Self {
            bytes,
            len: MAX_LEN as u8,
        }
    }
}

impl TryFrom<&[u8]> for Id {
    type Error = Error;

    fn try_from(slice: &[u8]) -> Result<Self, Self::Error> {
        let len = slice.len();
        if len > MAX_LEN {
            return Err(Error);
        }
        let mut bytes = [0; MAX_LEN];
        bytes[..len].copy_from_slice(slice);
        Ok(Self {
            bytes,
            len: len as u8,
        })
    }
}

impl AsRef<[u8]> for Id {
    fn as_ref(&self) -> &[u8] {
        &self.bytes[0..self.len as usize]
    }
}

decoder_value!(
    impl<'a> Id {
        fn decode(buffer: Buffer) -> Result<Self> {
            let len = buffer.len();
            let (value, buffer) = buffer.decode_slice(len)?;
            let value: &[u8] = value.into_less_safe_slice();
            let connection_id = Id::try_from(value)
                .map_err(|_| s2n_codec::DecoderError::UnexpectedBytes(len - MAX_LEN))?;

            Ok((connection_id, buffer))
        }
    }
);

impl EncoderValue for Id {
    fn encode<E: Encoder>(&self, encoder: &mut E) {
        self.as_ref().encode(encoder)
    }
}

/// Information about the connection that may be used
/// when generating or validating connection IDs
#[derive(Copy, Clone, Debug)]
#[non_exhaustive]
pub struct ConnectionInfo<'a> {
    pub remote_address: &'a SocketAddress,
}

impl<'a> ConnectionInfo<'a> {
    pub fn new(remote_address: &'a SocketAddress) -> Self {
        Self { remote_address }
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
    fn validate(&self, connection_info: &ConnectionInfo, buffer: &[u8]) -> Option<usize>;
}

impl Validator for usize {
    fn validate(&self, _connection_info: &ConnectionInfo, buffer: &[u8]) -> Option<usize> {
        if buffer.len() >= *self {
            Some(*self)
        } else {
            None
        }
    }
}

/// A generator for a connection ID format
pub trait Generator {
    /// Generates a connection ID with an optional validity duration.
    /// Connection IDs MUST NOT contain any information that can be used by
    /// an external observer (that is, one that does not cooperate with the
    /// issuer) to correlate them with other connection IDs for the same
    /// connection.
    fn generate(&mut self, connection_info: &ConnectionInfo) -> (Id, Option<core::time::Duration>);
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Interest {
    None,
    New { count: u32, retire_prior_to: u32 },
}

impl Add for Interest {
    type Output = Interest;

    fn add(self, rhs: Self) -> Self::Output {
        match (self, rhs) {
            (Interest::None, Interest::None) => Interest::None,
            (
                Interest::None,
                Interest::New {
                    count,
                    retire_prior_to,
                },
            ) => Interest::New {
                count,
                retire_prior_to,
            },
            (
                Interest::New {
                    count,
                    retire_prior_to,
                },
                Interest::None,
            ) => Interest::New {
                count,
                retire_prior_to,
            },
            (
                Interest::New {
                    count,
                    retire_prior_to,
                },
                Interest::New {
                    count: count_rhs,
                    retire_prior_to: retire_prior_to_rhs,
                },
            ) => Interest::New {
                count: count.max(count_rhs),
                retire_prior_to: retire_prior_to.max(retire_prior_to_rhs),
            },
        }
    }
}

impl Default for Interest {
    fn default() -> Self {
        Interest::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_connection_id() {
        let connection_id = Id::try_from_bytes(b"My Connection 123").unwrap();
        assert_eq!(b"My Connection 123", connection_id.as_bytes());
    }

    #[test]
    fn exceed_max_connection_id_length() {
        let connection_id_bytes = [0u8; MAX_LEN];
        assert!(Id::try_from_bytes(&connection_id_bytes).is_some());

        let connection_id_bytes = [0u8; MAX_LEN + 1];
        assert!(Id::try_from_bytes(&connection_id_bytes).is_none());
    }
}

#[cfg(any(test, feature = "testing"))]
pub mod testing {
    use super::*;
    use core::convert::TryInto;

    #[derive(Debug, Default)]
    pub struct Format(u64);

    impl Validator for Format {
        fn validate(&self, _connection_info: &ConnectionInfo, _buffer: &[u8]) -> Option<usize> {
            Some(core::mem::size_of::<u64>())
        }
    }

    impl Generator for Format {
        fn generate(
            &mut self,
            _connection_info: &ConnectionInfo,
        ) -> (Id, Option<core::time::Duration>) {
            let id = (&self.0.to_be_bytes()[..]).try_into().unwrap();
            self.0 += 1;
            (id, None)
        }
    }
}
