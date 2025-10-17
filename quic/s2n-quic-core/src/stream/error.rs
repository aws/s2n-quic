// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{application, connection, frame::ConnectionClose, transport};
use core::{fmt, panic};

/// Errors that a stream can encounter.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
#[non_exhaustive]
pub enum StreamError {
    /// The Stream ID which was referenced is invalid
    ///
    /// This could mean the ID is no longer tracked by the Connection.
    #[non_exhaustive]
    InvalidStream {
        source: &'static panic::Location<'static>,
    },
    /// The Stream had been reset by the peer via a `RESET_STREAM` frame.
    ///
    /// Inside this frame the peer will deliver an error code, which will be
    /// provided by the parameter.
    #[non_exhaustive]
    StreamReset {
        error: application::Error,
        source: &'static panic::Location<'static>,
    },
    /// A send attempt had been performed on a Stream after it was closed
    #[non_exhaustive]
    SendAfterFinish {
        source: &'static panic::Location<'static>,
    },
    /// Attempting to write data would exceed the stream limit
    ///
    /// This is caused because the maximum possible amount
    /// of data (2^62-1 bytes) had already been written to the
    /// Stream.
    #[non_exhaustive]
    MaxStreamDataSizeExceeded {
        source: &'static panic::Location<'static>,
    },
    /// The Stream was reset due to a Connection Error
    #[non_exhaustive]
    ConnectionError { error: connection::Error },
    /// The stream is not readable
    #[non_exhaustive]
    NonReadable {
        source: &'static panic::Location<'static>,
    },
    /// The stream is not writable
    #[non_exhaustive]
    NonWritable {
        source: &'static panic::Location<'static>,
    },
    /// The stream is blocked on writing data
    ///
    /// This is caused by trying to send data before polling readiness
    #[non_exhaustive]
    SendingBlocked {
        source: &'static panic::Location<'static>,
    },
    /// The stream was provided a non-empty placeholder buffer for receiving data.
    ///
    /// The application should ensure only empty buffers are provided to receive calls,
    /// otherwise it can lead to data loss on the stream.
    #[non_exhaustive]
    NonEmptyOutput {
        source: &'static panic::Location<'static>,
    },
}

impl core::error::Error for StreamError {}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidStream { .. } => {
                write!(f, "The Stream ID which was referenced is invalid")
            }
            Self::StreamReset { error, .. } => write!(
                f,
                "The Stream had been reset with the error {:?} by {}",
                error,
                crate::endpoint::Location::Remote,
            ),
            Self::SendAfterFinish { .. } => write!(
                f,
                "A send attempt had been performed on a Stream after it was closed"
            ),
            Self::MaxStreamDataSizeExceeded { .. } => {
                write!(f, "Attempting to write data would exceed the stream limit")
            }
            Self::ConnectionError { error, .. } => error.fmt(f),
            Self::NonReadable { .. } => write!(f, "The stream is not readable"),
            Self::NonWritable { .. } => write!(f, "The stream is not writable"),
            Self::SendingBlocked { .. } => write!(f, "The stream is blocked on writing data"),
            Self::NonEmptyOutput { .. } => write!(
                f,
                "The stream was provided a non-empty placeholder buffer for receiving data."
            ),
        }
    }
}

impl StreamError {
    /// Returns the [`panic::Location`] for the error
    pub fn source(&self) -> &'static panic::Location<'static> {
        match self {
            StreamError::InvalidStream { source } => source,
            StreamError::StreamReset { source, .. } => source,
            StreamError::SendAfterFinish { source } => source,
            StreamError::MaxStreamDataSizeExceeded { source } => source,
            StreamError::ConnectionError { error } => error.source(),
            StreamError::NonReadable { source } => source,
            StreamError::NonWritable { source } => source,
            StreamError::SendingBlocked { source } => source,
            StreamError::NonEmptyOutput { source } => source,
        }
    }

    #[track_caller]
    #[inline]
    #[doc(hidden)]
    pub fn invalid_stream() -> StreamError {
        let source = panic::Location::caller();
        StreamError::InvalidStream { source }
    }

    #[track_caller]
    #[inline]
    #[doc(hidden)]
    pub fn stream_reset(error: application::Error) -> StreamError {
        let source = panic::Location::caller();
        StreamError::StreamReset { source, error }
    }

    #[track_caller]
    #[inline]
    #[doc(hidden)]
    pub fn send_after_finish() -> StreamError {
        let source = panic::Location::caller();
        StreamError::SendAfterFinish { source }
    }

    #[track_caller]
    #[inline]
    #[doc(hidden)]
    pub fn max_stream_data_size_exceeded() -> StreamError {
        let source = panic::Location::caller();
        StreamError::MaxStreamDataSizeExceeded { source }
    }

    #[track_caller]
    #[inline]
    #[doc(hidden)]
    pub fn non_readable() -> StreamError {
        let source = panic::Location::caller();
        StreamError::NonReadable { source }
    }

    #[track_caller]
    #[inline]
    #[doc(hidden)]
    pub fn non_writable() -> StreamError {
        let source = panic::Location::caller();
        StreamError::NonWritable { source }
    }

    #[track_caller]
    #[inline]
    #[doc(hidden)]
    pub fn sending_blocked() -> StreamError {
        let source = panic::Location::caller();
        StreamError::SendingBlocked { source }
    }

    #[track_caller]
    #[inline]
    #[doc(hidden)]
    pub fn non_empty_output() -> StreamError {
        let source = panic::Location::caller();
        StreamError::NonEmptyOutput { source }
    }
}

impl application::error::TryInto for StreamError {
    fn application_error(&self) -> Option<application::Error> {
        if let StreamError::ConnectionError { error, .. } = self {
            error.application_error()
        } else {
            None
        }
    }
}

impl From<connection::Error> for StreamError {
    fn from(error: connection::Error) -> Self {
        Self::ConnectionError { error }
    }
}

impl From<transport::Error> for StreamError {
    #[track_caller]
    fn from(error: transport::Error) -> Self {
        let error: connection::Error = error.into();
        error.into()
    }
}

impl From<ConnectionClose<'_>> for StreamError {
    #[track_caller]
    fn from(error: ConnectionClose) -> Self {
        let error: connection::Error = error.into();
        error.into()
    }
}

#[cfg(feature = "std")]
impl From<StreamError> for std::io::Error {
    fn from(error: StreamError) -> Self {
        let kind = error.into();
        std::io::Error::new(kind, error)
    }
}

#[cfg(feature = "std")]
impl From<StreamError> for std::io::ErrorKind {
    fn from(error: StreamError) -> Self {
        use std::io::ErrorKind;
        match error {
            StreamError::InvalidStream { .. } => ErrorKind::NotFound,
            StreamError::StreamReset { .. } => ErrorKind::ConnectionReset,
            StreamError::SendAfterFinish { .. } => ErrorKind::BrokenPipe,
            StreamError::MaxStreamDataSizeExceeded { .. } => ErrorKind::Other,
            StreamError::ConnectionError { error, .. } => error.into(),
            StreamError::NonReadable { .. } => ErrorKind::Other,
            StreamError::NonWritable { .. } => ErrorKind::Other,
            StreamError::SendingBlocked { .. } => ErrorKind::WouldBlock,
            StreamError::NonEmptyOutput { .. } => ErrorKind::InvalidInput,
        }
    }
}
