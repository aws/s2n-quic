// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    application, connection, crypto::CryptoError, endpoint, frame::ConnectionClose, transport,
};
use core::{convert::TryInto, fmt, panic, time::Duration};

/// Errors that a connection can encounter.
#[derive(Debug, Copy, Clone)]
#[non_exhaustive]
pub enum Error {
    /// The connection was closed without an error
    #[non_exhaustive]
    Closed {
        initiator: endpoint::Location,
        source: &'static panic::Location<'static>,
    },

    /// The connection was closed on the transport level
    ///
    /// This can occur either locally or by the peer. The argument contains
    /// the error code which the transport provided in order to close the
    /// connection.
    #[non_exhaustive]
    Transport {
        code: transport::error::Code,
        frame_type: u64,
        reason: &'static str,
        initiator: endpoint::Location,
        source: &'static panic::Location<'static>,
    },

    /// The connection was closed on the application level
    ///
    /// This can occur either locally or by the peer. The argument contains
    /// the error code which the application/ supplied in order to close the
    /// connection.
    #[non_exhaustive]
    Application {
        error: application::Error,
        initiator: endpoint::Location,
        source: &'static panic::Location<'static>,
    },

    /// The connection was reset by a stateless reset from the peer
    #[non_exhaustive]
    StatelessReset {
        source: &'static panic::Location<'static>,
    },

    /// The connection was closed because the local connection's idle timer expired
    #[non_exhaustive]
    IdleTimerExpired {
        source: &'static panic::Location<'static>,
    },

    /// The connection was closed because there are no valid paths
    #[non_exhaustive]
    NoValidPath {
        source: &'static panic::Location<'static>,
    },

    /// All Stream IDs for Streams on the given connection had been exhausted
    #[non_exhaustive]
    StreamIdExhausted {
        source: &'static panic::Location<'static>,
    },

    /// The handshake has taken longer to complete than the configured max handshake duration
    #[non_exhaustive]
    MaxHandshakeDurationExceeded {
        max_handshake_duration: Duration,
        source: &'static panic::Location<'static>,
    },

    /// The connection should be closed immediately without notifying the peer
    #[non_exhaustive]
    ImmediateClose {
        reason: &'static str,
        source: &'static panic::Location<'static>,
    },

    /// The connection attempt was rejected because the endpoint is closing
    #[non_exhaustive]
    EndpointClosing {
        source: &'static panic::Location<'static>,
    },

    /// The connection was closed due to an unspecified reason
    #[non_exhaustive]
    Unspecified {
        source: &'static panic::Location<'static>,
    },
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Closed { initiator, .. } => write!(
                f,
                "The connection was closed without an error by {initiator}"
            ),
            Self::Transport { code, frame_type, reason, initiator, .. } => {
                let error = transport::Error {
                    code: *code,
                    frame_type: (*frame_type).try_into().ok().unwrap_or_default(),
                    reason,
                };
                write!(
                    f,
                    "The connection was closed on the transport level with error {error} by {initiator}"
                )
            },
            Self::Application { error, initiator, .. } => write!(
                f,
                "The connection was closed on the application level with error {error:?} by {initiator}"
            ),
            Self::StatelessReset { .. } => write!(
                f,
                "The connection was reset by a stateless reset by {}",
                endpoint::Location::Remote
            ),
            Self::IdleTimerExpired {.. } => write!(
                f,
                "The connection was closed because the connection's idle timer expired by {}",
                endpoint::Location::Local
            ),
            Self::NoValidPath { .. } => write!(
                f,
                "The connection was closed because there are no valid paths"
            ),
            Self::StreamIdExhausted { .. } => write!(
                f,
                "All Stream IDs for Streams on the given connection had been exhausted"
            ),
            Self::MaxHandshakeDurationExceeded { max_handshake_duration, .. } => write!(
              f,
                "The connection was closed because the handshake took longer than the max handshake \
                duration of {max_handshake_duration:?}"
            ),
            Self::ImmediateClose { reason, .. } => write!(
                f,
                "The connection was closed due to: {reason}"
            ),
            Self::EndpointClosing { .. } => {
                write!(f, "The connection attempt was rejected because the endpoint is closing")
            }
            Self::Unspecified { .. } => {
                write!(f, "The connection was closed due to an unspecified reason")
            }
        }
    }
}

