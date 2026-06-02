// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Shared ACK state between recv dispatch workers and send workers.
//!
//! ACK ranges are encoded by the recv worker and sent directly to the send worker over
//! an unsync channel. This avoids cross-task shared locks and naturally batches bursts:
//! one pending entry per recv context per dispatch poll.

use crate::{endpoint::id, path::secret::map::Entry as PathSecretEntry, time::precision};
use bytes::Bytes;
use s2n_quic_core::{frame::ack::EcnCounts, varint::VarInt};
use std::sync::Arc;

/// Notification sent on the direct channel from a recv dispatch worker to a send worker.
///
/// Carries the first ACK range and ECN counts inline. Additional gap/range pairs (if any)
/// are in `extra_ranges`. In the common no-loss case, `extra_ranges` is empty.
pub struct Submission {
    /// Largest acknowledged packet number (upper bound of first range).
    pub largest_acknowledged: VarInt,
    /// Smallest packet number in the first contiguous range.
    pub ack_range: VarInt,
    /// Additional gap/range VarInt pairs beyond the first range. Empty when no loss.
    pub extra_ranges: Bytes,
    /// ECN counts (always present in DC).
    pub ecn_counts: EcnCounts,
    /// Largest-acked packet receive time (recv clock domain) for ack_delay stamping.
    pub largest_recv_time: precision::Timestamp,
    /// Path secret entry identifying the peer — used by the send worker to find or
    /// create the corresponding send::Context.
    pub path_secret_entry: Arc<PathSecretEntry>,
    /// Which local sender_id this ACK should route through (determines the send socket
    /// and therefore the send::Context within the send worker's cache).
    pub local_sender_id: id::LocalSenderId,
    /// The remote peer's sender_id — written into the outbound packet header so the
    /// peer can route the ACK to its loss detection context.
    pub remote_sender_id: id::RemoteSenderId,
    /// Which recv dispatch worker submitted this entry. Used to route the completion
    /// notification back to the correct thread.
    pub recv_worker_id: id::RecvDispatchWorkerId,
}
