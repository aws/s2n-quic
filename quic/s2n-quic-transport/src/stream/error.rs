use s2n_quic_core::{
    application::ApplicationErrorCode, frame::ConnectionClose, transport::error::TransportError,
};

/// Errors that a stream can encounter.
#[derive(PartialEq, Debug, Copy, Clone)]
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
    /// The Stream had been reset because the connection was closed on application
    /// level either locally or by the peer. The argument contains the error code
    /// which the application supplied in order to close the connection.
    ConnectionClosed(ApplicationErrorCode),
    /// The connection was closed because the connections idle timer expired
    IdleTimerExpired,
    /// A send attempt had been performed on a Stream after the Stream was
    /// already closed.
    WriterAfterFinish,
    /// Data could not be written to a stream, because the maximum possible amount
    /// of data (4611686018427387903 bytes) had already been writtten to the
    /// Stream.
    MaxStreamDataSizeExceeded,
    /// The Stream was reset the due to a Connection Error
    ConnectionError,
    /// All Stream IDs for Streams on a given connection had been exhausted
    StreamIdExhausted,
    /// The stream is not readable
    NonReadable,
    /// The stream is not writable
    NonWritable,
}

impl StreamError {
    /// If the Stream errored since the connection was closed with an
    /// [`ApplicationErrorCode`], this returns the utilized error code.
    pub fn as_application_protocol_error_code(self) -> Option<ApplicationErrorCode> {
        if let StreamError::ConnectionClosed(error_code) = self {
            Some(error_code)
        } else {
            None
        }
    }
}

impl From<TransportError> for StreamError {
    fn from(error: TransportError) -> Self {
        // Derive the Stream error from the `TransportError`. If a `TransportError`
        // contains no frame type it was sent by an application and contains
        // an `ApplicationLevelErrorCode`. Otherwise it is an error on the QUIC layer.
        if error.frame_type.is_none() {
            // This is an application error
            StreamError::ConnectionClosed(error.code.into())
        } else {
            // This is a QUIC error
            StreamError::ConnectionError
        }
    }
}

impl<'a> From<ConnectionClose<'a>> for StreamError {
    fn from(error: ConnectionClose) -> Self {
        // Derive the Stream error from the `ConnectionClose`. If a `ConnectionClose`
        // contains no frame type it was sent by an application and contains
        // an `ApplicationLevelErrorCode`. Otherwise it is an error on the QUIC layer.
        if error.frame_type.is_none() {
            // This is an application error
            StreamError::ConnectionClosed(error.error_code.into())
        } else {
            // This is a QUIC error
            StreamError::ConnectionError
        }
    }
}
