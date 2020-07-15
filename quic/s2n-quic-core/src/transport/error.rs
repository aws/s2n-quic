#![forbid(unsafe_code)]

use crate::{
    application::{ApplicationErrorCode, ApplicationErrorExt},
    crypto::CryptoError,
    varint::{VarInt, VarIntError},
};
use core::fmt;
use s2n_codec::DecoderError;

//= https://tools.ietf.org/html/draft-ietf-quic-transport-23#section-20
//# 20.  Transport Error Codes
//#
//#    QUIC error codes are 62-bit unsigned integers.
//#
//#    This section lists the defined QUIC transport error codes that may be
//#    used in a CONNECTION_CLOSE frame.  These errors apply to the entire
//#    connection.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct TransportError {
    pub code: VarInt,
    pub frame_type: Option<VarInt>,
    pub reason: &'static str,
}

impl TransportError {
    /// Creates a new `TransportError` with the specified information
    pub const fn new(code: VarInt, reason: &'static str, frame_type: Option<VarInt>) -> Self {
        Self {
            code,
            reason,
            frame_type,
        }
    }

    /// Updates the `TransportError` with the specified `frame_type`
    pub const fn with_frame_type(mut self, frame_type: VarInt) -> Self {
        self.frame_type = Some(frame_type);
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
        if self.reason.is_empty() {
            let code: u64 = self.code.into();
            write!(f, "TransportError({})", code)?;
        } else {
            f.write_str(&self.reason)?;
        }

        Ok(())
    }
}

/// Internal convenience macro for defining standard error codes
macro_rules! def_error {
    ($doc:expr, $name:ident, $code:expr) => {
        impl TransportError {
            #[doc = $doc]
            pub const $name: VarInt = VarInt::from_u32($code);
        }
    };
}

//#    NO_ERROR (0x0):  An endpoint uses this with CONNECTION_CLOSE to
//#       signal that the connection is being closed abruptly in the absence
//#       of any error.

def_error!(
    "An endpoint uses this with CONNECTION_CLOSE to signal that the connection is being closed abruptly in the absence of any error.",
    NO_ERROR,
    0x0
);

//#    INTERNAL_ERROR (0x1):  The endpoint encountered an internal error and
//#       cannot continue with the connection.

def_error!(
    "The endpoint encountered an internal error and cannot continue with the connection.",
    INTERNAL_ERROR,
    0x1
);

//#    SERVER_BUSY (0x2):  The server is currently busy and does not accept
//#       any new connections.

def_error!(
    "The server is currently busy and does not accept any new connections.",
    SERVER_BUSY,
    0x2
);

//#    FLOW_CONTROL_ERROR (0x3):  An endpoint received more data than it
//#       permitted in its advertised data limits (see Section 4).

def_error!(
    "An endpoint received more data than it permitted in its advertised data limits.",
    FLOW_CONTROL_ERROR,
    0x3
);

//#    STREAM_LIMIT_ERROR (0x4):  An endpoint received a frame for a stream
//#       identifier that exceeded its advertised stream limit for the
//#       corresponding stream type.

def_error!(
    "An endpoint received a frame for a stream identifier that exceeded its advertised stream limit for the corresponding stream type.",
    STREAM_LIMIT_ERROR,
    0x4
);

//#    STREAM_STATE_ERROR (0x5):  An endpoint received a frame for a stream
//#       that was not in a state that permitted that frame (see Section 3).

def_error!(
    "An endpoint received a frame for a stream that was not in a state that permitted that frame.",
    STREAM_STATE_ERROR,
    0x5
);

//#    FINAL_SIZE_ERROR (0x6):  An endpoint received a STREAM frame
//#       containing data that exceeded the previously established final
//#       size.  Or an endpoint received a STREAM frame or a RESET_STREAM
//#       frame containing a final size that was lower than the size of
//#       stream data that was already received.  Or an endpoint received a
//#       STREAM frame or a RESET_STREAM frame containing a different final
//#       size to the one already established.

def_error!(
    "An endpoint received a STREAM frame containing data that exceeded the previously established final size.",
    FINAL_SIZE_ERROR,
    0x6
);

//#    FRAME_ENCODING_ERROR (0x7):  An endpoint received a frame that was
//#       badly formatted.  For instance, a frame of an unknown type, or an
//#       ACK frame that has more acknowledgment ranges than the remainder
//#       of the packet could carry.

def_error!(
    "An endpoint received a frame that was badly formatted.",
    FRAME_ENCODING_ERROR,
    0x7
);

//#    TRANSPORT_PARAMETER_ERROR (0x8):  An endpoint received transport
//#       parameters that were badly formatted, included an invalid value,
//#       was absent even though it is mandatory, was present though it is
//#       forbidden, or is otherwise in error.

