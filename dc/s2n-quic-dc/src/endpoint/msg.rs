// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::{
    endpoint::id::LocalSenderId, path::secret::map::Entry as PathSecretEntry,
    stream::endpoint::ack::state,
};
use bytes::BytesMut;
use core::time::Duration;
use s2n_quic_core::{frame::ack::EcnCounts, varint::VarInt};
use std::sync::Arc;

pub enum Stream {
    Data {
        offset: VarInt,
        /// Absolute largest stream offset the writer wants to send (its high watermark),
        /// reconstructed from the frame's `largest_offset`. Lets the reader right-size the
        /// receive window it advertises.
        peer_max_offset: VarInt,
        fin: bool,
        /// The writer signaled it is flow-control blocked at `peer_max_offset`.
        blocked: bool,
        payload: BytesMut,
    },
    /// Standalone writer-blocked signal (from a `QueueDataBlocked` frame). Carries the desired
    /// high-water offset; the reader uses it to grow its window. No payload.
    Blocked {
        desired_offset: VarInt,
    },
    Reset {
        error_code: VarInt,
    },
}

pub enum Control {
    Frames {
        payload: BytesMut,
    },
    /// Inline window update: the new `maximum_data` value advertised by the reader.
    ///
    /// This is the fast path dispatched from a [`Header::QueueMaxData`] frame,
    /// avoiding payload allocation and QUIC frame decoding for the common
    /// flow-control case.
    MaxData {
        maximum_data: VarInt,
    },
    Reset {
        error_code: VarInt,
    },
}

pub enum Sender {
    /// Inbound ACK from the peer — decoded fields drive loss detection and CCA updates.
    ReceivedAck {
        local_sender_id: LocalSenderId,
        path_secret_entry: Arc<PathSecretEntry>,
        /// Additional gap/range pairs beyond the first range (often empty).
        payload: BytesMut,
        /// Wire-time ACK delay: time from when the largest acknowledged packet was received
        /// by the peer to when the ACK was sent.  Extracted from `Header::Ack.ack_delay` by
        /// the dispatch layer and subtracted from the RTT sample in `process_ack`.
        ack_delay: Duration,
        largest_acknowledged: VarInt,
        ack_range: VarInt,
        ecn_counts: EcnCounts,
    },
    /// Notification carrying a freshly encoded outbound ACK body from recv worker.
    /// The send worker stamps wire-time ack_delay during assembly.
    PendingAck(state::Submission),
    /// Token indicating that freed queue IDs are ready for QueueFree emission.
    /// Submitted by OnFree::Server when a slot is reclaimed. The assembler
    /// encodes ranges JIT from FreedInner at transmission time.
    PendingFreed {
        path_secret_entry: Arc<PathSecretEntry>,
        local_sender_id: LocalSenderId,
    },
}

impl Sender {
    /// Returns the send socket index this message should route to.
    #[inline]
    pub fn sender_idx(&self) -> LocalSenderId {
        match self {
            Self::ReceivedAck {
                local_sender_id, ..
            } => *local_sender_id,
            Self::PendingAck(submission) => submission.local_sender_id,
            Self::PendingFreed {
                local_sender_id, ..
            } => *local_sender_id,
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
            Self::PendingFreed {
                path_secret_entry, ..
            } => path_secret_entry,
        }
    }

    /// Returns the recv dispatch worker ID for completion routing, if applicable.
    #[inline]
    pub fn recv_worker_id(&self) -> Option<super::id::RecvDispatchWorkerId> {
        match self {
            Self::ReceivedAck { .. } => None,
            Self::PendingAck(submission) => Some(submission.recv_worker_id),
            Self::PendingFreed { .. } => None,
        }
    }
}
