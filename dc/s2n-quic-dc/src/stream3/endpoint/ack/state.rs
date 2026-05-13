// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Shared ACK state between recv dispatch workers and send workers.
//!
//! Each recv state (keyed by credentials::Id + remote_sender_id) owns a SharedAckState
//! behind an Arc<RwLock>. The receiver writes pre-encoded ACK ranges whenever it updates,
//! and the sender reads them at assembly time to encode the freshest possible ACK with
//! ack_delay computed at the moment the packet hits the wire.
//!
//! Versioning enables the at-most-one-in-flight invariant: the sender records which version
//! it transmitted, and the completion loop compares that against the current version to
//! decide whether to re-submit.

use crate::{clock::precision, path::secret::map::Entry as PathSecretEntry};
use bytes::Bytes;
use parking_lot::RwLock;
use s2n_quic_core::varint::VarInt;
use std::sync::Arc;

/// Pre-encoded ACK body shared between a recv context and its corresponding send context.
///
/// The recv worker holds the write side and updates `body` + `current_version` whenever
/// new packets are received. The send worker holds the read side and clones the body at
/// assembly time, computing `ack_delay` from `largest_recv_time`.
struct Inner {
    /// Pre-encoded ACK ranges (and optional ECN counts). This is the payload that gets
    /// written into the packet body — it does NOT include the ack_delay field, which is
    /// computed by the sender at assembly time.
    body: Bytes,
    /// Monotonically increasing version, bumped on each receiver update.
    current_version: u64,
    /// The version that was last transmitted by the sender. Set by the sender after
    /// encoding the ACK into a packet. The completion loop compares this against
    /// current_version to detect staleness.
    tx_version: u64,
    /// When the largest acknowledged packet number was received. The sender uses this
    /// to compute ack_delay = now - largest_recv_time at assembly time.
    largest_recv_time: precision::Timestamp,
    /// Whether the body includes ECN counts appended after the ranges.
    has_ecn: bool,
}

impl Inner {
    fn new() -> Self {
        Self {
            body: Bytes::new(),
            current_version: 0,
            tx_version: 0,
            largest_recv_time: precision::Timestamp { nanos: 0 },
            has_ecn: false,
        }
    }
}

/// Handle held by the recv dispatch worker for writing ACK state updates.
#[derive(Clone)]
pub struct Writer {
    inner: Arc<RwLock<Inner>>,
}

/// Handle held by the send worker for reading ACK state at assembly time.
#[derive(Clone)]
pub struct Reader {
    inner: Arc<RwLock<Inner>>,
}

/// Snapshot of the shared ACK state, taken by the sender at assembly time.
pub struct Snapshot {
    pub body: Bytes,
    pub version: u64,
    pub largest_recv_time: precision::Timestamp,
    pub has_ecn: bool,
}

/// Create a paired writer/reader for a single recv state's ACK.
pub fn channel() -> (Writer, Reader) {
    let state = Arc::new(RwLock::new(Inner::new()));
    (
        Writer {
            inner: state.clone(),
        },
        Reader { inner: state },
    )
}

impl Writer {
    /// Create a Reader handle that shares the same underlying state.
    pub fn reader(&self) -> Reader {
        Reader {
            inner: self.inner.clone(),
        }
    }

    /// Update the shared ACK state with a new pre-encoded body.
    ///
    /// Called by the recv dispatch worker when it has budget to re-encode ACK ranges.
    /// The write lock hold is minimal: a pointer swap and a few field writes.
    pub fn update(&self, body: Bytes, largest_recv_time: precision::Timestamp, has_ecn: bool) {
        let mut state = self.inner.write();
        state.body = body;
        state.current_version += 1;
        state.largest_recv_time = largest_recv_time;
        state.has_ecn = has_ecn;
    }

    #[expect(dead_code)]
    /// Check whether the last transmitted version is stale (a newer version exists).
    pub fn is_stale(&self) -> bool {
        let state = self.inner.read();
        state.tx_version < state.current_version
    }

    /// Returns true if no ACK body has been written yet.
    #[expect(dead_code)]
    pub fn is_empty(&self) -> bool {
        let state = self.inner.read();
        state.body.is_empty()
    }
}

impl Reader {
    /// Read the current ACK state for assembly. The lock hold is three field copies
    /// (Bytes clone is an Arc ref bump).
    ///
    /// Returns `None` if no ACK has been written yet (empty body).
    pub fn snapshot(&self) -> Option<Snapshot> {
        let state = self.inner.read();
        if state.body.is_empty() {
            return None;
        }
        Some(Snapshot {
            body: state.body.clone(),
            version: state.current_version,
            largest_recv_time: state.largest_recv_time,
            has_ecn: state.has_ecn,
        })
    }

    /// Record that a particular version was transmitted. Called by the assembler after
    /// encoding an ACK into a packet.
    pub fn mark_transmitted(&self, version: u64) {
        let mut state = self.inner.write();
        if version > state.tx_version {
            state.tx_version = version;
        }
    }

    /// Check whether there's a newer version available than what was last transmitted.
    #[expect(dead_code)]
    pub fn has_pending(&self) -> bool {
        let state = self.inner.read();
        state.tx_version < state.current_version
    }
}

/// Notification sent on the direct channel from a recv dispatch worker to a send worker.
///
/// This is the lightweight handle indicating that an ACK is ready to be read from the
/// shared state. The send worker uses the reader to snapshot the ACK body at assembly
/// time, and the path_secret_entry to look up the corresponding send::Context.
pub struct Submission {
    /// Reader handle to the shared ACK state for this recv context.
    pub reader: Reader,
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
