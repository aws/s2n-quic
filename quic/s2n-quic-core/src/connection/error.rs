use crate::{
    application::{ApplicationErrorCode, ApplicationErrorExt},
    frame::ConnectionClose,
    transport::error::TransportError,
};

/// Errors that a connection can encounter.
#[derive(PartialEq, Debug, Copy, Clone, displaydoc::Display)]
#[non_exhaustive]
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
pub enum Error {
    /// The connection was closed on application level either locally or
    /// by the peer. The argument contains the error code which the application
    /// supplied in order to close the connection.
    ConnectionClosed(ApplicationErrorCode),
    /// The connection was closed because the connection's idle timer expired
    IdleTimerExpired,
    /// All Stream IDs for Streams on a given connection had been exhausted
    StreamIdExhausted,
    /// The connection was closed due to an unspecified reason
    Unspecified,
}

impl ApplicationErrorExt for Error {
    fn application_error_code(&self) -> Option<ApplicationErrorCode> {
        if let Self::ConnectionClosed(error_code) = self {
            Some(*error_code)
        } else {
            None
        }
    }
}

impl From<ApplicationErrorCode> for Error {
    fn from(error_code: ApplicationErrorCode) -> Self {
        Self::ConnectionClosed(error_code)
    }
}

impl From<TransportError> for Error {
    fn from(error: TransportError) -> Self {
        if let Some(error_code) = error.application_error_code() {
            Self::ConnectionClosed(error_code)
        } else {
            Self::Unspecified
        }
    }
}

impl<'a> From<ConnectionClose<'a>> for Error {
    fn from(error: ConnectionClose) -> Self {
        if let Some(error_code) = error.application_error_code() {
            Self::ConnectionClosed(error_code)
        } else {
            Self::Unspecified
        }
    }
}
