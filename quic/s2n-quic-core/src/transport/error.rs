// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use crate::{
    application,
    crypto::CryptoError,
    varint::{VarInt, VarIntError},
};
use core::{fmt, ops};
use s2n_codec::DecoderError;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20
//# QUIC transport error codes and application error codes are 62-bit
//# unsigned integers.

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
pub struct Error {
    pub code: Code,
    pub frame_type: Option<VarInt>,
    pub reason: &'static str,
}

impl Error {
    /// Creates a new `Error`
    pub const fn new(code: VarInt) -> Self {
        Self {
            code: Code::new(code),
            reason: "",
            frame_type: None,
        }
    }

    /// Updates the `Error` with the specified `frame_type`
    pub const fn with_frame_type(self, frame_type: VarInt) -> Self {
        self.with_optional_frame_type(Some(frame_type))
    }

    /// Updated the `Error` with the optional `frame_type`
    pub const fn with_optional_frame_type(mut self, frame_type: Option<VarInt>) -> Self {
        self.frame_type = frame_type;
        self
    }

    /// Updates the `Error` with the specified `reason`
    pub const fn with_reason(mut self, reason: &'static str) -> Self {
        self.reason = reason;
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.reason.is_empty() {
            self.reason.fmt(f)
        } else {
            self.code.fmt(f)
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("transport::Error");

        d.field("code", &self.code.as_u64());

        if !self.reason.is_empty() {
            d.field("reason", &self.reason);
        }

        if let Some(frame_type) = self.frame_type {
            d.field("frame_type", &frame_type);
        }

        d.finish()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Code(VarInt);

impl Code {
    /// Creates a new `TransportError`
    pub const fn new(code: VarInt) -> Self {
        Self(code)
    }
}

impl fmt::Debug for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_tuple("transport::error::Code");

        d.field(&self.0);

        if let Some(desc) = self.description() {
            d.field(&desc);
        }

        d.finish()
    }
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(description) = self.description() {
            description.fmt(f)
        } else {
            write!(f, "error({:x?})", self.as_u64())
        }
    }
}

