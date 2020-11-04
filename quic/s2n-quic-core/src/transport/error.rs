#![forbid(unsafe_code)]

use crate::{
    application::{ApplicationErrorCode, ApplicationErrorExt},
    crypto::CryptoError,
    varint::{VarInt, VarIntError},
};
use core::fmt;
use s2n_codec::DecoderError;

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
//# QUIC error codes are 62-bit unsigned integers.
//#
//# This section lists the defined QUIC transport error codes that may be
//# used in a CONNECTION_CLOSE frame.  These errors apply to the entire
//# connection.

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
pub struct TransportError {
    pub code: VarInt,
    pub frame_type: Option<VarInt>,
    pub reason: &'static str,
}

impl TransportError {
    /// Creates a new `TransportError`
    pub const fn new(code: VarInt) -> Self {
        Self {
            code,
            reason: "",
            frame_type: None,
        }
    }

    /// Updates the `TransportError` with the specified `frame_type`
    pub const fn with_frame_type(self, frame_type: VarInt) -> Self {
        self.with_optional_frame_type(Some(frame_type))
    }

    /// Updated the `TransportError` with the optional `frame_type`
    pub const fn with_optional_frame_type(mut self, frame_type: Option<VarInt>) -> Self {
        self.frame_type = frame_type;
        self
    }

    /// Updates the `TransportError` with the specified `reason`
    pub const fn with_reason(mut self, reason: &'static str) -> Self {
        self.reason = reason;
        self
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if !self.reason.is_empty() {
            self.reason.fmt(f)
        } else if let Some(description) = self.description() {
            description.fmt(f)
        } else {
            write!(f, "TransportError({})", self.code)
        }
    }
}

impl fmt::Debug for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("TransportError");

        d.field("code", &self.code);

        if let Some(description) = self.description() {
            d.field("description", &description);
        }

        if !self.reason.is_empty() {
            d.field("reason", &self.reason);
        }

        if let Some(frame_type) = self.frame_type {
            d.field("frame_type", &frame_type);
        }

        d.finish()
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-32.txt#19.19
//# A value of 0 (equivalent to the mention
//# of the PADDING frame) is used when the frame type is unknown.
const UNKNOWN_FRAME_TYPE: u32 = 0;

