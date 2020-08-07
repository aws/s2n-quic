use crate::{
    application::{ApplicationErrorCode, ApplicationErrorExt},
    frame::ConnectionClose,
    transport::error::TransportError,
};

/// Errors that a connection can encounter.
#[derive(PartialEq, Debug, Copy, Clone, displaydoc::Display)]
#[non_exhaustive]
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
pub enum ConnectionError {
    /// The connection was closed on application level either locally or
    /// by the peer. The argument contains the error code which the application
    /// supplied in order to close the connection.
    ConnectionClosed(ApplicationErrorCode),
    /// The connection was closed because the connection's idle timer expired
    IdleTimerExpired,
    /// The connection was closed due to an unspecified reason
    Unspecified,
}

impl ApplicationErrorExt for ConnectionError {
    fn application_error_code(&self) -> Option<ApplicationErrorCode> {
        if let ConnectionError::ConnectionClosed(error_code) = self {
            Some(*error_code)
        } else {
            None
        }
    }
}

impl From<ApplicationErrorCode> for ConnectionError {
    fn from(error_code: ApplicationErrorCode) -> Self {
        ConnectionError::ConnectionClosed(error_code)
    }
}

impl From<TransportError> for ConnectionError {
    fn from(error: TransportError) -> Self {
        if let Some(error_code) = error.application_error_code() {
            ConnectionError::ConnectionClosed(error_code)
        } else {
            ConnectionError::Unspecified
        }
    }
}

impl<'a> From<ConnectionClose<'a>> for ConnectionError {
    fn from(error: ConnectionClose) -> Self {
        if let Some(error_code) = error.application_error_code() {
            ConnectionError::ConnectionClosed(error_code)
        } else {
            ConnectionError::Unspecified
        }
    }
}
