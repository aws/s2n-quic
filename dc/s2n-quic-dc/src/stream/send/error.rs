// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::stream::packet_number;
use core::{fmt, panic::Location};
use s2n_quic_core::{buffer, varint::VarInt};

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
    #[error("payload provided is too large and exceeded the maximum offset")]
    PayloadTooLarge,
    #[error("the provided packet buffer is too small for the minimum packet size")]
    PacketBufferTooSmall,
    #[error("the number of packets able to be sent on the sender has been exceeded")]
    PacketNumberExhaustion,
    #[error("retransmission not possible")]
    RetransmissionFailure,
    #[error("stream has been finished")]
    StreamFinished,
    #[error("the final size of the stream has changed")]
    FinalSizeChanged,
    #[error("the sender idle timer expired")]
    IdleTimeout,
    #[error("the stream was reset by the peer with code {code}")]
    TransportError { code: VarInt },
    #[error("the stream was closed with application code {error}")]
    ApplicationError {
        error: s2n_quic_core::application::Error,
    },
    #[error("an invalid frame was received: {decoder}")]
    FrameError { decoder: s2n_codec::DecoderError },
    #[error("the stream experienced an unrecoverable error")]
    FatalError,
}

impl Kind {
    #[inline]
    #[track_caller]
    pub(crate) fn err(self) -> Error {
        Error::new(self)
    }
}

impl From<Error> for std::io::Error {
    #[inline]
    #[track_caller]
    fn from(error: Error) -> Self {
        Self::new(error.kind.into(), error)
    }
}

impl From<Kind> for std::io::ErrorKind {
    #[inline]
    fn from(kind: Kind) -> Self {
        use std::io::ErrorKind;
        match kind {
            Kind::PayloadTooLarge => ErrorKind::BrokenPipe,
            Kind::PacketBufferTooSmall => ErrorKind::InvalidInput,
            Kind::PacketNumberExhaustion => ErrorKind::BrokenPipe,
            Kind::RetransmissionFailure => ErrorKind::BrokenPipe,
            Kind::StreamFinished => ErrorKind::UnexpectedEof,
            Kind::FinalSizeChanged => ErrorKind::InvalidInput,
            Kind::IdleTimeout => ErrorKind::TimedOut,
            Kind::ApplicationError { .. } => ErrorKind::ConnectionReset,
            Kind::TransportError { .. } => ErrorKind::ConnectionAborted,
            Kind::FrameError { .. } => ErrorKind::InvalidData,
            Kind::FatalError => ErrorKind::BrokenPipe,
        }
    }
}

impl From<packet_number::ExhaustionError> for Error {
    #[inline]
    #[track_caller]
    fn from(_error: packet_number::ExhaustionError) -> Self {
        Kind::PacketNumberExhaustion.err()
    }
}

impl From<buffer::Error<core::convert::Infallible>> for Error {
    #[inline]
    #[track_caller]
    fn from(error: buffer::Error<core::convert::Infallible>) -> Self {
        match error {
            buffer::Error::OutOfRange => Kind::PayloadTooLarge.err(),
            buffer::Error::InvalidFin => Kind::FinalSizeChanged.err(),
            buffer::Error::ReaderError(_) => unreachable!(),
        }
    }
}