/// Internal convenience macro for defining standard error codes
macro_rules! impl_errors {
    ($($(#[doc = $doc:expr])* $name:ident = $code:literal $(.with_frame_type($frame:expr))?),* $(,)?) => {
        impl TransportError {
            $(
                $(#[doc = $doc])*
                pub const $name: Self = Self::new(VarInt::from_u32($code))
                    $( .with_frame_type(VarInt::from_u32($frame)) )?;
            )*

            pub fn description(&self) -> Option<&'static str> {
                match self.code.as_u64() {
                    $(
                        $code => Some(stringify!($name)),
                    )*
                    code @ 0x100..=0x1ff => CryptoError::new(code as u8).description(),
                    _ => None
                }
            }
        }

        #[test]
        fn description_test() {
            $(
                assert_eq!(&TransportError::$name.to_string(), stringify!($name));
            )*
            assert_eq!(&TransportError::from(CryptoError::DECODE_ERROR).to_string(), "DECODE_ERROR");
        }
    };
}

impl_errors! {
    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# NO_ERROR (0x0):  An endpoint uses this with CONNECTION_CLOSE to
    //#    signal that the connection is being closed abruptly in the absence
    //#    of any error.
    /// An endpoint uses this with CONNECTION_CLOSE to
    /// signal that the connection is being closed abruptly in the absence
    /// of any error
    NO_ERROR = 0x0.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# INTERNAL_ERROR (0x1):  The endpoint encountered an internal error and
    //#    cannot continue with the connection.
    /// The endpoint encountered an internal error
    /// and cannot continue with the connection.
    INTERNAL_ERROR = 0x1.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# CONNECTION_REFUSED (0x2):  The server refused to accept a new
    //#  connection.
    /// The server refused to accept a new
    ///  connection.
    CONNECTION_REFUSED = 0x2.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# FLOW_CONTROL_ERROR (0x3):  An endpoint received more data than it
    //#    permitted in its advertised data limits; see Section 4.
    /// An endpoint received more data than it
    /// permitted in its advertised data limits.
    FLOW_CONTROL_ERROR = 0x3.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# STREAM_LIMIT_ERROR (0x4):  An endpoint received a frame for a stream
    //#    identifier that exceeded its advertised stream limit for the
    //#    corresponding stream type.
    /// An endpoint received a frame for a stream
    /// identifier that exceeded its advertised stream limit for the
    /// corresponding stream type.
    STREAM_LIMIT_ERROR = 0x4.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# STREAM_STATE_ERROR (0x5):  An endpoint received a frame for a stream
    //#    that was not in a state that permitted that frame; see Section 3.
    /// An endpoint received a frame for a stream
    /// that was not in a state that permitted that frame.
    STREAM_STATE_ERROR = 0x5.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
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

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# FRAME_ENCODING_ERROR (0x7):  An endpoint received a frame that was
    //#    badly formatted.  For instance, a frame of an unknown type, or an
    //#    ACK frame that has more acknowledgment ranges than the remainder
    //#    of the packet could carry.
    /// An endpoint received a frame that was
    /// badly formatted.
    FRAME_ENCODING_ERROR = 0x7.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# TRANSPORT_PARAMETER_ERROR (0x8):  An endpoint received transport
    //#    parameters that were badly formatted, included an invalid value,
    //#    was absent even though it is mandatory, was present though it is
    //#    forbidden, or is otherwise in error.
    /// An endpoint received transport
    /// parameters that were badly formatted.
    TRANSPORT_PARAMETER_ERROR = 0x8.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# CONNECTION_ID_LIMIT_ERROR (0x9):  The number of connection IDs
    //#    provided by the peer exceeds the advertised
    //#    active_connection_id_limit.
    /// The number of connection IDs
    /// provided by the peer exceeds the advertised
    /// active_connection_id_limit.
    CONNECTION_ID_LIMIT_ERROR = 0x9.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# PROTOCOL_VIOLATION (0xA):  An endpoint detected an error with
    //#    protocol compliance that was not covered by more specific error
    //#    codes.
    /// An endpoint detected an error with
    /// protocol compliance that was not covered by more specific error
    /// codes.
    PROTOCOL_VIOLATION = 0xA.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# INVALID_TOKEN (0xB):  A server received a Retry Token in a client
    //#    Initial that is invalid.
    /// A server received a Retry Token in a client
    /// Initial that is invalid.
    INVALID_TOKEN = 0xB.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# APPLICATION_ERROR (0xC):  The application or application protocol
    //#    caused the connection to be closed.
    /// The application or application protocol
    /// caused the connection to be closed.
    APPLICATION_ERROR = 0xC,

    //= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
    //# CRYPTO_BUFFER_EXCEEDED (0xD):  An endpoint has received more data in
    //#    CRYPTO frames than it can buffer.
    /// An endpoint has received more data in
    /// CRYPTO frames than it can buffer.
    CRYPTO_BUFFER_EXCEEDED = 0xD.with_frame_type(UNKNOWN_FRAME_TYPE),
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20
//# CRYPTO_ERROR (0x1XX):  The cryptographic handshake failed.  A range
//#    of 256 values is reserved for carrying error codes specific to the
//#    cryptographic handshake that is used.  Codes for errors occurring
//#    when TLS is used for the crypto handshake are described in
//#    Section 4.8 of [QUIC-TLS].

impl TransportError {
    #[inline]
    /// Creates a crypto-level `TransportError` from a TLS alert code.
    pub const fn crypto_error(code: u8) -> Self {
        Self::new(VarInt::from_u16(0x100 | (code as u16)))
            .with_frame_type(VarInt::from_u32(UNKNOWN_FRAME_TYPE))
    }
}

//= https://tools.ietf.org/id/draft-ietf-quic-transport-29.txt#20.1
//# Application protocol error codes are 62-bit unsigned integers, but
//# the management of application error codes is left to application
//# protocols.  Application protocol error codes are used for the
//# RESET_STREAM frame (Section 19.4), the STOP_SENDING frame
//# (Section 19.5), and the CONNECTION_CLOSE frame with a type of 0x1d
//# (Section 19.19).

impl TransportError {
    #[inline]
    /// Creates an application-level `TransportError`
    pub const fn applicaton_error(code: VarInt) -> Self {
        Self::new(code)
    }
}

// If a `TransportError` contains no frame type it was sent by an application
// and contains an `ApplicationLevelErrorCode`. Otherwise it is an
// error on the QUIC layer.
impl ApplicationErrorExt for TransportError {
    fn application_error_code(&self) -> Option<ApplicationErrorCode> {
        if self.frame_type.is_none() {
            Some(self.code.into())
        } else {
            None
        }
    }
}

/// Implements conversion from decoder errors
impl From<DecoderError> for TransportError {
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
/// See `TransportError::crypto_error` for more details
impl From<CryptoError> for TransportError {
    fn from(crypto_error: CryptoError) -> Self {
        Self::crypto_error(crypto_error.code).with_reason(crypto_error.reason)
    }
}

/// Implements conversion from crypto errors
/// See `TransportError::crypto_error` for more details
impl From<VarIntError> for TransportError {
    fn from(_: VarIntError) -> Self {
        Self::INTERNAL_ERROR.with_reason("varint encoding limit exceeded")
    }
}
