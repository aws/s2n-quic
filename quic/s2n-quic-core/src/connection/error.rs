// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application, connection, crypto::CryptoError, endpoint, frame::ConnectionClose, transport,
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

/// Returns a CONNECTION_CLOSE frame for the given connection Error, if any
///
/// The first item will be a close frame for an early (initial, handshake) packet.
/// The second item will be a close frame for a 1-RTT (application data) packet.
pub fn as_frame<'a, F: connection::close::Formatter>(
    error: Error,
    formatter: &'a F,
    context: &'a connection::close::Context<'a>,
) -> Option<(ConnectionClose<'a>, ConnectionClose<'a>)> {
    match error {
        Error::Closed { initiator } => {
            // don't send CONNECTION_CLOSE frames on remote-initiated errors
            if initiator.is_remote() {
                return None;
            }

            let error = transport::Error::NO_ERROR;
            let early = formatter.format_early_transport_error(context, error);
            let one_rtt = formatter.format_transport_error(context, error);

            Some((early, one_rtt))
        }
        Error::Transport { error, initiator } => {
            // don't send CONNECTION_CLOSE frames on remote-initiated errors
            if initiator.is_remote() {
                return None;
            }

            let early = formatter.format_early_transport_error(context, error);
            let one_rtt = formatter.format_transport_error(context, error);
            Some((early, one_rtt))
        }
        Error::Application { error, initiator } => {
            // don't send CONNECTION_CLOSE frames on remote-initiated errors
            if initiator.is_remote() {
                return None;
            }

            let early = formatter.format_early_application_error(context, error);
            let one_rtt = formatter.format_application_error(context, error);
            Some((early, one_rtt))
        }
        // This error comes from the peer so we don't respond with a CONNECTION_CLOSE
        Error::StatelessReset => None,
        // Nothing gets sent on idle timeouts
        Error::IdleTimerExpired => None,
        Error::StreamIdExhausted => {
            let error =
                transport::Error::PROTOCOL_VIOLATION.with_reason("stream IDs have been exhausted");

            let early = formatter.format_early_transport_error(context, error);
            let one_rtt = formatter.format_transport_error(context, error);

            Some((early, one_rtt))
        }
        Error::Unspecified => {
            let error =
                transport::Error::INTERNAL_ERROR.with_reason("an unspecified error occurred");

            let early = formatter.format_early_transport_error(context, error);
            let one_rtt = formatter.format_transport_error(context, error);

            Some((early, one_rtt))
        }
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
        Self::from_transport_error(error, endpoint::Location::Local)
    }
}

impl From<CryptoError> for Error {
    fn from(error: CryptoError) -> Self {
        transport::Error::from(error).into()
    }
}

impl<'a> From<ConnectionClose<'a>> for Error {
    fn from(error: ConnectionClose) -> Self {
        if let Some(frame_type) = error.frame_type {
            let error = transport::Error {
                code: error.error_code.into(),
                // we use an empty `&'static str` so we don't allocate anything
                // in the event of an error
                reason: "",
                frame_type,
            };
            Self::from_transport_error(error, endpoint::Location::Remote)
        } else {
            Self::Application {
                error: error.error_code.into(),
                initiator: endpoint::Location::Remote,
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
