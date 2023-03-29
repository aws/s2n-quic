// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines the QUIC connection ID

use crate::{
    event::{api::SocketAddress, IntoEvent},
    inet, transport,
};
use core::{
    convert::{TryFrom, TryInto},
    time::Duration,
};
use s2n_codec::{decoder_value, Encoder, EncoderValue};

#[cfg(any(test, feature = "generator"))]
use bolero_generator::*;

//= https://www.rfc-editor.org/rfc/rfc9000#section-5.1
//# Each connection possesses a set of connection identifiers, or
//# connection IDs, each of which can identify the connection.
//# Connection IDs are independently selected by endpoints; each endpoint
//# selects the connection IDs that its peer uses.

/// The maximum size of a connection ID.
pub const MAX_LEN: usize = crate::packet::long::DESTINATION_CONNECTION_ID_MAX_LEN;

/// The minimum lifetime of a connection ID.
pub const MIN_LIFETIME: Duration = Duration::from_secs(60);

/// The maximum bounded lifetime of a connection ID. Connection IDs may have no specified
/// lifetime at all, but if a lifetime is specified, it cannot exceed this value.
pub const MAX_LIFETIME: Duration = Duration::from_secs(24 * 60 * 60); // one day

macro_rules! id {
    ($type:ident, $min_len:expr) => {
        /// Uniquely identifies a QUIC connection between 2 peers
        #[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[cfg_attr(any(feature = "generator", test), derive(TypeGenerator))]
        pub struct $type {
            bytes: [u8; MAX_LEN],
            #[cfg_attr(any(feature = "generator", test), generator(Self::GENERATOR))]
            len: u8,
        }

        impl core::fmt::Debug for $type {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, "{}({:?})", stringify!($type), self.as_bytes())
            }
        }

        impl $type {
            /// The minimum length for this connection ID type
            pub const MIN_LEN: usize = $min_len;

            #[cfg(any(feature = "generator", test))]
            const GENERATOR: core::ops::RangeInclusive<u8> = $min_len..=(MAX_LEN as u8);

            /// Creates a connection ID from a byte array.
            ///
            /// If the passed byte array exceeds the maximum allowed length for
            /// Connection IDs (20 bytes in QUIC v1) `None` will be returned.
            /// All other input values are valid.
            #[inline]
            pub fn try_from_bytes(bytes: &[u8]) -> Option<$type> {
                Self::try_from(bytes).ok()
            }

            /// Returns the Connection ID in byte form
            #[inline]
            pub fn as_bytes(&self) -> &[u8] {
                self.as_ref()
            }

            /// Returns the length of the connection id
            #[inline]
            pub const fn len(&self) -> usize {
                self.len as usize
            }

            /// Returns true if this connection ID is zero-length
            #[inline]
            pub fn is_empty(&self) -> bool {
                self.len == 0
            }

            /// A connection ID to use for testing
            #[cfg(any(test, feature = "testing"))]
            pub const TEST_ID: Self = Self::test_id();

            // Constructs a test connection ID by converting the name
            // of the ID type to bytes and populating the first and last
            // bytes of a max length ID with those bytes.
            #[cfg(any(test, feature = "testing"))]
            const fn test_id() -> Self {
                let type_bytes = stringify!($type).as_bytes();
                let mut result = [0u8; MAX_LEN];
                result[0] = type_bytes[0];
                result[1] = type_bytes[1];
                result[2] = type_bytes[2];
                result[3] = type_bytes[3];
                result[4] = type_bytes[4];
                result[5] = type_bytes[5];
                result[14] = type_bytes[0];
                result[15] = type_bytes[1];
                result[16] = type_bytes[2];
                result[17] = type_bytes[3];
                result[18] = type_bytes[4];
                result[19] = type_bytes[5];
                Self {
                    bytes: result,
                    len: MAX_LEN as u8,
                }
            }
        }

        impl From<[u8; MAX_LEN]> for $type {
            #[inline]
            fn from(bytes: [u8; MAX_LEN]) -> Self {
                Self {
                    bytes,
                    len: MAX_LEN as u8,
                }
            }
        }

        impl TryFrom<&[u8]> for $type {
            type Error = Error;

            #[inline]
            fn try_from(slice: &[u8]) -> Result<Self, Self::Error> {
                let len = slice.len();
                if !($type::MIN_LEN..=MAX_LEN).contains(&len) {
                    return Err(Error::InvalidLength);
                }
                let mut bytes = [0; MAX_LEN];
                bytes[..len].copy_from_slice(slice);
                Ok(Self {
                    bytes,
                    len: len as u8,
                })
            }
        }

        impl AsRef<[u8]> for $type {
            #[inline]
            fn as_ref(&self) -> &[u8] {
                &self.bytes[0..self.len as usize]
            }
        }

        decoder_value!(
            impl<'a> $type {
                fn decode(buffer: Buffer) -> Result<Self> {
                    let len = buffer.len();
                    let (value, buffer) = buffer.decode_slice(len)?;
                    let value: &[u8] = value.into_less_safe_slice();
                    let connection_id = $type::try_from(value).map_err(|_| {
                        s2n_codec::DecoderError::InvariantViolation(concat!(
                            "invalid ",
                            stringify!($type)
                        ))
                    })?;

                    Ok((connection_id, buffer))
                }
            }
        );

        impl EncoderValue for $type {
            #[inline]
            fn encode<E: Encoder>(&self, encoder: &mut E) {
                self.as_ref().encode(encoder)
            }
        }

        // Implement Default to allow for transport_parameter macro to work consistently,
        // though this value should never be used.
        impl Default for $type {
            fn default() -> Self {
                unimplemented!("connection IDs do not have default values")
            }
        }
    };
}

