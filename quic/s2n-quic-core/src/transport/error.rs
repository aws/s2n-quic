// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

use crate::{
    crypto::tls,
    event::metrics::aggregate,
    frame::ConnectionClose,
    varint::{VarInt, VarIntError},
};
use core::fmt;
use s2n_codec::DecoderError;

//= https://www.rfc-editor.org/rfc/rfc9000#section-20
//# QUIC transport error codes and application error codes are 62-bit
//# unsigned integers.

/// Transport Errors are 62-bit unsigned integer values indicating a QUIC transport error
/// has occurred, as defined in [QUIC Transport RFC](https://www.rfc-editor.org/rfc/rfc9000.html#section-20).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Error {
    /// A 62-bit unsigned integer value indicating the error that occurred
    pub code: Code,
    /// If this error was caused by a particular QUIC frame, `frame_type` will contain
    /// the Frame Type as defined in [QUIC Transport RFC](https://www.rfc-editor.org/rfc/rfc9000.html#name-frame-types-and-formats).
    pub frame_type: VarInt,
    /// Additional information about the error that occurred
    pub reason: &'static str,
}

impl core::error::Error for Error {}

impl Error {
    /// Creates a new `Error`
    pub const fn new(code: VarInt) -> Self {
        Self {
            code: Code::new(code),
            reason: "",
            frame_type: VarInt::from_u8(0),
        }
    }

    /// Updates the `Error` with the specified `frame_type`
    #[must_use]
    pub const fn with_frame_type(mut self, frame_type: VarInt) -> Self {
        self.frame_type = frame_type;
        self
    }

    /// Updates the `Error` with the specified `reason`
    #[must_use]
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

        if let Some(description) = self.description() {
            d.field("description", &description);
        }

        if !self.reason.is_empty() {
            d.field("reason", &self.reason);
        }

        d.field("frame_type", &self.frame_type);

        d.finish()
    }
}

impl From<Error> for ConnectionClose<'_> {
    fn from(error: Error) -> Self {
        ConnectionClose {
            error_code: error.code.0,
            frame_type: Some(error.frame_type),
            reason: Some(error.reason.as_bytes()).filter(|reason| !reason.is_empty()),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Code(VarInt);

impl Code {
    #[doc(hidden)]
    pub const fn new(code: VarInt) -> Self {
        Self(code)
    }

    #[inline]
    pub fn as_u64(self) -> u64 {
        self.0.as_u64()
    }

    #[inline]
    pub fn as_varint(self) -> VarInt {
        self.0
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

//= https://www.rfc-editor.org/rfc/rfc9000#section-19.19
//# A value of 0 (equivalent to the mention
//# of the PADDING frame) is used when the frame type is unknown.
const UNKNOWN_FRAME_TYPE: u32 = 0;

//= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
//# CRYPTO_ERROR (0x0100-0x01ff):  The cryptographic handshake failed.  A
//#    range of 256 values is reserved for carrying error codes specific
//#    to the cryptographic handshake that is used.  Codes for errors
//#    occurring when TLS is used for the cryptographic handshake are
//#    described in Section 4.8 of [QUIC-TLS].
const CRYPTO_ERROR_RANGE: core::ops::RangeInclusive<u64> = 0x100..=0x1ff;

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
                    code if CRYPTO_ERROR_RANGE.contains(&code) => tls::Error::new(code as u8).description(),
                    _ => None
                }
            }
        }

        impl aggregate::AsVariant for Code {
            const VARIANTS: &'static [aggregate::info::Variant] = &{
                use aggregate::info::{Variant, Str};

                const fn count(_v: u64) -> usize {
                    1
                }

                const QUIC_VARIANTS: usize = 0 $( + count($code))*;

                const TLS: &'static [Variant] = tls::Error::VARIANTS;

                let mut array = [
                    Variant { name: Str::new("\0"), id: 0 };
                    QUIC_VARIANTS + TLS.len() + 1
                ];

                let mut id = 0;

                $(
                    array[id] = Variant {
                        name: Str::new(concat!("QUIC_", stringify!($name), "\0")),
                        id,
                    };
                    id += 1;
                )*

                let mut tls_idx = 0;
                while tls_idx < TLS.len() {
                    let variant = TLS[tls_idx];
                    array[id] = Variant {
                        name: variant.name,
                        id,
                    };
                    id += 1;
                    tls_idx += 1;
                }

                array[id] = Variant {
                    name: Str::new("QUIC_UNKNOWN_ERROR\0"),
                    id,
                };

                array
            };

            #[inline]
            fn variant_idx(&self) -> usize {
                let mut idx = 0;
                let code = self.0.as_u64();

                $(
                    if code == $code {
                        return idx;
                    }
                    idx += 1;
                )*

                if CRYPTO_ERROR_RANGE.contains(&code) {
                    return tls::Error::new(code as _).variant_idx() + idx;
                }

                idx + tls::Error::VARIANTS.len()
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
            assert_eq!(&Error::from(tls::Error::DECODE_ERROR).to_string(), "DECODE_ERROR");
        }

        #[test]
        #[cfg_attr(miri, ignore)]
        fn variants_test() {
            use aggregate::AsVariant;
            insta::assert_debug_snapshot!(Code::VARIANTS);

            let mut seen = std::collections::HashSet::new();
            for variant in Code::VARIANTS {
                assert!(seen.insert(variant.id));
            }
        }
    };
}

