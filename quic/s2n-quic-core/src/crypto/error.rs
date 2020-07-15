use s2n_codec::DecoderError;

/// Error type for crypto-related errors
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CryptoError {
    pub reason: &'static str,
    pub code: u8,
}

impl CryptoError {
    /// Creates a `CryptoError` with the `decrypt_error` status code
    pub const fn decrypt_error() -> Self {
        Self {
            reason: "",
            code: DECRYPT_ERROR,
        }
    }

    /// Creates a `CryptoError` with the `decode_error` status code
    pub const fn decode_error() -> Self {
        Self {
            reason: "",
            code: DECODE_ERROR,
        }
    }

    /// Creates a `CryptoError` with the `missing_extension` status code
    pub const fn missing_extension() -> Self {
        Self {
            reason: "",
            code: MISSING_EXTENSION,
        }
    }

    /// Sets the reason for `CryptoError`
    pub fn with_reason(mut self, reason: &'static str) -> Self {
        self.reason = reason;
        self
    }
}

impl From<DecoderError> for CryptoError {
    fn from(_: DecoderError) -> Self {
        Self::decode_error()
    }
}

//= https://tools.ietf.org/rfc/rfc8446.txt#B.2
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

const DECODE_ERROR: u8 = 50;
const DECRYPT_ERROR: u8 = 51;
const MISSING_EXTENSION: u8 = 109;
