// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application::{self, error::TryInto as _},
    crypto::CryptoError,
    endpoint,
    frame::ConnectionClose,
    transport,
};

/// Errors that a connection can encounter.
#[derive(PartialEq, Debug, Copy, Clone, displaydoc::Display)]
#[non_exhaustive]
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
pub enum Error {
    /// The connection was closed without an error
    Closed { initiator: endpoint::Location },

    /// The connection was closed on the transport level
    ///
    /// This can occur either locally or by the peer. The argument contains
    /// the error code which the transport provided in order to close the
    /// connection.
    Transport {
        error: transport::Error,
        initiator: endpoint::Location,
    },

    /// The connection was closed on the application level
    ///
    /// This can occur either locally or by the peer. The argument contains
    /// the error code which the application/ supplied in order to close the
    /// connection.
    Application {
        error: application::Error,
        initiator: endpoint::Location,
    },

    /// The connection was reset by a stateless reset from the peer
    StatelessReset,

    /// The connection was closed because the local connection's idle timer expired
    IdleTimerExpired,

    /// All Stream IDs for Streams on a given connection had been exhausted
    StreamIdExhausted,

    /// The connection was closed due to an unspecified reason
    Unspecified,
}

impl Error {
    fn from_transport_error(error: transport::Error, initiator: endpoint::Location) -> Self {
        match error.code {
            // The connection closed without an error
            code if code == transport::Error::NO_ERROR.code => Self::Closed { initiator },
            // The connection closed without an error at the application layer
            code if code == transport::Error::APPLICATION_ERROR.code && initiator.is_remote() => {
                Self::Closed { initiator }
            }
            // The connection closed with an actual error
            _ => Self::Transport { error, initiator },
        }
    }
}

/// Returns a CONNECTION_CLOSE frame for the given connection Error,
/// if any
pub fn as_frame(error: Error) -> Option<ConnectionClose<'static>> {
    match error {
        Error::Closed {
            initiator: endpoint::Location::Remote,
        }
        | Error::Transport {
            initiator: endpoint::Location::Remote,
            ..
        }
        | Error::Application {
            initiator: endpoint::Location::Remote,
            ..
        } => {
            // we don't send CONNECTION_CLOSE frames on remote-initiated errors
            None
        }
        Error::Closed { .. } => Some(ConnectionClose {
            error_code: *transport::Error::NO_ERROR.code,
            frame_type: None,
            reason: None,
        }),
        Error::Transport { error, .. } => Some(ConnectionClose {
            error_code: *error.code,
            frame_type: error.frame_type,
            reason: Some(error.reason.as_bytes()),
        }),
        Error::Application { error, .. } => Some(ConnectionClose {
            error_code: *error,
            frame_type: None,
            reason: None,
        }),
        // This error comes from the peer so we don't respond with a CONNECTION_CLOSE
        Error::StatelessReset => None,
        // Nothing gets sent on idle timeouts
        Error::IdleTimerExpired => None,
        Error::StreamIdExhausted => Some(ConnectionClose {
            error_code: *transport::Error::PROTOCOL_VIOLATION.code,
            frame_type: Some(Default::default()),
            reason: Some(b"stream ids exhausted"),
        }),
        Error::Unspecified if cfg!(debug_assertions) => Some(ConnectionClose {
            error_code: *transport::Error::INTERNAL_ERROR.code,
            frame_type: Some(Default::default()),
            reason: Some(b"unspecified error occurred"),
        }),
        Error::Unspecified => Some(ConnectionClose {
            error_code: *transport::Error::PROTOCOL_VIOLATION.code,
            frame_type: Some(Default::default()),
            reason: None,
        }),
    }
}

impl application::error::TryInto for Error {
    fn application_error(&self) -> Option<application::Error> {
        if let Self::Application { error, .. } = self {
            Some(*error)
        } else {
            None
        }
    }
}

impl From<transport::Error> for Error {
    fn from(error: transport::Error) -> Self {
        if let Some(error) = error.application_error() {
            Self::Application {
                error,
                initiator: endpoint::Location::Local,
            }
        } else {
            Self::from_transport_error(error, endpoint::Location::Local)
        }
    }
}

impl From<CryptoError> for Error {
    fn from(error: CryptoError) -> Self {
        transport::Error::from(error).into()
    }
}

impl<'a> From<ConnectionClose<'a>> for Error {
    fn from(error: ConnectionClose) -> Self {
        if let Some(error) = error.application_error() {
            Self::Application {
                error,
                initiator: endpoint::Location::Remote,
            }
        } else {
            let error = transport::Error {
                code: error.error_code.into(),
                // we use an empty `&'static str` so we don't allocate anything
                // in the event of an error
                reason: "",
                frame_type: error.frame_type,
            };
            Self::from_transport_error(error, endpoint::Location::Remote)
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
            Error::Closed { .. } => ErrorKind::ConnectionAborted,
            Error::Transport { error, .. }
                if error.code == transport::Error::CONNECTION_REFUSED.code =>
            {
                ErrorKind::ConnectionRefused
            }
            Error::Transport { .. } => ErrorKind::ConnectionReset,
            Error::Application { .. } => ErrorKind::ConnectionReset,
            Error::StatelessReset => ErrorKind::ConnectionReset,
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
    ConnectionError(Error),
    CryptoError(CryptoError),
}

impl From<Error> for ProcessingError {
    fn from(inner_error: Error) -> Self {
        ProcessingError::ConnectionError(inner_error)
    }
}

impl From<CryptoError> for ProcessingError {
    fn from(inner_error: CryptoError) -> Self {
        ProcessingError::CryptoError(inner_error)
    }
}
