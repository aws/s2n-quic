// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{crypto::encrypt, stream::packet_number};
use s2n_quic_core::{buffer, varint::VarInt};

#[derive(Clone, Copy, Debug, thiserror::Error)]
pub enum Error {
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
    #[error("the crypto was retired by the peer")]
    CryptoRetired,
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

impl From<Error> for std::io::Error {
    #[inline]
    fn from(error: Error) -> Self {
        use std::io::ErrorKind;
        let kind = match error {
            Error::PayloadTooLarge => ErrorKind::BrokenPipe,
            Error::PacketBufferTooSmall => ErrorKind::InvalidInput,
            Error::PacketNumberExhaustion => ErrorKind::BrokenPipe,
            Error::RetransmissionFailure => ErrorKind::BrokenPipe,
            Error::StreamFinished => ErrorKind::UnexpectedEof,
            Error::FinalSizeChanged => ErrorKind::InvalidInput,
            Error::IdleTimeout => ErrorKind::TimedOut,
            Error::CryptoRetired => ErrorKind::ConnectionAborted,
            Error::ApplicationError { .. } => ErrorKind::ConnectionReset,
            Error::TransportError { .. } => ErrorKind::ConnectionAborted,
            Error::FrameError { .. } => ErrorKind::InvalidData,
            Error::FatalError => ErrorKind::BrokenPipe,
        };
        Self::new(kind, error)
    }
}

impl From<encrypt::Error> for Error {
    fn from(value: encrypt::Error) -> Self {
        match value {
            encrypt::Error::Retired => Self::CryptoRetired,
        }
    }
}

impl From<packet_number::ExhaustionError> for Error {
    #[inline]
    fn from(_error: packet_number::ExhaustionError) -> Self {
        Self::PacketNumberExhaustion
    }
}

impl From<buffer::Error<core::convert::Infallible>> for Error {
    #[inline]
    fn from(error: buffer::Error<core::convert::Infallible>) -> Self {
        match error {
            buffer::Error::OutOfRange => Self::PayloadTooLarge,
            buffer::Error::InvalidFin => Self::FinalSizeChanged,
            buffer::Error::ReaderError(_) => unreachable!(),
        }
    }
}
