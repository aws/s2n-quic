// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{application, connection, frame::ConnectionClose, transport};
use core::fmt;

/// Errors that a stream can encounter.
#[derive(PartialEq, Eq, Debug, Copy, Clone)]
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
#[non_exhaustive]
pub enum StreamError {
    /// The Stream ID which was referenced is invalid
    ///
    /// This could mean the ID is no longer tracked by the Connection.
    InvalidStream,
    /// The Stream had been reset by the peer via a `RESET_STREAM` frame.
    ///
    /// Inside this frame the peer will deliver an error code, which will be
    /// provided by the parameter.
    StreamReset(application::Error),
    /// A send attempt had been performed on a Stream after it was closed
    SendAfterFinish,
    /// Attempting to write data would exceed the stream limit
    ///
    /// This is caused because the maximum possible amount
    /// of data (2^62-1 bytes) had already been written to the
    /// Stream.
    MaxStreamDataSizeExceeded,
    /// The Stream was reset due to a Connection Error
    ConnectionError(connection::Error),
    /// The stream is not readable
    NonReadable,
    /// The stream is not writable
    NonWritable,
    /// The stream is blocked on writing data
    ///
    /// This is caused by trying to send data before polling readiness
    SendingBlocked,
    /// The stream was provided a non-empty placeholder buffer for receiving data.
    ///
    /// The application should ensure only empty buffers are provided to receive calls,
    /// otherwise it can lead to data loss on the stream.
    NonEmptyOutput,
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::InvalidStream => write!(f, "The Stream ID which was referenced is invalid"),
            Self::StreamReset(error) => write!(
                f,
                "The Stream had been reset with the error {:?} by {}",
                error,
                crate::endpoint::Location::Remote,
            ),
            Self::SendAfterFinish => write!(
                f,
                "A send attempt had been performed on a Stream after it was closed"
            ),
            Self::MaxStreamDataSizeExceeded => {
                write!(f, "Attempting to write data would exceed the stream limit")
            }
            Self::ConnectionError(error) => error.fmt(f),
            Self::NonReadable => write!(f, "The stream is not readable"),
            Self::NonWritable => write!(f, "The stream is not writable"),
            Self::SendingBlocked => write!(f, "The stream is blocked on writing data"),
            Self::NonEmptyOutput => write!(
                f,
                "The stream was provided a non-empty placeholder buffer for receiving data."
            ),
        }
    }
}

impl application::error::TryInto for StreamError {
    fn application_error(&self) -> Option<application::Error> {
        if let StreamError::ConnectionError(error) = self {
            error.application_error()
        } else {
            None
        }
    }
}

impl From<connection::Error> for StreamError {
    fn from(error: connection::Error) -> Self {
        Self::ConnectionError(error)
    }
}

impl From<transport::Error> for StreamError {
    fn from(error: transport::Error) -> Self {
        Self::ConnectionError(error.into())
    }
}

impl<'a> From<ConnectionClose<'a>> for StreamError {
    fn from(error: ConnectionClose) -> Self {
        Self::ConnectionError(error.into())
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
            StreamError::InvalidStream => ErrorKind::NotFound,
            StreamError::StreamReset(_) => ErrorKind::ConnectionReset,
            StreamError::SendAfterFinish => ErrorKind::BrokenPipe,
            StreamError::MaxStreamDataSizeExceeded => ErrorKind::Other,
            StreamError::ConnectionError(error) => error.into(),
            StreamError::NonReadable => ErrorKind::Other,
            StreamError::NonWritable => ErrorKind::Other,
            StreamError::SendingBlocked => ErrorKind::WouldBlock,
            StreamError::NonEmptyOutput => ErrorKind::InvalidInput,
        }
    }
}