impl PartialEq for Error {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        // ignore the `source` attribute when considering if errors are equal
        match (self, other) {
            (Error::Closed { initiator: a, .. }, Error::Closed { initiator: b, .. }) => a.eq(b),
            (
                Error::Transport {
                    code: a_code,
                    frame_type: a_frame_type,
                    reason: a_reason,
                    initiator: a_initiator,
                    ..
                },
                Error::Transport {
                    code: b_code,
                    frame_type: b_frame_type,
                    reason: b_reason,
                    initiator: b_initiator,
                    ..
                },
            ) => {
                a_code.eq(b_code)
                    && a_frame_type.eq(b_frame_type)
                    && a_reason.eq(b_reason)
                    && a_initiator.eq(b_initiator)
            }
            (
                Error::Application {
                    error: a_error,
                    initiator: a_initiator,
                    ..
                },
                Error::Application {
                    error: b_error,
                    initiator: b_initiator,
                    ..
                },
            ) => a_error.eq(b_error) && a_initiator.eq(b_initiator),
            (Error::StatelessReset { .. }, Error::StatelessReset { .. }) => true,
            (Error::IdleTimerExpired { .. }, Error::IdleTimerExpired { .. }) => true,
            (Error::NoValidPath { .. }, Error::NoValidPath { .. }) => true,
            (Error::StreamIdExhausted { .. }, Error::StreamIdExhausted { .. }) => true,
            (
                Error::MaxHandshakeDurationExceeded {
                    max_handshake_duration: a,
                    ..
                },
                Error::MaxHandshakeDurationExceeded {
                    max_handshake_duration: b,
                    ..
                },
            ) => a.eq(b),
            (Error::ImmediateClose { reason: a, .. }, Error::ImmediateClose { reason: b, .. }) => {
                a.eq(b)
            }
            (Error::EndpointClosing { .. }, Error::EndpointClosing { .. }) => true,
            (Error::Unspecified { .. }, Error::Unspecified { .. }) => true,
            _ => false,
        }
    }
}

impl Eq for Error {}

impl Error {
    /// Returns the [`panic::Location`] for the error
    pub fn source(&self) -> &'static panic::Location<'static> {
        match self {
            Error::Closed { source, .. } => source,
            Error::Transport { source, .. } => source,
            Error::Application { source, .. } => source,
            Error::StatelessReset { source } => source,
            Error::IdleTimerExpired { source } => source,
            Error::NoValidPath { source } => source,
            Error::StreamIdExhausted { source } => source,
            Error::MaxHandshakeDurationExceeded { source, .. } => source,
            Error::ImmediateClose { source, .. } => source,
            Error::EndpointClosing { source } => source,
            Error::Unspecified { source } => source,
        }
    }

    #[track_caller]
    fn from_transport_error(error: transport::Error, initiator: endpoint::Location) -> Self {
        let source = panic::Location::caller();
        match error.code {
            // The connection closed without an error
            code if code == transport::Error::NO_ERROR.code => Self::Closed { initiator, source },
            // The connection closed without an error at the application layer
            code if code == transport::Error::APPLICATION_ERROR.code && initiator.is_remote() => {
                Self::Closed { initiator, source }
            }
            // The connection closed with an actual error
            _ => Self::Transport {
                code: error.code,
                frame_type: error.frame_type.into(),
                reason: error.reason,
                initiator,
                source,
            },
        }
    }

    #[inline]
    #[track_caller]
    #[doc(hidden)]
    pub fn closed(initiator: endpoint::Location) -> Error {
        let source = panic::Location::caller();
        Error::Closed { initiator, source }
    }

    #[inline]
    #[track_caller]
    #[doc(hidden)]
    pub fn immediate_close(reason: &'static str) -> Error {
        let source = panic::Location::caller();
        Error::ImmediateClose { reason, source }
    }

    #[inline]
    #[track_caller]
    #[doc(hidden)]
    pub fn idle_timer_expired() -> Error {
        let source = panic::Location::caller();
        Error::IdleTimerExpired { source }
    }

    #[inline]
    #[track_caller]
    #[doc(hidden)]
    pub fn stream_id_exhausted() -> Error {
        let source = panic::Location::caller();
        Error::StreamIdExhausted { source }
    }

    #[inline]
    #[track_caller]
    #[doc(hidden)]
    pub fn no_valid_path() -> Error {
        let source = panic::Location::caller();
        Error::NoValidPath { source }
    }

    #[inline]
    #[track_caller]
    #[doc(hidden)]
    pub fn stateless_reset() -> Error {
        let source = panic::Location::caller();
        Error::StatelessReset { source }
    }

    #[inline]
    #[track_caller]
    #[doc(hidden)]
    pub fn max_handshake_duration_exceeded(max_handshake_duration: Duration) -> Error {
        let source = panic::Location::caller();
        Error::MaxHandshakeDurationExceeded {
            max_handshake_duration,
            source,
        }
    }

    #[inline]
    #[track_caller]
    #[doc(hidden)]
    pub fn application(error: application::Error) -> Error {
        let source = panic::Location::caller();
        Error::Application {
            error,
            initiator: endpoint::Location::Local,
            source,
        }
    }

    #[inline]
    #[track_caller]
    #[doc(hidden)]
    pub fn endpoint_closing() -> Error {
        let source = panic::Location::caller();
        Error::EndpointClosing { source }
    }

    #[inline]
    #[track_caller]
    #[doc(hidden)]
    pub fn unspecified() -> Error {
        let source = panic::Location::caller();
        Error::Unspecified { source }
    }

