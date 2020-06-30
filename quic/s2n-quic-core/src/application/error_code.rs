//! Defines QUIC Application Error Codes

use crate::varint::VarInt;

//=https://tools.ietf.org/html/draft-ietf-quic-transport-24#section-20.1
//# 20.1.  Application Protocol Error Codes
//#
//#    Application protocol error codes are 62-bit unsigned integers, but
//#    the management of application error codes is left to application
//#    protocols.  Application protocol error codes are used for the
//#    RESET_STREAM frame (Section 19.4), the STOP_SENDING frame
//#    (Section 19.5), and the CONNECTION_CLOSE frame with a type of 0x1d
//#    (Section 19.19).

/// Application Error Codes are 62-bit unsigned integer values which
/// may be used by applications to exchange errors.
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct ApplicationErrorCode(VarInt);

impl core::fmt::Debug for ApplicationErrorCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ApplicationErrorCode({})", Into::<u64>::into(self.0))
    }
}

impl ApplicationErrorCode {
    /// An error code that can be used when the application cannot provide
    /// a more meaningful code.
    pub const UNKNOWN: ApplicationErrorCode = ApplicationErrorCode(VarInt::MAX);

    /// Creates an `ApplicationErrorCode` from an unsigned integer.
    ///
    /// This will return the error code if the given value is inside the valid
    /// range for error codes and return `Err` otherwise.
    pub fn new(value: u64) -> Result<ApplicationErrorCode, ()> {
        Ok(ApplicationErrorCode(VarInt::new(value).map_err(|_| ())?))
    }
}

impl From<VarInt> for ApplicationErrorCode {
    fn from(value: VarInt) -> Self {
        ApplicationErrorCode(value)
    }
}

impl Into<VarInt> for ApplicationErrorCode {
    fn into(self) -> VarInt {
        self.0
    }
}

impl Into<u64> for ApplicationErrorCode {
    fn into(self) -> u64 {
        self.0.into()
    }
}
