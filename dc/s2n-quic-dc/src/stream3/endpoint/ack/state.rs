// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Shared ACK state between recv dispatch workers and send workers.
//!
//! ACK ranges are encoded by the recv worker and sent directly to the send worker over
//! an unsync channel. This avoids cross-task shared locks and naturally batches bursts:
//! one pending entry per recv context per dispatch poll.

use crate::{clock::precision, path::secret::map::Entry as PathSecretEntry};
use bytes::Bytes;
use s2n_quic_core::varint::VarInt;
use std::sync::Arc;

/// Notification sent on the direct channel from a recv dispatch worker to a send worker.
///
/// Carries a fully encoded ACK body and metadata needed by the send worker to stamp
/// wire-time ack_delay.
pub struct Submission {
    /// Pre-encoded ACK frame body (ranges + optional ECN counts).
    pub body: Bytes,
    /// Largest-acked packet receive time (recv clock domain) for ack_delay stamping.
    pub largest_recv_time: precision::Timestamp,
    /// Whether `body` includes ECN counts.
    pub has_ecn: bool,
    /// Path secret entry identifying the peer — used by the send worker to find or
    /// create the corresponding send::Context.
    pub path_secret_entry: Arc<PathSecretEntry>,
    /// Which local sender_id this ACK should route through (determines the send socket
    /// and therefore the send::Context within the send worker's cache).
    pub local_sender_id: VarInt,
    /// The remote peer's sender_id — written into the outbound packet header so the
    /// peer can route the ACK to its loss detection context.
    pub remote_sender_id: VarInt,
    /// Which recv dispatch worker submitted this entry. Used to route the completion
    /// notification back to the correct thread.
    pub recv_worker_id: usize,
}
