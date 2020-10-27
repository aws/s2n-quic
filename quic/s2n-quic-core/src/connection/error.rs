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
        match error_code.into() {
            0u64 => Self::Closed,
            _ => Self::Application(error_code),
        }
    }
}

impl From<TransportError> for Error {
    fn from(error: TransportError) -> Self {
        if let Some(error_code) = error.application_error_code() {
            error_code.into()
        } else {
            match error.code.as_u64() {
                0 => Self::Closed,
                code => Self::Transport(code),
            }
        }
    }
}

impl<'a> From<ConnectionClose<'a>> for Error {
    fn from(error: ConnectionClose) -> Self {
        if let Some(error_code) = error.application_error_code() {
            error_code.into()
        } else {
            match error.error_code.as_u64() {
                0 => Self::Closed,
                code => Self::Transport(code),
            }
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