// Connection IDs that are generated locally and used to route packets from the peer to the local
// endpoint. s2n-QUIC does not provide zero-length connection IDs, the minimum allowable LocalId
// is 4 bytes.
id!(LocalId, 4);

// Connection IDs used to route packets to the peer. The peer may choose to use zero-length
// connection IDs.
id!(PeerId, 0);

// Connection IDs that are used as either a LocalId or a PeerId depending on if the endpoint is a
// server or a client, and thus the minimum length of the ID is not validated.
id!(UnboundedId, 0);
// The randomly generated ID the client sends when first contacting a server.
//= https://www.rfc-editor.org/rfc/rfc9000#section-7.2
//# When an Initial packet is sent by a client that has not previously
//# received an Initial or Retry packet from the server, the client
//# populates the Destination Connection ID field with an unpredictable
//# value.  This Destination Connection ID MUST be at least 8 bytes in
//# length.
id!(InitialId, 8);

impl From<LocalId> for UnboundedId {
    fn from(id: LocalId) -> Self {
        UnboundedId {
            bytes: id.bytes,
            len: id.len,
        }
    }
}

impl From<PeerId> for UnboundedId {
    fn from(id: PeerId) -> Self {
        UnboundedId {
            bytes: id.bytes,
            len: id.len,
        }
    }
}

impl From<InitialId> for UnboundedId {
    fn from(id: InitialId) -> Self {
        UnboundedId {
            bytes: id.bytes,
            len: id.len,
        }
    }
}

impl From<InitialId> for PeerId {
    fn from(id: InitialId) -> Self {
        PeerId {
            bytes: id.bytes,
            len: id.len,
        }
    }
}

// A LocalId may be converted to an InitialId, but InitialId has a higher
// minimum length, so conversion may not succeed.
impl TryFrom<LocalId> for InitialId {
    type Error = Error;

    #[inline]
    fn try_from(value: LocalId) -> Result<Self, Self::Error> {
        value.as_bytes().try_into()
    }
}

#[derive(Debug, PartialEq)]
pub enum Error {
    InvalidLength,
    InvalidLifetime,
}

impl Error {
    fn message(&self) -> &'static str {
        match self {
            Error::InvalidLength => "invalid connection id length",
            Error::InvalidLifetime => "invalid connection id lifetime",
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl From<Error> for transport::Error {
    #[inline]
    fn from(error: Error) -> Self {
        Self::PROTOCOL_VIOLATION.with_reason(error.message())
    }
}

/// Information about the connection that may be used
/// when generating or validating connection IDs
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ConnectionInfo<'a> {
    pub remote_address: SocketAddress<'a>,
}

impl<'a> ConnectionInfo<'a> {
    #[inline]
    #[doc(hidden)]
    pub fn new(remote_address: &'a inet::SocketAddress) -> Self {
        Self {
            remote_address: remote_address.into_event(),
        }
    }
}

/// Format for connection IDs
pub trait Format: 'static + Validator + Generator + Send {}

/// Implement Format for all types that implement the required subtraits
impl<T: 'static + Validator + Generator + Send> Format for T {}

/// A validator for a connection ID format
pub trait Validator {
    //= https://www.rfc-editor.org/rfc/rfc9000#section-10.3.2
    //# An endpoint that uses this design MUST
    //# either use the same connection ID length for all connections or
    //# encode the length of the connection ID such that it can be recovered
    //# without state.
    /// Validates a connection ID from a buffer
    ///
    /// Implementations should handle situations where the buffer will include extra
    /// data after the connection ID.
    ///
    /// Returns the length of the connection id if successful, otherwise `None` is returned.
    fn validate(&self, connection_info: &ConnectionInfo, buffer: &[u8]) -> Option<usize>;
}

impl Validator for usize {
    #[inline]
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
    /// Generates a connection ID.
    ///
    /// Connection IDs MUST NOT contain any information that can be used by
    /// an external observer (that is, one that does not cooperate with the
    /// issuer) to correlate them with other connection IDs for the same
    /// connection.
    ///
    /// Each call to `generate` should produce a unique Connection ID,
    /// otherwise the endpoint may terminate.
    fn generate(&mut self, connection_info: &ConnectionInfo) -> LocalId;