    #[inline]
    #[doc(hidden)]
    pub fn into_accept_error(error: connection::Error) -> Result<(), connection::Error> {
        match error {
            // The connection closed without an error
            connection::Error::Closed { .. } => Ok(()),
            // The application closed the connection
            connection::Error::Transport { code, .. }
                if code == transport::Error::APPLICATION_ERROR.code =>
            {
                Ok(())
            }
            // The local connection's idle timer expired
            connection::Error::IdleTimerExpired { .. } => Ok(()),
            // Otherwise return the real error to the user
            _ => Err(error),
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
        Error::Closed { initiator, .. } => {
            // don't send CONNECTION_CLOSE frames on remote-initiated errors
            if initiator.is_remote() {
                return None;
            }

            let error = transport::Error::NO_ERROR;
            let early = formatter.format_early_transport_error(context, error);
            let one_rtt = formatter.format_transport_error(context, error);

            Some((early, one_rtt))
        }
        Error::Transport {
            code,
            frame_type,
            reason,
            initiator,
            ..
        } => {
            // don't send CONNECTION_CLOSE frames on remote-initiated errors
            if initiator.is_remote() {
                return None;
            }

            let error = transport::Error {
                code,
                frame_type: frame_type.try_into().unwrap_or_default(),
                reason,
            };

            let early = formatter.format_early_transport_error(context, error);
            let one_rtt = formatter.format_transport_error(context, error);
            Some((early, one_rtt))
        }
        Error::Application {
            error, initiator, ..
        } => {
            // don't send CONNECTION_CLOSE frames on remote-initiated errors
            if initiator.is_remote() {
                return None;
            }

            let early = formatter.format_early_application_error(context, error);
            let one_rtt = formatter.format_application_error(context, error);
            Some((early, one_rtt))
        }
        // This error comes from the peer so we don't respond with a CONNECTION_CLOSE
        Error::StatelessReset { .. } => None,
        // Nothing gets sent on idle timeouts
        Error::IdleTimerExpired { .. } => None,
        Error::NoValidPath { .. } => None,
        Error::StreamIdExhausted { .. } => {
            let error =
                transport::Error::PROTOCOL_VIOLATION.with_reason("stream IDs have been exhausted");

            let early = formatter.format_early_transport_error(context, error);
            let one_rtt = formatter.format_transport_error(context, error);

            Some((early, one_rtt))
        }
        Error::MaxHandshakeDurationExceeded { .. } => None,
        Error::ImmediateClose { .. } => None,
        Error::EndpointClosing { .. } => None,
        Error::Unspecified { .. } => {
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
    #[track_caller]
    fn from(error: transport::Error) -> Self {
        Self::from_transport_error(error, endpoint::Location::Local)
    }
}

impl From<CryptoError> for Error {
    #[track_caller]
    fn from(error: CryptoError) -> Self {
        transport::Error::from(error).into()
    }
}

impl<'a> From<ConnectionClose<'a>> for Error {
    #[track_caller]
    fn from(error: ConnectionClose) -> Self {
        if let Some(frame_type) = error.frame_type {
            let error = transport::Error {
                code: transport::error::Code::new(error.error_code),
                // we use an empty `&'static str` so we don't allocate anything
                // in the event of an error
                reason: "",
                frame_type,
            };
            Self::from_transport_error(error, endpoint::Location::Remote)
        } else {
            let source = panic::Location::caller();
            Self::Application {
                error: error.error_code.into(),
                initiator: endpoint::Location::Remote,
                source,
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
            Error::Transport { code, .. } if code == transport::Error::CONNECTION_REFUSED.code => {
                ErrorKind::ConnectionRefused
            }
            Error::Transport { .. } => ErrorKind::ConnectionReset,
            Error::Application { .. } => ErrorKind::ConnectionReset,
            Error::StatelessReset { .. } => ErrorKind::ConnectionReset,
            Error::IdleTimerExpired { .. } => ErrorKind::TimedOut,
            Error::NoValidPath { .. } => ErrorKind::Other,
            Error::StreamIdExhausted { .. } => ErrorKind::Other,
            Error::MaxHandshakeDurationExceeded { .. } => ErrorKind::TimedOut,
            Error::ImmediateClose { .. } => ErrorKind::Other,
            Error::EndpointClosing { .. } => ErrorKind::Other,
            Error::Unspecified { .. } => ErrorKind::Other,
        }
    }
}

/// Some connection methods may need to indicate both `TransportError`s and `CryptoError`s. This
/// enum is used to allow for either error type to be returned as appropriate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ProcessingError {
    ConnectionError(Error),
    DecryptError,
    Other,
}

impl From<Error> for ProcessingError {
    fn from(inner_error: Error) -> Self {
        ProcessingError::ConnectionError(inner_error)
    }
}

impl From<crate::transport::Error> for ProcessingError {
    #[track_caller]
    fn from(inner_error: crate::transport::Error) -> Self {
        // Try extracting out the decrypt error from other transport errors
        if let Some(error) = inner_error.try_into_crypto_error() {
            error.into()
        } else {
            Self::ConnectionError(inner_error.into())
        }
    }
}

impl From<CryptoError> for ProcessingError {
    fn from(inner_error: CryptoError) -> Self {
        if inner_error.code == CryptoError::DECRYPT_ERROR.code {
            Self::DecryptError
        } else {
            Self::ConnectionError(inner_error.into())
        }
    }
}
