// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application::{ApplicationErrorCode, ApplicationErrorExt},
    crypto::CryptoError,
    frame::ConnectionClose,
    transport::error::TransportError,
};

/// Errors that a connection can encounter.
#[derive(PartialEq, Debug, Copy, Clone, displaydoc::Display)]
#[non_exhaustive]
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
pub enum Error {
    /// The connection was closed without an error
    Closed,

    /// The connection was closed on the transport level
    ///
    /// This can occur either locally or by the peer. The argument contains
    /// the error code which the transport provided in order to close the
    /// connection.
    Transport(u64),

    /// The connection was closed on the application level
    ///
    /// This can occur either locally or by the peer. The argument contains
    /// the error code which the application/ supplied in order to close the
    /// connection.
    Application(ApplicationErrorCode),

    /// The connection was closed because the connection's idle timer expired
    IdleTimerExpired,

    /// All Stream IDs for Streams on a given connection had been exhausted
    StreamIdExhausted,

    /// The connection was closed due to an unspecified reason
    Unspecified,
}

impl Error {
    fn from_error_code(code: u64) -> Self {
        match code {
            // The connection closed without an error
            code if code == TransportError::NO_ERROR.code.as_u64() => Self::Closed,
            // The connection closed without an error at the application layer
            code if code == TransportError::APPLICATION_ERROR.code.as_u64() => Self::Closed,
            // The connection closed with an actual error
            code => Self::Transport(code),
        }
    }
}

impl ApplicationErrorExt for Error {
    fn application_error_code(&self) -> Option<ApplicationErrorCode> {
        if let Self::Application(error_code) = self {
            Some(*error_code)
        } else {
            None
        }
    }
}

impl From<ApplicationErrorCode> for Error {
    fn from(error_code: ApplicationErrorCode) -> Self {
        Self::Application(error_code)
    }
}

impl From<TransportError> for Error {
    fn from(error: TransportError) -> Self {
        if let Some(error_code) = error.application_error_code() {
            error_code.into()
        } else {
            Self::from_error_code(error.code.as_u64())
        }
    }
}

impl<'a> From<ConnectionClose<'a>> for Error {
    fn from(error: ConnectionClose) -> Self {
        if let Some(error_code) = error.application_error_code() {
            error_code.into()
        } else {
            Self::from_error_code(error.error_code.as_u64())
        }
    }
}

#[cfg(feature = "std")]
impl From<Error> for std::io::Error {
    fn from(error: Error) -> Self {
        let kind = error.into();
        std::io::Error::new(kind, error)
    }
}

#[cfg(feature = "std")]
impl From<Error> for std::io::ErrorKind {
    fn from(error: Error) -> Self {
        use std::io::ErrorKind;
        match error {
            Error::Closed => ErrorKind::ConnectionAborted,
            Error::Transport(code) if code == TransportError::CONNECTION_REFUSED.code.as_u64() => {
                ErrorKind::ConnectionRefused
            }
            Error::Transport(_) => ErrorKind::ConnectionReset,
            Error::Application(_) => ErrorKind::ConnectionReset,
            Error::IdleTimerExpired => ErrorKind::TimedOut,
            Error::StreamIdExhausted => ErrorKind::Other,
            Error::Unspecified => ErrorKind::Other,
        }
    }
}

/// Some connection methods may need to indicate both `TransportError`s and `CryptoError`s. This
/// enum is used to allow for either error type to be returned as appropriate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProcessingError {
    DuplicatePacket,
    TransportError(TransportError),
    CryptoError(CryptoError),
}

impl From<TransportError> for ProcessingError {
    fn from(inner_error: TransportError) -> Self {
        ProcessingError::TransportError(inner_error)
    }
}

impl From<CryptoError> for ProcessingError {
    fn from(inner_error: CryptoError) -> Self {
        ProcessingError::CryptoError(inner_error)
    }
}
