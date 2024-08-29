// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    credentials,
    crypto::open,
    packet::{self, stream},
    stream::TransportFeatures,
};
use core::{fmt, panic::Location};
use s2n_quic_core::{buffer, frame};

#[derive(Clone, Copy)]
pub struct Error {
    kind: Kind,
    location: &'static Location<'static>,
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Error")
            .field("kind", &self.kind)
            .field("crate", &"s2n-quic-dc")
            .field("file", &self.file())
            .field("line", &self.location.line())
            .finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self { kind, location } = self;
        let file = self.file();
        let line = location.line();
        write!(f, "[s2n-quic-dc::{file}:{line}]: {kind}")
    }
}

impl std::error::Error for Error {}

impl Error {
    #[track_caller]
    #[inline]
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            location: Location::caller(),
        }
    }

    #[inline]
    pub fn kind(&self) -> &Kind {
        &self.kind
    }

    #[inline]
    fn file(&self) -> &'static str {
        self.location
            .file()
            .trim_start_matches(concat!(env!("CARGO_MANIFEST_DIR"), "/src/"))
    }
}

impl From<Kind> for Error {
    #[track_caller]
    #[inline]
    fn from(kind: Kind) -> Self {
        Self::new(kind)
    }
}

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum Kind {
    #[error("could not decode packet")]
    Decode,
    #[error("could not decrypt packet: {0}")]
    Crypto(open::Error),
    #[error("packet has already been processed")]
    Duplicate,
    #[error("the packet was for another credential ({actual:?}) but was handled by {expected:?}")]
    CredentialMismatch {
        expected: credentials::Id,
        actual: credentials::Id,
    },
    #[error("the packet was for another stream ({actual}) but was handled by {expected}")]
    StreamMismatch {
        expected: stream::Id,
        actual: stream::Id,
    },
    #[error("the stream expected in-order delivery of {expected} but got {actual}")]
    OutOfOrder { expected: u64, actual: u64 },
    #[error("the peer exceeded the max data window")]
    MaxDataExceeded,
    #[error("invalid fin")]
    InvalidFin,
    #[error("out of range")]
    OutOfRange,
    #[error("unexpected retransmission packet")]
    UnexpectedRetransmission,
    #[error("the transport has been truncated without authentication")]
    TruncatedTransport,
    #[error("the receiver idle timer expired")]
    IdleTimeout,
    #[error("the crypto key has been replayed and is invalid")]
    KeyReplayPrevented,
    #[error("the crypto key has been potentially replayed (gap: {gap:?}) and is invalid")]
    KeyReplayMaybePrevented { gap: Option<u64> },
    #[error("application error: {error}")]
    ApplicationError {
        error: s2n_quic_core::application::Error,
    },
    #[error("unexpected packet: {packet:?}")]
    UnexpectedPacket { packet: packet::Kind },
}

impl Kind {
    #[inline]
    #[track_caller]
    pub(crate) fn err(self) -> Error {
        Error::new(self)
    }
}

impl From<open::Error> for Error {
    #[track_caller]
    fn from(value: open::Error) -> Self {
        match value {
            open::Error::ReplayDefinitelyDetected => Kind::KeyReplayPrevented,
            open::Error::ReplayPotentiallyDetected { gap } => Kind::KeyReplayMaybePrevented { gap },
            error => Kind::Crypto(error),
        }
        .err()
    }
}

impl Error {
    #[inline]
    pub(super) fn is_fatal(&self, features: &TransportFeatures) -> bool {
        // if the transport is a stream then any error we encounter is fatal, since the stream is
        // now likely corrupted
        if features.is_stream() {
            return true;
        }

        !matches!(
            self.kind(),
            Kind::Decode
                | Kind::Crypto(_)
                | Kind::Duplicate
                | Kind::CredentialMismatch { .. }
                | Kind::StreamMismatch { .. }
        )
    }

    #[inline]
    pub(super) fn connection_close(&self) -> Option<frame::ConnectionClose<'static>> {
        use s2n_quic_core::transport;
        match self.kind() {
            Kind::Decode
            | Kind::Crypto(_)
            | Kind::Duplicate
            | Kind::CredentialMismatch { .. }
            | Kind::StreamMismatch { .. }
            | Kind::UnexpectedPacket { .. }
            | Kind::UnexpectedRetransmission => {
                // return protocol violation for the errors that are only fatal for reliable
                // transports
                Some(transport::Error::PROTOCOL_VIOLATION.into())
            }
            Kind::IdleTimeout => None,
            Kind::MaxDataExceeded => Some(transport::Error::FLOW_CONTROL_ERROR.into()),
            Kind::InvalidFin | Kind::TruncatedTransport => {
                Some(transport::Error::FINAL_SIZE_ERROR.into())
            }
            Kind::OutOfOrder { .. } => Some(transport::Error::STREAM_STATE_ERROR.into()),
            Kind::OutOfRange => Some(transport::Error::STREAM_LIMIT_ERROR.into()),
            // we don't have working crypto keys so we can't respond
            Kind::KeyReplayPrevented | Kind::KeyReplayMaybePrevented { .. } => None,
            Kind::ApplicationError { error } => Some((*error).into()),
        }
    }
}

impl From<buffer::Error<Error>> for Error {
    #[inline]
    #[track_caller]
    fn from(value: buffer::Error<Error>) -> Self {
        match value {
            buffer::Error::OutOfRange => Kind::OutOfRange.err(),
            buffer::Error::InvalidFin => Kind::InvalidFin.err(),
            buffer::Error::ReaderError(error) => error,
        }
    }
}

impl From<Error> for std::io::Error {
    #[inline]
    fn from(error: Error) -> Self {
        Self::new(error.kind.into(), error)
    }
}

impl From<Kind> for std::io::ErrorKind {
    #[inline]
    fn from(kind: Kind) -> Self {
        use std::io::ErrorKind;
        match kind {
            Kind::Decode => ErrorKind::InvalidData,
            Kind::Crypto(_) => ErrorKind::InvalidData,
            Kind::Duplicate => ErrorKind::InvalidData,
            Kind::CredentialMismatch { .. } | Kind::StreamMismatch { .. } => ErrorKind::InvalidData,
            Kind::MaxDataExceeded => ErrorKind::ConnectionAborted,
            Kind::InvalidFin => ErrorKind::InvalidData,
            Kind::TruncatedTransport => ErrorKind::UnexpectedEof,
            Kind::OutOfRange => ErrorKind::ConnectionAborted,
            Kind::OutOfOrder { .. } => ErrorKind::InvalidData,
            Kind::UnexpectedRetransmission { .. } => ErrorKind::InvalidData,
            Kind::IdleTimeout => ErrorKind::TimedOut,
            Kind::KeyReplayPrevented => ErrorKind::PermissionDenied,
            Kind::KeyReplayMaybePrevented { .. } => ErrorKind::PermissionDenied,
            Kind::ApplicationError { .. } => ErrorKind::ConnectionReset,
            Kind::UnexpectedPacket {
                packet:
                    packet::Kind::UnknownPathSecret
                    | packet::Kind::StaleKey
                    | packet::Kind::ReplayDetected,
            } => ErrorKind::ConnectionRefused,
            Kind::UnexpectedPacket {
                packet: packet::Kind::Stream | packet::Kind::Control | packet::Kind::Datagram,
            } => ErrorKind::InvalidData,
        }
    }
}
