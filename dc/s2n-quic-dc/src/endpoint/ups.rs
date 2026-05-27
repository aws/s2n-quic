// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! UnknownPathSecret (UPS) response sending.
//!
//! When a recv_dispatch worker receives a packet whose credentials are not in the path
//! secret map, the pre-authentication step encodes an UnknownPathSecret control packet.
//! That packet is pushed into a shared sync queue and drained by a single background task
//! that applies per-credential dedup and token-bucket rate limiting before sending.

use crate::{
    counter::{Counter, Registry},
    credentials,
    msg::addr::Addr,
    socket::channel::{ByteCost, Sendable},
    time::precision::Timestamp,
};
use hashbrown::HashMap;
use s2n_quic_core::inet::ExplicitCongestionNotification;
use std::io::IoSlice;

/// A pre-encoded UnknownPathSecret response ready to be sent.
pub struct Response {
    pub dest_addr: Addr,
    pub packet: Vec<u8>,
}

impl ByteCost for Response {
    fn byte_cost(&self) -> u64 {
        self.packet.len() as u64
    }
}

impl Sendable for Response {
    fn send<S: crate::socket::send::Socket>(&mut self, socket: &S) -> std::io::Result<()> {
        socket.send_msg(
            &self.dest_addr,
            &[IoSlice::new(&self.packet)],
            0,
            ExplicitCongestionNotification::NotEct,
        )?;
        Ok(())
    }
}

// TODO: dedup filter needs to consider both UPS and stale key messages, not just UPS
const CREDENTIAL_ID_OFFSET: usize = 1;
const CREDENTIAL_ID_LEN: usize = 16;

/// Extracts the credential_id from an encoded UnknownPathSecret packet.
///
/// The packet layout is: [tag: 1 byte][credential_id: 16 bytes][...].
#[expect(dead_code)]
fn extract_credential_id(packet: &[u8]) -> Option<credentials::Id> {
    let end = CREDENTIAL_ID_OFFSET + CREDENTIAL_ID_LEN;
    if packet.len() < end {
        return None;
    }
    let mut id = [0u8; CREDENTIAL_ID_LEN];
    id.copy_from_slice(&packet[CREDENTIAL_ID_OFFSET..end]);
    Some(credentials::Id::from(id))
}

/// Per-credential dedup filter for the UPS drain pipeline.
///
/// Maintains a fixed-capacity map of credential_id → last-sent timestamp.
/// When the map exceeds capacity, it is cleared entirely (acceptable loss of
/// dedup accuracy for simplicity).
#[expect(dead_code)]
pub struct DedupFilter {
    seen: HashMap<credentials::Id, Timestamp>,
    capacity: usize,
    window: core::time::Duration,
    pub counters: DedupCounters,
}

pub struct DedupCounters {
    #[expect(dead_code)]
    pub suppressed: Counter,
}

impl DedupFilter {
    pub fn new(capacity: usize, window: core::time::Duration, counters: DedupCounters) -> Self {
        Self {
            seen: HashMap::with_capacity(capacity),
            capacity,
            window,
            counters,
        }
    }

    /// Returns `true` if the response should be sent, `false` if suppressed.
    pub fn check(&mut self, _response: &Response, _now: Timestamp) -> bool {
        // Dedup disabled: stale key and replay control packets must always be sent
        // to ensure the peer learns about invalidated flows promptly.
        true
    }
}

/// Counters for the UPS background send task.
pub struct Counters {
    pub sent: Counter,
    pub send_error: Counter,
    pub dedup_suppressed: Counter,
}

impl Counters {
    pub fn new(registry: &Registry) -> Self {
        Self {
            sent: registry.register("ups.sent"),
            send_error: registry.register("!ups.send_error"),
            dedup_suppressed: registry.register("ups.dedup_suppressed"),
        }
    }
}