    /// The maximum amount of time each generated connection ID should be
    /// used for. By default there is no maximum, though connection IDs
    /// may be retired due to rotation requirements or peer requests.
    #[inline]
    fn lifetime(&self) -> Option<core::time::Duration> {
        None
    }
}

#[derive(Debug, Copy, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Interest {
    /// No new connection Ids are required
    #[default]
    None,
    /// The specified number of new connection Ids are required
    New(u8),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_connection_id() {
        let connection_id = LocalId::try_from_bytes(b"My Connection 123").unwrap();
        assert_eq!(b"My Connection 123", connection_id.as_bytes());

        let connection_id = PeerId::try_from_bytes(b"My Connection 456").unwrap();
        assert_eq!(b"My Connection 456", connection_id.as_bytes());

        let connection_id = InitialId::try_from_bytes(b"My Connection 789").unwrap();
        assert_eq!(b"My Connection 789", connection_id.as_bytes());
    }

    #[test]
    fn exceed_max_connection_id_length() {
        let connection_id_bytes = [0u8; MAX_LEN];
        assert!(LocalId::try_from_bytes(&connection_id_bytes).is_some());
        assert!(PeerId::try_from_bytes(&connection_id_bytes).is_some());
        assert!(InitialId::try_from_bytes(&connection_id_bytes).is_some());

        let connection_id_bytes = [0u8; MAX_LEN + 1];
        assert!(LocalId::try_from_bytes(&connection_id_bytes).is_none());
        assert!(PeerId::try_from_bytes(&connection_id_bytes).is_none());
        assert!(InitialId::try_from_bytes(&connection_id_bytes).is_none());
    }

    #[test]
    fn min_connection_id_length() {
        let connection_id_bytes = [0u8; LocalId::MIN_LEN];
        assert!(LocalId::try_from_bytes(&connection_id_bytes).is_some());

        let connection_id_bytes = [0u8; PeerId::MIN_LEN];
        assert!(PeerId::try_from_bytes(&connection_id_bytes).is_some());

        let connection_id_bytes = [0u8; InitialId::MIN_LEN];
        assert!(InitialId::try_from_bytes(&connection_id_bytes).is_some());

        let connection_id_bytes = [0u8; LocalId::MIN_LEN - 1];
        assert!(LocalId::try_from_bytes(&connection_id_bytes).is_none());

        let connection_id_bytes = [0u8; InitialId::MIN_LEN - 1];
        assert!(InitialId::try_from_bytes(&connection_id_bytes).is_none());
    }

    #[test]
    fn unbounded_id() {
        let connection_id_bytes = [0u8; LocalId::MIN_LEN];
        assert!(UnboundedId::try_from_bytes(&connection_id_bytes).is_some());

        let connection_id_bytes = [0u8; PeerId::MIN_LEN];
        assert!(UnboundedId::try_from_bytes(&connection_id_bytes).is_some());

        let connection_id_bytes = [0u8; InitialId::MIN_LEN];
        assert!(UnboundedId::try_from_bytes(&connection_id_bytes).is_some());

        println!("{:?}", LocalId::TEST_ID);
        println!("{:?}", PeerId::TEST_ID);
        println!("{:?}", UnboundedId::TEST_ID);
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
        fn generate(&mut self, _connection_info: &ConnectionInfo) -> LocalId {
            let id = (&self.0.to_be_bytes()[..]).try_into().unwrap();
            self.0 += 1;
            id
        }
    }
}