def_error!(
    "An endpoint received transport parameters that were badly formatted.",
    TRANSPORT_PARAMETER_ERROR,
    0x8
);

//#    PROTOCOL_VIOLATION (0xA):  An endpoint detected an error with
//#       protocol compliance that was not covered by more specific error
//#       codes.

def_error!(
    "An endpoint detected an error with protocol compliance that was not covered by more specific error codes.",
    PROTOCOL_VIOLATION,
    0xA
);

//#    CRYPTO_BUFFER_EXCEEDED (0xD):  An endpoint has received more data in
//#       CRYPTO frames than it can buffer.

def_error!(
    "An endpoint has received more data in CRYPTO frames than it can buffer.",
    CRYPTO_BUFFER_EXCEEDED,
    0xD
);

//#    CRYPTO_ERROR (0x1XX):  The cryptographic handshake failed.  A range
//#       of 256 values is reserved for carrying error codes specific to the
//#       cryptographic handshake that is used.  Codes for errors occurring
//#       when TLS is used for the crypto handshake are described in
//#       Section 4.8 of [QUIC-TLS].

impl TransportError {
    #[inline]
    /// Creates a crypto-level `TransportError` from a TLS alert code.
    pub fn crypto_error(code: u8, reason: &'static str) -> Self {
        Self {
            code: VarInt::from_u32(0x100 | u32::from(code)),
            reason,
            frame_type: None,
        }
    }
}

//#    See Section 22.3 for details of registering new error codes.
//#
//# 20.1.  Application Protocol Error Codes
//#
//#    Application protocol error codes are 62-bit unsigned integers, but
//#    the management of application error codes is left to application
//#    protocols.  Application protocol error codes are used for the
//#    RESET_STREAM frame (Section 19.4), the STOP_SENDING frame
//#    (Section 19.5), and the CONNECTION_CLOSE frame with a type of 0x1d
//#    (Section 19.19).

impl TransportError {
    #[inline]
    /// Creates an application-level `TransportError`
    pub const fn applicaton_error(code: VarInt, reason: &'static str) -> Self {
        Self {
            code,
            reason,
            frame_type: None,
        }
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

/// Creates a `TransportErrors` with variable arguments
#[macro_export]
macro_rules! transport_error {
    ($error:ident) => {
        $crate::transport::error::TransportError::new(
            $crate::transport::error::TransportError::$error,
            "",
            None,
        )
    };
    ($error:expr) => {
        $crate::transport::error::TransportError::new($error, "", None)
    };
    ($error:ident, $reason:expr) => {
        $crate::transport::error::TransportError::new(
            $crate::transport::error::TransportError::$error,
            $reason,
            None,
        )
    };
    ($error:expr, $reason:expr) => {
        $crate::transport::error::TransportError::new($error, $reason, None)
    };
    ($error:ident, $reason:expr, $frame:expr) => {
        $crate::transport::error::TransportError::new(
            $crate::transport::error::TransportError::$error,
            $reason,
            Some($frame.into()),
        )
    };
    ($error:expr, $reason:expr, $frame:expr) => {
        $crate::transport::error::TransportError::new($error, $reason, Some($frame.into()))
    };
}

/// Implements conversion from decoder errors
impl From<DecoderError> for TransportError {
    fn from(decoder_error: DecoderError) -> Self {
        match decoder_error {
            DecoderError::InvariantViolation(reason) => {
                transport_error!(PROTOCOL_VIOLATION, reason)
            }
            _ => transport_error!(PROTOCOL_VIOLATION, "malformed packet"),
        }
    }
}

/// Implements conversion from crypto errors
/// See `TransportError::crypto_error` for more details
impl From<CryptoError> for TransportError {
    fn from(crypto_error: CryptoError) -> Self {
        Self::crypto_error(crypto_error.code, crypto_error.reason)
    }
}

/// Implements conversion from crypto errors
/// See `TransportError::crypto_error` for more details
impl From<VarIntError> for TransportError {
    fn from(_: VarIntError) -> Self {
        transport_error!(INTERNAL_ERROR, "varint encoding limit exceeded")
    }
}

/// Converts an error into a `TransportError` and adds
/// error context.
#[macro_export]
macro_rules! with_transport_information {
    ($reason:expr) => {
        |err| {
            let err: $crate::transport::error::TransportError = err.into();
            err.with_reason($reason)
        }
    };
    ($reason:expr, $frame:expr) => {
        |err| {
            let err: $crate::transport::error::TransportError = err.into();
            err.with_reason($reason).with_frame_type($frame)
        }
    };
}