impl ops::Deref for Code {
    type Target = VarInt;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<VarInt> for Code {
    fn from(value: VarInt) -> Self {
        Self::new(value)
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.19
//# A value of 0 (equivalent to the mention
//# of the PADDING frame) is used when the frame type is unknown.
const UNKNOWN_FRAME_TYPE: u32 = 0;

/// Internal convenience macro for defining standard error codes
macro_rules! impl_errors {
    ($($(#[doc = $doc:expr])* $name:ident = $code:literal $(.with_frame_type($frame:expr))?),* $(,)?) => {
        impl Code {
            $(
                $(#[doc = $doc])*
                pub const $name: Self = Self::new(VarInt::from_u32($code));
            )*

            pub fn description(&self) -> Option<&'static str> {
                match self.0.as_u64() {
                    $(
                        $code => Some(stringify!($name)),
                    )*
                    code @ 0x100..=0x1ff => CryptoError::new(code as u8).description(),
                    _ => None
                }
            }
        }

        impl Error {
            $(
                $(#[doc = $doc])*
                pub const $name: Self = Self::new(VarInt::from_u32($code))
                    $( .with_frame_type(VarInt::from_u32($frame)) )?;
            )*

            pub fn description(&self) -> Option<&'static str> {
                self.code.description()
            }
        }

        #[test]
        fn description_test() {
            $(
                assert_eq!(&Error::$name.to_string(), stringify!($name));
            )*
            assert_eq!(&Error::from(CryptoError::DECODE_ERROR).to_string(), "DECODE_ERROR");
        }
    };
}

impl_errors! {
    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# NO_ERROR (0x0):  An endpoint uses this with CONNECTION_CLOSE to
    //#    signal that the connection is being closed abruptly in the absence
    //#    of any error.
    /// An endpoint uses this with CONNECTION_CLOSE to
    /// signal that the connection is being closed abruptly in the absence
    /// of any error
    NO_ERROR = 0x0.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# INTERNAL_ERROR (0x1):  The endpoint encountered an internal error and
    //#    cannot continue with the connection.
    /// The endpoint encountered an internal error
    /// and cannot continue with the connection.
    INTERNAL_ERROR = 0x1.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# CONNECTION_REFUSED (0x2):  The server refused to accept a new
    //#  connection.
    /// The server refused to accept a new
    ///  connection.
    CONNECTION_REFUSED = 0x2.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# FLOW_CONTROL_ERROR (0x3):  An endpoint received more data than it
    //#    permitted in its advertised data limits; see Section 4.
    /// An endpoint received more data than it
    /// permitted in its advertised data limits.
    FLOW_CONTROL_ERROR = 0x3.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# STREAM_LIMIT_ERROR (0x4):  An endpoint received a frame for a stream
    //#    identifier that exceeded its advertised stream limit for the
    //#    corresponding stream type.
    /// An endpoint received a frame for a stream
    /// identifier that exceeded its advertised stream limit for the
    /// corresponding stream type.
    STREAM_LIMIT_ERROR = 0x4.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# STREAM_STATE_ERROR (0x5):  An endpoint received a frame for a stream
    //#    that was not in a state that permitted that frame; see Section 3.
    /// An endpoint received a frame for a stream
    /// that was not in a state that permitted that frame.
    STREAM_STATE_ERROR = 0x5.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# FINAL_SIZE_ERROR (0x6):  An endpoint received a STREAM frame
    //#    containing data that exceeded the previously established final
    //#    size.  Or an endpoint received a STREAM frame or a RESET_STREAM
    //#    frame containing a final size that was lower than the size of
    //#    stream data that was already received.  Or an endpoint received a
    //#    STREAM frame or a RESET_STREAM frame containing a different final
    //#    size to the one already established.
    /// An endpoint received a STREAM frame
    /// containing data that exceeded the previously established final
    /// size.
    FINAL_SIZE_ERROR = 0x6.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# FRAME_ENCODING_ERROR (0x7):  An endpoint received a frame that was
    //#    badly formatted.  For instance, a frame of an unknown type, or an
    //#    ACK frame that has more acknowledgment ranges than the remainder
    //#    of the packet could carry.
    /// An endpoint received a frame that was
    /// badly formatted.
    FRAME_ENCODING_ERROR = 0x7.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# TRANSPORT_PARAMETER_ERROR (0x8):  An endpoint received transport
    //#    parameters that were badly formatted, included an invalid value,
    //#    was absent even though it is mandatory, was present though it is
    //#    forbidden, or is otherwise in error.
    /// An endpoint received transport
    /// parameters that were badly formatted.
    TRANSPORT_PARAMETER_ERROR = 0x8.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# CONNECTION_ID_LIMIT_ERROR (0x9):  The number of connection IDs
    //#    provided by the peer exceeds the advertised
    //#    active_connection_id_limit.
    /// The number of connection IDs
    /// provided by the peer exceeds the advertised
    /// active_connection_id_limit.
    CONNECTION_ID_LIMIT_ERROR = 0x9.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# PROTOCOL_VIOLATION (0xA):  An endpoint detected an error with
    //#    protocol compliance that was not covered by more specific error
    //#    codes.
    /// An endpoint detected an error with
    /// protocol compliance that was not covered by more specific error
    /// codes.
    PROTOCOL_VIOLATION = 0xA.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# INVALID_TOKEN (0xB):  A server received a client Initial that
    //#     contained an invalid Token field.
    /// A server received a client Initial that
    /// contained an invalid Token field.
    INVALID_TOKEN = 0xB.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# APPLICATION_ERROR (0xC):  The application or application protocol
    //#    caused the connection to be closed.
    /// The application or application protocol
    /// caused the connection to be closed.
    APPLICATION_ERROR = 0xC,

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
    //# CRYPTO_BUFFER_EXCEEDED (0xD):  An endpoint has received more data in
    //#    CRYPTO frames than it can buffer.
    /// An endpoint has received more data in
    /// CRYPTO frames than it can buffer.
    CRYPTO_BUFFER_EXCEEDED = 0xD.with_frame_type(UNKNOWN_FRAME_TYPE),

    //# KEY_UPDATE_ERROR (0xe):  An endpoint detected errors in performing
    //#    key updates; see Section 6 of [QUIC-TLS].
    /// An endpoint detected errors in performing
    /// key updates.
    KEY_UPDATE_ERROR = 0xe.with_frame_type(UNKNOWN_FRAME_TYPE),

    //# AEAD_LIMIT_REACHED (0xf):  An endpoint has reached the
    //#    confidentiality or integrity limit for the AEAD algorithm used by
    //#    the given connection.
    /// An endpoint has reached the
    /// confidentiality or integrity limit for the AEAD algorithm used by
    /// the given connection.
    AEAD_LIMIT_REACHED = 0xf.with_frame_type(UNKNOWN_FRAME_TYPE),
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.1
//# CRYPTO_ERROR (0x1XX):  The cryptographic handshake failed.  A range
//#    of 256 values is reserved for carrying error codes specific to the
//#    cryptographic handshake that is used.  Codes for errors occurring
//#    when TLS is used for the crypto handshake are described in
//#    Section 4.8 of [QUIC-TLS].

impl Error {
    #[inline]
    /// Creates a crypto-level `TransportError` from a TLS alert code.
    pub const fn crypto_error(code: u8) -> Self {
        Self::new(VarInt::from_u16(0x100 | (code as u16)))
            .with_frame_type(VarInt::from_u32(UNKNOWN_FRAME_TYPE))
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#20.2
//# The management of application error codes is left to application
//# protocols.  Application protocol error codes are used for the
//# RESET_STREAM frame (Section 19.4), the STOP_SENDING frame
//# (Section 19.5), and the CONNECTION_CLOSE frame with a type of 0x1d
//# (Section 19.19).

impl Error {
    #[inline]
    /// Creates an application-level `Error`
    pub const fn applicaton_error(code: VarInt) -> Self {
        // Application errors set `frame_type` to `None`
        Self::new(code)
    }
}

/// If a `Error` contains no frame type it was sent by an application
/// and contains an `ApplicationLevelErrorCode`. Otherwise it is an
/// error on the QUIC layer.
impl application::error::TryInto for Error {
    fn application_error(&self) -> Option<application::Error> {
        if self.frame_type.is_none() {
            Some(self.code.0.into())
        } else {
            None
        }
    }
}

/// Implements conversion from decoder errors
impl From<DecoderError> for Error {
    fn from(decoder_error: DecoderError) -> Self {
        match decoder_error {
            DecoderError::InvariantViolation(reason) => {
                Self::PROTOCOL_VIOLATION.with_reason(reason)
            }
            _ => Self::PROTOCOL_VIOLATION.with_reason("malformed packet"),
        }
    }
}

/// Implements conversion from crypto errors
/// See `Error::crypto_error` for more details
impl From<CryptoError> for Error {
    fn from(crypto_error: CryptoError) -> Self {
        Self::crypto_error(crypto_error.code).with_reason(crypto_error.reason)
    }
}

/// Implements conversion from crypto errors
/// See `Error::crypto_error` for more details
impl From<VarIntError> for Error {
    fn from(_: VarIntError) -> Self {
        Self::INTERNAL_ERROR.with_reason("varint encoding limit exceeded")
    }
}
