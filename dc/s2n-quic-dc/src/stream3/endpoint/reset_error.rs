// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use s2n_quic_core::varint::VarInt;
use std::fmt;

/// Stream ID was reused before the previous flow completed
pub const STREAM_ID_ERROR: VarInt = VarInt::from_u32(1);

/// The acceptor ID specified in FlowInit was not found
pub const ACCEPTOR_NOT_FOUND: VarInt = VarInt::from_u32(2);

/// The queue state became stale or inconsistent during validation
pub const STALE_STATE: VarInt = VarInt::from_u32(3);

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
pub const FLOW_CONTROL_ERROR: VarInt = VarInt::from_u32(9);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetError {
    StreamIdError,
    AcceptorNotFound,
    StaleState,
    FrameDecodeError,
    AbnormalTermination,
    StopSending,
    RetransmissionsExhausted,
    ServerBusy,
    FlowControlError,
    Unknown(VarInt),
}

impl ResetError {
    pub fn as_varint(self) -> VarInt {
        match self {
            Self::StreamIdError => STREAM_ID_ERROR,
            Self::AcceptorNotFound => ACCEPTOR_NOT_FOUND,
            Self::StaleState => STALE_STATE,
            Self::FrameDecodeError => FRAME_DECODE_ERROR,
            Self::AbnormalTermination => ABNORMAL_TERMINATION,
            Self::StopSending => STOP_SENDING,
            Self::RetransmissionsExhausted => RETRANSMISSIONS_EXHAUSTED,
            Self::ServerBusy => SERVER_BUSY,
            Self::FlowControlError => FLOW_CONTROL_ERROR,
            Self::Unknown(code) => code,
        }
    }
}

impl From<VarInt> for ResetError {
    fn from(code: VarInt) -> Self {
        match code {
            STREAM_ID_ERROR => Self::StreamIdError,
            ACCEPTOR_NOT_FOUND => Self::AcceptorNotFound,
            STALE_STATE => Self::StaleState,
            FRAME_DECODE_ERROR => Self::FrameDecodeError,
            ABNORMAL_TERMINATION => Self::AbnormalTermination,
            STOP_SENDING => Self::StopSending,
            RETRANSMISSIONS_EXHAUSTED => Self::RetransmissionsExhausted,
            SERVER_BUSY => Self::ServerBusy,
            FLOW_CONTROL_ERROR => Self::FlowControlError,
            _ => Self::Unknown(code),
        }
    }
}

impl fmt::Display for ResetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StreamIdError => {
                write!(
                    f,
                    "STREAM_ID_ERROR: stream ID reused before previous flow completed"
                )
            }
            Self::AcceptorNotFound => {
                write!(f, "ACCEPTOR_NOT_FOUND: acceptor ID not found")
            }
            Self::StaleState => {
                write!(f, "STALE_STATE: queue state became stale or inconsistent")
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
            Self::FlowControlError => {
                write!(
                    f,
                    "FLOW_CONTROL_ERROR: sender exceeded advertised flow-control credit"
                )
            }
            Self::Unknown(code) => {
                write!(f, "UNKNOWN({}): unknown reset error code", code.as_u64())
            }
        }
    }
}

impl std::error::Error for ResetError {}
