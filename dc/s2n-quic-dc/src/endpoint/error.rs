// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::varint::VarInt;
use std::fmt;

/// The acceptor ID specified in the init frame was not registered
pub const ACCEPTOR_NOT_FOUND: VarInt = VarInt::from_u32(1);

/// Failed to decode a control frame payload
pub const FRAME_DECODE_ERROR: VarInt = VarInt::from_u32(2);

/// The sender terminated abnormally (e.g., panic, crash, process exit)
pub const ABNORMAL_TERMINATION: VarInt = VarInt::from_u32(3);

/// The receiver no longer wants to receive data (graceful cancel)
pub const STOP_SENDING: VarInt = VarInt::from_u32(4);

/// All retransmission attempts exhausted without acknowledgment
pub const RETRANSMISSIONS_EXHAUSTED: VarInt = VarInt::from_u32(5);

/// Server accept queue was full — stream dropped before the application could handle it
pub const SERVER_BUSY: VarInt = VarInt::from_u32(6);

/// The sender exceeded the advertised receive window
pub const QUEUE_CONTROL_ERROR: VarInt = VarInt::from_u32(7);

/// The peer was declared dead after its idle timeout elapsed without activity
pub const IDLE_TIMEOUT: VarInt = VarInt::from_u32(8);

/// The sender cancelled the stream before completing a partially-sent message
pub const SENDER_CANCELLED: VarInt = VarInt::from_u32(9);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    AcceptorNotFound,
    FrameDecodeError,
    AbnormalTermination,
    StopSending,
    RetransmissionsExhausted,
    ServerBusy,
    QueueControlError,
    IdleTimeout,
    SenderCancelled,
    Unknown(VarInt),
}

impl Error {
    pub fn io_error_kind(self) -> std::io::ErrorKind {
        match self {
            Self::IdleTimeout => std::io::ErrorKind::TimedOut,
            _ => std::io::ErrorKind::ConnectionReset,
        }
    }

    pub fn as_varint(self) -> VarInt {
        match self {
            Self::AcceptorNotFound => ACCEPTOR_NOT_FOUND,
            Self::FrameDecodeError => FRAME_DECODE_ERROR,
            Self::AbnormalTermination => ABNORMAL_TERMINATION,
            Self::StopSending => STOP_SENDING,
            Self::RetransmissionsExhausted => RETRANSMISSIONS_EXHAUSTED,
            Self::ServerBusy => SERVER_BUSY,
            Self::QueueControlError => QUEUE_CONTROL_ERROR,
            Self::IdleTimeout => IDLE_TIMEOUT,
            Self::SenderCancelled => SENDER_CANCELLED,
            Self::Unknown(code) => code,
        }
    }
}

impl From<VarInt> for Error {
    fn from(code: VarInt) -> Self {
        match code {
            ACCEPTOR_NOT_FOUND => Self::AcceptorNotFound,
            FRAME_DECODE_ERROR => Self::FrameDecodeError,
            ABNORMAL_TERMINATION => Self::AbnormalTermination,
            STOP_SENDING => Self::StopSending,
            RETRANSMISSIONS_EXHAUSTED => Self::RetransmissionsExhausted,
            SERVER_BUSY => Self::ServerBusy,
            QUEUE_CONTROL_ERROR => Self::QueueControlError,
            IDLE_TIMEOUT => Self::IdleTimeout,
            SENDER_CANCELLED => Self::SenderCancelled,
            _ => Self::Unknown(code),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AcceptorNotFound => write!(f, "acceptor ID not registered"),
            Self::FrameDecodeError => write!(f, "failed to decode control frame"),
            Self::AbnormalTermination => write!(f, "sender terminated abnormally"),
            Self::StopSending => write!(f, "receiver cancelled the stream"),
            Self::RetransmissionsExhausted => write!(f, "retransmission attempts exhausted"),
            Self::ServerBusy => write!(f, "server accept queue full"),
            Self::QueueControlError => write!(f, "sender exceeded receive window"),
            Self::IdleTimeout => write!(f, "peer idle timeout expired"),
            Self::SenderCancelled => write!(f, "sender cancelled mid-message"),
            Self::Unknown(code) => write!(f, "unknown error ({})", code.as_u64()),
        }
    }
}

impl std::error::Error for Error {}
