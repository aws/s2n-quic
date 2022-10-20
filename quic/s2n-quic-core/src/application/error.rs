// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Defines QUIC Application Error Codes

use crate::{
    frame::ConnectionClose,
    varint::{VarInt, VarIntError},
};
use core::{convert::TryFrom, fmt, ops};

//= https://www.rfc-editor.org/rfc/rfc9000#section-20.2
//# The management of application error codes is left to application
//# protocols.

/// Application Error Codes are 62-bit unsigned integer values which
/// may be used by applications to exchange errors.
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Error(VarInt);

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "application::Error({})", self.0.as_u64())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "QUIC application error code: {}", self.0.as_u64())
    }
}

impl Error {
    /// An error code that can be used when the application cannot provide
    /// a more meaningful code.
    pub const UNKNOWN: Self = Self(VarInt::from_u8(0));

    /// Creates an `ApplicationErrorCode` from an unsigned integer.
    ///
    /// This will return the error code if the given value is inside the valid
    /// range for error codes and return `Err` otherwise.
    pub fn new(value: u64) -> Result<Self, VarIntError> {
        Ok(Self(VarInt::new(value)?))
    }
}

impl ops::Deref for Error {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl TryInto for Error {
    fn application_error(&self) -> Option<Error> {
        Some(*self)
    }
}

impl From<VarInt> for Error {
    fn from(value: VarInt) -> Self {
        Self(value)
    }
}

impl From<Error> for VarInt {
    fn from(e: Error) -> Self {
        e.0
    }
}

impl From<Error> for u64 {
    fn from(e: Error) -> Self {
        e.0.as_u64()
    }
}

impl<'a> From<Error> for ConnectionClose<'a> {
    fn from(error: Error) -> Self {
        ConnectionClose {
            error_code: error.0,
            frame_type: None,
            reason: None,
        }
    }
}

macro_rules! convert {
    ($ty:ident) => {
        impl From<$ty> for Error {
            fn from(value: $ty) -> Self {
                Self(VarInt::from(value))
            }
        }
    };
}

convert!(u8);
convert!(u16);
convert!(u32);

impl TryFrom<u64> for Error {
    type Error = VarIntError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Ok(VarInt::try_from(value)?.into())
    }
}

/// Conversion trait for errors that have an associated [`Error`]
pub trait TryInto {
    /// Returns the associated [`Error`], if any
    fn application_error(&self) -> Option<Error>;
}
