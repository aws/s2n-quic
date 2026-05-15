// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::credentials::Id;
use std::net::SocketAddr;

#[cfg(test)]
mod tests;

/// The minimal information persisted per peer so that a server restart can
/// proactively notify clients that their cached path secrets are no longer
/// valid.
///
/// Secret material is intentionally NOT persisted; on replay the server
/// reconstructs the stateless reset tag via `signer().sign(credential_id)`,
/// which assumes the signing key is durable across restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistedEntry {
    pub peer: SocketAddr,
    pub credential_id: Id,
}

/// Hook invoked by the path-secret map cleaner to stream live entries for
/// persistence.
///
/// `on_entry_visited` is called for each live entry during the cleaner's
/// retain loop while the eviction queue lock is held — implementations must
/// only do cheap in-memory work there.
///
/// `on_cycle_complete` is called after the retain loop with the lock released;
/// this is where disk I/O (diff computation, journal/snapshot writes) belongs.
pub trait PersistenceObserver: Send + Sync {
    fn on_entry_visited(&self, entry: &PersistedEntry);

    fn on_cycle_complete(&self);
}

/// Outcome of `Map::replay_unknown_path_secrets`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplayResult {
    /// Number of UPS packets successfully sent.
    pub sent: u32,
    /// Number of UPS packets that failed to send.
    pub failed: u32,
    /// Number of persisted entries that were not attempted because the timeout
    /// elapsed.
    pub remaining: u32,
}