impl_errors! {
    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# NO_ERROR (0x00):  An endpoint uses this with CONNECTION_CLOSE to
    //#    signal that the connection is being closed abruptly in the absence
    //#    of any error.
    /// An endpoint uses this with CONNECTION_CLOSE to
    /// signal that the connection is being closed abruptly in the absence
    /// of any error
    NO_ERROR = 0x0.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# INTERNAL_ERROR (0x01):  The endpoint encountered an internal error and
    //#    cannot continue with the connection.
    /// The endpoint encountered an internal error
    /// and cannot continue with the connection.
    INTERNAL_ERROR = 0x1.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# CONNECTION_REFUSED (0x02):  The server refused to accept a new
    //#  connection.
    /// The server refused to accept a new
    ///  connection.
    CONNECTION_REFUSED = 0x2.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# FLOW_CONTROL_ERROR (0x03):  An endpoint received more data than it
    //#    permitted in its advertised data limits; see Section 4.
    /// An endpoint received more data than it
    /// permitted in its advertised data limits.
    FLOW_CONTROL_ERROR = 0x3.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# STREAM_LIMIT_ERROR (0x04):  An endpoint received a frame for a stream
    //#    identifier that exceeded its advertised stream limit for the
    //#    corresponding stream type.
    /// An endpoint received a frame for a stream
    /// identifier that exceeded its advertised stream limit for the
    /// corresponding stream type.
    STREAM_LIMIT_ERROR = 0x4.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# STREAM_STATE_ERROR (0x05):  An endpoint received a frame for a stream
    //#    that was not in a state that permitted that frame; see Section 3.
    /// An endpoint received a frame for a stream
    /// that was not in a state that permitted that frame.
    STREAM_STATE_ERROR = 0x5.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# FINAL_SIZE_ERROR (0x06):  (1) An endpoint received a STREAM frame
    //#    containing data that exceeded the previously established final
    //#    size, (2) an endpoint received a STREAM frame or a RESET_STREAM
    //#    frame containing a final size that was lower than the size of
    //#    stream data that was already received, or (3) an endpoint received
    //#    a STREAM frame or a RESET_STREAM frame containing a different
    //#    final size to the one already established.
    /// An endpoint received a STREAM frame
    /// containing data that exceeded the previously established final
    /// size.
    FINAL_SIZE_ERROR = 0x6.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# FRAME_ENCODING_ERROR (0x07):  An endpoint received a frame that was
    //#   badly formatted -- for instance, a frame of an unknown type or an
    //#   ACK frame that has more acknowledgment ranges than the remainder
    //#   of the packet could carry.
    /// An endpoint received a frame that was
    /// badly formatted.
    FRAME_ENCODING_ERROR = 0x7.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# TRANSPORT_PARAMETER_ERROR (0x08):  An endpoint received transport
    //#    parameters that were badly formatted, included an invalid value,
    //#    omitted a mandatory transport parameter, included a forbidden
    //#    transport parameter, or were otherwise in error.
    /// An endpoint received transport
    /// parameters that were badly formatted.
    TRANSPORT_PARAMETER_ERROR = 0x8.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# CONNECTION_ID_LIMIT_ERROR (0x09):  The number of connection IDs
    //#    provided by the peer exceeds the advertised
    //#    active_connection_id_limit.
    /// The number of connection IDs
    /// provided by the peer exceeds the advertised
    /// active_connection_id_limit.
    CONNECTION_ID_LIMIT_ERROR = 0x9.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# PROTOCOL_VIOLATION (0x0a):  An endpoint detected an error with
    //#    protocol compliance that was not covered by more specific error
    //#    codes.
    /// An endpoint detected an error with
    /// protocol compliance that was not covered by more specific error
    /// codes.
    PROTOCOL_VIOLATION = 0xA.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# INVALID_TOKEN (0x0b):  A server received a client Initial that
    //#     contained an invalid Token field.
    /// A server received a client Initial that
    /// contained an invalid Token field.
    INVALID_TOKEN = 0xB.with_frame_type(UNKNOWN_FRAME_TYPE),

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# APPLICATION_ERROR (0x0c):  The application or application protocol
    //#    caused the connection to be closed.
    /// The application or application protocol
    /// caused the connection to be closed.
    APPLICATION_ERROR = 0xC,

    //= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
    //# CRYPTO_BUFFER_EXCEEDED (0x0d):  An endpoint has received more data in
    //#    CRYPTO frames than it can buffer.
    /// An endpoint has received more data in
    /// CRYPTO frames than it can buffer.
    CRYPTO_BUFFER_EXCEEDED = 0xD.with_frame_type(UNKNOWN_FRAME_TYPE),

    //# KEY_UPDATE_ERROR (0x0e):  An endpoint detected errors in performing
    //#    key updates; see Section 6 of [QUIC-TLS].
    /// An endpoint detected errors in performing
    /// key updates.
    KEY_UPDATE_ERROR = 0xe.with_frame_type(UNKNOWN_FRAME_TYPE),

    //# AEAD_LIMIT_REACHED (0x0f):  An endpoint has reached the
    //#    confidentiality or integrity limit for the AEAD algorithm used by
    //#    the given connection.
    /// An endpoint has reached the
    /// confidentiality or integrity limit for the AEAD algorithm used by
    /// the given connection.
    AEAD_LIMIT_REACHED = 0xf.with_frame_type(UNKNOWN_FRAME_TYPE),
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-20.1
//# CRYPTO_ERROR (0x0100-0x01ff):  The cryptographic handshake failed.  A
//#   range of 256 values is reserved for carrying error codes specific
//#   to the cryptographic handshake that is used.  Codes for errors
//#   occurring when TLS is used for the cryptographic handshake are
//#   described in Section 4.8 of [QUIC-TLS].

impl Error {
    /// Creates a crypto-level `TransportError` from a TLS alert code.
    #[inline]
    pub const fn crypto_error(code: u8) -> Self {
        Self::new(VarInt::from_u16(0x100 | (code as u16)))
            .with_frame_type(VarInt::from_u32(UNKNOWN_FRAME_TYPE))
    }

    /// If the [`Error`] contains a [`tls::Error`], it is returned
    #[inline]
    pub fn try_into_tls_error(self) -> Option<tls::Error> {
        let code = self.code.as_u64();
        if (0x100..=0x1ff).contains(&code) {
            Some(tls::Error::new(code as u8).with_reason(self.reason))
        } else {
            None
        }
    }
}

//= https://www.rfc-editor.org/rfc/rfc9000#section-20.2
//# The management of application error codes is left to application
//# protocols.  Application protocol error codes are used for the
//# RESET_STREAM frame (Section 19.4), the STOP_SENDING frame
//# (Section 19.5), and the CONNECTION_CLOSE frame with a type of 0x1d
//# (Section 19.19).

impl Error {
    /// Creates an application-level `Error`
    #[inline]
    pub const fn application_error(code: VarInt) -> Self {
        // Application errors set `frame_type` to `None`
        Self::new(code)
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

/// Implements conversion from TLS errors
/// See `Error::crypto_error` for more details
impl From<tls::Error> for Error {
    fn from(tls_error: tls::Error) -> Self {
        Self::crypto_error(tls_error.code).with_reason(tls_error.reason)
    }
}

/// Implements conversion from crypto errors
/// See `Error::crypto_error` for more details
impl From<VarIntError> for Error {
    fn from(_: VarIntError) -> Self {
        Self::INTERNAL_ERROR.with_reason("varint encoding limit exceeded")
    }
}
