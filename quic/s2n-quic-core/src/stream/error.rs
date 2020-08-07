use crate::{
    application::{ApplicationErrorCode, ApplicationErrorExt},
    connection::ConnectionError,
    frame::ConnectionClose,
    transport::error::TransportError,
};

/// Errors that a stream can encounter.
#[derive(PartialEq, Debug, Copy, Clone, displaydoc::Display)]
#[cfg_attr(feature = "thiserror", derive(thiserror::Error))]
#[non_exhaustive]
pub enum StreamError {
    /// The Stream ID which was referenced is invalid and/or no longer tracked
    /// by the Connection.
    InvalidStream,
    /// The Stream had been reset by the peer via a `RESET_STREAM` frame.
    ///
    /// Inside this frame the peer will deliver an error code, which will be
    /// provided by the parameter.
    StreamReset(ApplicationErrorCode),
    /// A send attempt had been performed on a Stream after the Stream was
    /// already closed.
    WriterAfterFinish,
    /// Data could not be written to a stream, because the maximum possible amount
    /// of data (2^62-1 bytes) had already been writtten to the
    /// Stream.
    MaxStreamDataSizeExceeded,
    /// The Stream was reset due to a Connection Error
    ConnectionError(ConnectionError),
    /// All Stream IDs for Streams on a given connection had been exhausted
    StreamIdExhausted,
    /// The stream is not readable
    NonReadable,
    /// The stream is not writable
    NonWritable,
}

impl ApplicationErrorExt for StreamError {
    fn application_error_code(&self) -> Option<ApplicationErrorCode> {
        if let StreamError::ConnectionError(error) = self {
            error.application_error_code()
        } else {
            None
        }
    }
}

impl From<ConnectionError> for StreamError {
    fn from(error: ConnectionError) -> Self {
        Self::ConnectionError(error)
    }
}

impl From<ApplicationErrorCode> for StreamError {
    fn from(error: ApplicationErrorCode) -> Self {
        Self::ConnectionError(error.into())
    }
}

impl From<TransportError> for StreamError {
    fn from(error: TransportError) -> Self {
        Self::ConnectionError(error.into())
    }
}

impl<'a> From<ConnectionClose<'a>> for StreamError {
    fn from(error: ConnectionClose) -> Self {
        Self::ConnectionError(error.into())
    }
}
