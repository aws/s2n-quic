// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use super::msg;
use crate::{
    credentials::{self, Credentials},
    intrusive::Entry,
    socket::channel::UnboundedSender,
};
use s2n_quic_core::varint::VarInt;

#[inline]
pub fn hash_credentials_and_sender(credentials: &Credentials, source_sender_id: VarInt) -> u64 {
    hash_id_and_sender(&credentials.id, source_sender_id)
}

/// Combines a credentials ID with a sender ID to produce a hash for routing decisions.
///
/// The credentials ID already has good entropy (it's a random 128-bit value hashed down to
/// 64 bits). The sender ID is a small sequential value (socket index). We mix them using a
/// three-step process:
///
/// 1. XOR the hash with sender_id scaled by a large odd constant (phi, the golden ratio
///    as a fixed-point 64-bit integer: 2^64 / phi ≈ 0x9e3779b97f4a7c15). Multiplying by
///    phi spreads even closely-spaced sender IDs across the full 64-bit range.
///
/// 2. Rotate and XOR to break any remaining linear correlation between input bits and
///    output bits.
///
/// 3. Multiply by another large odd constant to provide final avalanche — small changes
///    in any input bit propagate to all output bits.
///
/// The result is that (credentials_id=X, sender_id=0) and (credentials_id=X, sender_id=1)
/// map to completely different hash buckets, which is what we need for distributing packets
/// from the same peer across different workers.
#[inline]
pub fn hash_id_and_sender(id: &credentials::Id, sender_id: VarInt) -> u64 {
    let mut hash = id.to_hash();

    let sender_id = sender_id.as_u64();
    hash ^= sender_id.wrapping_mul(0x9e3779b97f4a7c15);
    hash = hash.rotate_left(32) ^ sender_id;
    hash = hash.wrapping_mul(0x517cc1b727220a95);

    hash
}

pub(crate) trait SenderRoute: Clone + Copy + Send + 'static {
    fn new(count: usize) -> Self;
    fn route(&self, hash: u64) -> usize;

    /// Returns the sender_id that should be used to send ACK packets back to the peer
    #[inline]
    fn sender_id_for_ack(
        &self,
        credentials_id: &credentials::Id,
        source_sender_id: VarInt,
    ) -> VarInt {
        let hash = hash_id_and_sender(credentials_id, source_sender_id);
        unsafe { VarInt::new_unchecked(self.route(hash) as u64) }
    }

    /// Returns the local worker_id that is responsible for decoding/decrypting a packet
    #[inline]
    fn worker_id_for_recv(&self, credentials: &Credentials, source_sender_id: VarInt) -> usize {
        let hash = hash_credentials_and_sender(credentials, source_sender_id);
        self.route(hash)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PowerOfTwoRoute {
    mask: u64,
}

impl SenderRoute for PowerOfTwoRoute {
    fn new(count: usize) -> Self {
        debug_assert!(count.is_power_of_two());
        Self {
            mask: (count - 1) as u64,
        }
    }

    #[inline]
    fn route(&self, hash: u64) -> usize {
        (hash & self.mask) as usize
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ModuloRoute {
    divisor: u64,
}

impl SenderRoute for ModuloRoute {
    fn new(count: usize) -> Self {
        Self {
            divisor: count as u64,
        }
    }

    #[inline]
    fn route(&self, hash: u64) -> usize {
        (hash % self.divisor) as usize
    }
}

// ── ACK Routing ───────────────────────────────────────────────────────────

/// Routes `msg::Sender` messages to the correct per-worker ACK channel.
///
/// Indexed directly by `sender_idx` — each slot is a clone of the owning worker's
/// channel sender. Single lookup, no indirection.
#[derive(Clone)]
pub(crate) struct AckSender<T> {
    senders: Vec<T>,
}

impl<T: Clone> AckSender<T> {
    /// Build a flat sender array indexed by sender_idx.
    ///
    /// `worker_senders` has one entry per send worker. `sender_id_to_worker` maps
    /// each sender_idx to its owning worker. The result has one entry per sender_idx,
    /// each cloned from the appropriate worker's sender.
    pub fn new(worker_senders: Vec<T>, sender_id_to_worker: &[usize]) -> Self {
        let senders = sender_id_to_worker
            .iter()
            .map(|&worker_idx| worker_senders[worker_idx].clone())
            .collect();
        Self { senders }
    }
}

impl<T> UnboundedSender<Entry<msg::Sender>> for AckSender<T>
where
    T: UnboundedSender<Entry<msg::Sender>>,
{
    fn send(&mut self, msg: Entry<msg::Sender>) -> Result<(), Entry<msg::Sender>> {
        let sender_idx = msg.sender_idx();
        debug_assert!(
            sender_idx < self.senders.len(),
            "sender_idx {sender_idx} out of bounds (len {})",
            self.senders.len()
        );
        let tx = unsafe { self.senders.get_unchecked_mut(sender_idx) };
        tx.send(msg)
    }
}

// ── ACK Completion Routing ───────────────────────────────────────────────

use crate::intrusive::Queue;

/// Routes ACK completion entries back to the recv dispatch worker that submitted them.
///
/// Accepts a `Queue<msg::Sender>` (batch from the assembler), partitions entries by
/// `recv_worker_id`, and sends one queue per worker — one lock acquisition per
/// destination worker per batch.
pub(crate) struct AckCompletionSender<T> {
    senders: Vec<T>,
    /// Staging queues for partitioning, one per recv worker. Reused across sends
    /// to avoid repeated allocation.
    staging: Vec<Queue<msg::Sender>>,
}

impl<T: Clone> Clone for AckCompletionSender<T> {
    fn clone(&self) -> Self {
        Self {
            senders: self.senders.clone(),
            staging: (0..self.staging.len()).map(|_| Queue::new()).collect(),
        }
    }
}

impl<T: Clone> AckCompletionSender<T> {
    pub fn new(senders: Vec<T>) -> Self {
        let len = senders.len();
        Self {
            senders,
            staging: (0..len).map(|_| Queue::new()).collect(),
        }
    }
}

impl<T> UnboundedSender<Queue<msg::Sender>> for AckCompletionSender<T>
where
    T: UnboundedSender<Queue<msg::Sender>>,
{
    fn send(&mut self, mut queue: Queue<msg::Sender>) -> Result<(), Queue<msg::Sender>> {
        while let Some(entry) = queue.pop_front() {
            let worker_id = entry
                .recv_worker_id()
                .expect("completion entry must have a recv_worker_id");
            debug_assert!(
                worker_id < self.staging.len(),
                "recv_worker_id {worker_id} out of bounds (len {})",
                self.staging.len()
            );
            unsafe { self.staging.get_unchecked_mut(worker_id) }.push_back(entry);
        }

        for (tx, staging) in self.senders.iter_mut().zip(self.staging.iter_mut()) {
            if !staging.is_empty() {
                let batch = core::mem::take(staging);
                let _ = tx.send(batch);
            }
        }

        Ok(())
    }
}
