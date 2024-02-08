// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use core::fmt;
use s2n_codec::DecoderError;

/// Error type for TLS-related errors
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
pub struct Error {
    pub reason: &'static str,
    pub code: u8,
}

impl Error {
    /// Creates a new `tls::Error`
    pub const fn new(code: u8) -> Self {
        Self { code, reason: "" }
    }

    /// Sets the reason for `tls::Error`
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
        } else if let Some(description) = self.description() {
            description.fmt(f)
        } else {
            write!(f, "tls::Error({})", self.code)
        }
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("tls::Error");

        d.field("code", &self.code);

        if let Some(description) = self.description() {
            d.field("description", &description);
        }

        if !self.reason.is_empty() {
            d.field("reason", &self.reason);
        }

        d.finish()
    }
}

impl From<DecoderError> for Error {
    fn from(_: DecoderError) -> Self {
        Self::DECODE_ERROR
    }
}

macro_rules! alert_descriptions {
    ($($name:ident = $value:expr),* $(,)?) => {
        impl Error {
            pub fn description(&self) -> Option<&'static str> {
                match self.code {
                    $(
                        $value => Some(stringify!($name)),
                    )*
                    _ => None,
                }
            }

            $(
                pub const $name: Self = Self::new($value);
            )*
        }

        #[test]
        fn description_test() {
            $(
                assert_eq!(&Error::$name.to_string(), stringify!($name));
            )*
        }
    };
}

//= https://www.rfc-editor.org/rfc/rfc8446#appendix-B.2
//# enum { warning(1), fatal(2), (255) } AlertLevel;
//#
//# enum {
//#     close_notify(0),
//#     unexpected_message(10),
//#     bad_record_mac(20),
//#     decryption_failed_RESERVED(21),
//#     record_overflow(22),
//#     decompression_failure_RESERVED(30),
//#     handshake_failure(40),
//#     no_certificate_RESERVED(41),
//#     bad_certificate(42),
//#     unsupported_certificate(43),
//#     certificate_revoked(44),
//#     certificate_expired(45),
//#     certificate_unknown(46),
//#     illegal_parameter(47),
//#     unknown_ca(48),
//#     access_denied(49),
//#     decode_error(50),
//#     decrypt_error(51),
//#     export_restriction_RESERVED(60),
//#     protocol_version(70),
//#     insufficient_security(71),
//#     internal_error(80),
//#     inappropriate_fallback(86),
//#     user_canceled(90),
//#     no_renegotiation_RESERVED(100),
//#     missing_extension(109),
//#     unsupported_extension(110),
//#     certificate_unobtainable_RESERVED(111),
//#     unrecognized_name(112),
//#     bad_certificate_status_response(113),
//#     bad_certificate_hash_value_RESERVED(114),
//#     unknown_psk_identity(115),
//#     certificate_required(116),
//#     no_application_protocol(120),
//#     (255)
//# } AlertDescription;
//#
//# struct {
//#     AlertLevel level;
//#     AlertDescription description;
//# } Alert;

alert_descriptions!(
    CLOSE_NOTIFY = 0,
    UNEXPECTED_MESSAGE = 10,
    BAD_RECORD_MAC = 20,
    DECRYPTION_FAILED_RESERVED = 21,
    RECORD_OVERFLOW = 22,
    DECOMPRESSION_FAILURE_RESERVED = 30,
    HANDSHAKE_FAILURE = 40,
    NO_CERTIFICATE_RESERVED = 41,
    BAD_CERTIFICATE = 42,
    UNSUPPORTED_CERTIFICATE = 43,
    CERTIFICATE_REVOKED = 44,
    CERTIFICATE_EXPIRED = 45,
    CERTIFICATE_UNKNOWN = 46,
    ILLEGAL_PARAMETER = 47,
    UNKNOWN_CA = 48,
    ACCESS_DENIED = 49,
    DECODE_ERROR = 50,
    DECRYPT_ERROR = 51,
    EXPORT_RESTRICTION_RESERVED = 60,
    PROTOCOL_VERSION = 70,
    INSUFFICIENT_SECURITY = 71,
    INTERNAL_ERROR = 80,
    INAPPROPRIATE_FALLBACK = 86,
    USER_CANCELED = 90,
    NO_RENEGOTIATION_RESERVED = 100,
    MISSING_EXTENSION = 109,
    UNSUPPORTED_EXTENSION = 110,
    CERTIFICATE_UNOBTAINABLE_RESERVED = 111,
    UNRECOGNIZED_NAME = 112,
    BAD_CERTIFICATE_STATUS_RESPONSE = 113,
    BAD_CERTIFICATE_HASH_VALUE_RESERVED = 114,
    UNKNOWN_PSK_IDENTITY = 115,
    CERTIFICATE_REQUIRED = 116,
    NO_APPLICATION_PROTOCOL = 120,
);
