// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::varint::VarInt;
use std::fmt;

/// Stream ID was reused before the previous flow completed
pub const BINDING_ID_ERROR: VarInt = VarInt::from_u32(1);

/// The acceptor ID specified in QueueInit was not found
pub const ACCEPTOR_NOT_FOUND: VarInt = VarInt::from_u32(2);

/// The queue ID is not allocated (no stream is registered at this slot)
pub const QUEUE_UNALLOCATED: VarInt = VarInt::from_u32(3);

/// Failed to decode control frames
pub const FRAME_DECODE_ERROR: VarInt = VarInt::from_u32(4);

/// The sender terminated abnormally (e.g., panic, crash)
pub const ABNORMAL_TERMINATION: VarInt = VarInt::from_u32(5);

/// The receiver no longer wants to receive data
pub const STOP_SENDING: VarInt = VarInt::from_u32(6);

/// Retransmissions exhausted after repeated transmission failures
pub const RETRANSMISSIONS_EXHAUSTED: VarInt = VarInt::from_u32(7);

/// Server accept queue overflowed - stream was dropped before the application could handle it
pub const SERVER_BUSY: VarInt = VarInt::from_u32(8);

/// The sender exceeded advertised flow-control credit
pub const QUEUE_CONTROL_ERROR: VarInt = VarInt::from_u32(9);

/// The binding_id in the packet doesn't match the queue's current occupant
pub const BINDING_ID_MISMATCH: VarInt = VarInt::from_u32(10);

/// The credential_id in the packet doesn't match the queue's owner
pub const CREDENTIAL_MISMATCH: VarInt = VarInt::from_u32(11);

/// Flow validation failed during the retry handshake
pub const QUEUE_VALIDATION_FAILED: VarInt = VarInt::from_u32(12);

/// The peer was declared dead after its idle timeout elapsed without activity
pub const IDLE_TIMEOUT: VarInt = VarInt::from_u32(13);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    BindingIdError,
    AcceptorNotFound,
    QueueUnallocated,
    FrameDecodeError,
    AbnormalTermination,
    StopSending,
    RetransmissionsExhausted,
    ServerBusy,
    QueueControlError,
    BindingIdMismatch,
    CredentialMismatch,
    QueueValidationFailed,
    IdleTimeout,
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
            Self::BindingIdError => BINDING_ID_ERROR,
            Self::AcceptorNotFound => ACCEPTOR_NOT_FOUND,
            Self::QueueUnallocated => QUEUE_UNALLOCATED,
            Self::FrameDecodeError => FRAME_DECODE_ERROR,
            Self::AbnormalTermination => ABNORMAL_TERMINATION,
            Self::StopSending => STOP_SENDING,
            Self::RetransmissionsExhausted => RETRANSMISSIONS_EXHAUSTED,
            Self::ServerBusy => SERVER_BUSY,
            Self::QueueControlError => QUEUE_CONTROL_ERROR,
            Self::BindingIdMismatch => BINDING_ID_MISMATCH,
            Self::CredentialMismatch => CREDENTIAL_MISMATCH,
            Self::QueueValidationFailed => QUEUE_VALIDATION_FAILED,
            Self::IdleTimeout => IDLE_TIMEOUT,
            Self::Unknown(code) => code,
        }
    }
}

impl From<VarInt> for Error {
    fn from(code: VarInt) -> Self {
        match code {
            BINDING_ID_ERROR => Self::BindingIdError,
            ACCEPTOR_NOT_FOUND => Self::AcceptorNotFound,
            QUEUE_UNALLOCATED => Self::QueueUnallocated,
            FRAME_DECODE_ERROR => Self::FrameDecodeError,
            ABNORMAL_TERMINATION => Self::AbnormalTermination,
            STOP_SENDING => Self::StopSending,
            RETRANSMISSIONS_EXHAUSTED => Self::RetransmissionsExhausted,
            SERVER_BUSY => Self::ServerBusy,
            QUEUE_CONTROL_ERROR => Self::QueueControlError,
            BINDING_ID_MISMATCH => Self::BindingIdMismatch,
            CREDENTIAL_MISMATCH => Self::CredentialMismatch,
            QUEUE_VALIDATION_FAILED => Self::QueueValidationFailed,
            IDLE_TIMEOUT => Self::IdleTimeout,
            _ => Self::Unknown(code),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BindingIdError => {
                write!(
                    f,
                    "BINDING_ID_ERROR: stream ID reused before previous flow completed"
                )
            }
            Self::AcceptorNotFound => {
                write!(f, "ACCEPTOR_NOT_FOUND: acceptor ID not found")
            }
            Self::QueueUnallocated => {
                write!(f, "QUEUE_UNALLOCATED: queue ID has no registered stream")
            }
            Self::FrameDecodeError => {
                write!(f, "FRAME_DECODE_ERROR: failed to decode control frames")
            }
            Self::AbnormalTermination => {
                write!(
                    f,
                    "ABNORMAL_TERMINATION: sender terminated abnormally (panic/crash)"
                )
            }
            Self::StopSending => {
                write!(f, "STOP_SENDING: receiver no longer wants to receive data")
            }
            Self::RetransmissionsExhausted => {
                write!(f, "RETRANSMISSIONS_EXHAUSTED: retransmissions exhausted after repeated transmission failures")
            }
            Self::ServerBusy => {
                write!(f, "SERVER_BUSY: server accept queue overflowed")
            }
            Self::QueueControlError => {
                write!(
                    f,
                    "QUEUE_CONTROL_ERROR: sender exceeded advertised flow-control credit"
                )
            }
            Self::BindingIdMismatch => {
                write!(
                    f,
                    "BINDING_ID_MISMATCH: packet binding_id does not match queue occupant"
                )
            }
            Self::CredentialMismatch => {
                write!(
                    f,
                    "CREDENTIAL_MISMATCH: packet credential_id does not match queue owner"
                )
            }
            Self::QueueValidationFailed => {
                write!(
                    f,
                    "QUEUE_VALIDATION_FAILED: queue validation failed during retry handshake"
                )
            }
            Self::IdleTimeout => {
                write!(
                    f,
                    "IDLE_TIMEOUT: peer declared dead after idle timeout elapsed"
                )
            }
            Self::Unknown(code) => {
                write!(f, "UNKNOWN({}): unknown reset error code", code.as_u64())
            }
        }
    }
}

impl std::error::Error for Error {}
