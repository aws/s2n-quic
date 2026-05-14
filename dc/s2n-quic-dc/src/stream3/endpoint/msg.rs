// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{path::secret::map::Entry as PathSecretEntry, stream3::endpoint::ack::state};
use bytes::BytesMut;
use s2n_quic_core::varint::VarInt;
use std::sync::Arc;

pub enum Stream {
    FlowValidated,
    Data {
        offset: VarInt,
        fin: bool,
        payload: BytesMut,
    },
    Reset {
        error_code: VarInt,
    },
}

pub enum Control {
    Frames { payload: BytesMut },
    Reset { error_code: VarInt },
}

pub enum Sender {
    /// Inbound ACK from the peer — decoded payload drives loss detection and CCA updates.
    ReceivedAck {
        local_sender_id: VarInt,
        path_secret_entry: Arc<PathSecretEntry>,
        payload: BytesMut,
    },
    /// Notification carrying a freshly encoded outbound ACK body from recv worker.
    /// The send worker stamps wire-time ack_delay during assembly.
    PendingAck(state::Submission),
}

impl Sender {
    /// Returns the send socket index this message should route to.
    #[inline]
    pub fn sender_idx(&self) -> usize {
        match self {
            Self::ReceivedAck {
                local_sender_id, ..
            } => local_sender_id.as_u64() as usize,
            Self::PendingAck(submission) => submission.local_sender_id.as_u64() as usize,
        }
    }

    /// Returns a reference to the path secret entry for context lookup.
    #[inline]
    pub fn path_secret_entry(&self) -> &Arc<PathSecretEntry> {
        match self {
            Self::ReceivedAck {
                path_secret_entry, ..
            } => path_secret_entry,
            Self::PendingAck(submission) => &submission.path_secret_entry,
        }
    }

    /// Returns the recv dispatch worker ID for completion routing, if applicable.
    #[inline]
    pub fn recv_worker_id(&self) -> Option<usize> {
        match self {
            Self::ReceivedAck { .. } => None,
            Self::PendingAck(submission) => Some(submission.recv_worker_id),
        }
    }
}

pub mod queue {
    use crate::flow;

    pub type Allocator = flow::queue::Allocator<super::Stream, super::Control, flow::Handle>;
    pub type Dispatcher = flow::queue::Dispatch<super::Stream, super::Control, flow::Handle>;
    pub type Control = flow::queue::Control<super::Stream, super::Control, flow::Handle>;
    pub type Stream = flow::queue::Stream<super::Stream, super::Control, flow::Handle>;
}
