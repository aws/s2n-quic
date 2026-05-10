// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

use crate::credentials::{self, Credentials};
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

    #[inline]
    fn sender_id(&self, credentials_id: &credentials::Id, source_sender_id: VarInt) -> VarInt {
        let hash = hash_id_and_sender(credentials_id, source_sender_id);
        unsafe { VarInt::new_unchecked(self.route(hash) as u64) }
    }

    #[inline]
    fn worker_id(&self, credentials: &Credentials, source_sender_id: VarInt) -> usize {
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

/// Routes `msg::Sender` messages to the correct per-socket ACK channel.
///
/// The `local_sender_id` embedded in `msg::Sender::Ack` identifies which send socket's
/// context should process this acknowledgement. `AckSender<T>` wraps one sender of type `T`
/// per socket and dispatches based on that ID.
///
/// `T` is generic over the sender type so callers can wrap or transform it freely (e.g., wrap
/// an `intrusive_queue::sync::Sender<Entry<msg::Sender>>` in
/// [`EntryBoxSender`][crate::socket::channel::EntryBoxSender] to get a plain `UnboundedSender<msg::Sender>`).
pub(crate) struct AckSender<T> {
    inner: Vec<T>,
}

impl<T> AckSender<T> {
    pub fn new(senders: Vec<T>) -> Self {
        Self { inner: senders }
    }
}

impl<T> crate::socket::channel::UnboundedSender<super::msg::Sender> for AckSender<T>
where
    T: crate::socket::channel::UnboundedSender<super::msg::Sender>,
{
    fn send(
        &mut self,
        msg: super::msg::Sender,
    ) -> Result<(), super::msg::Sender> {
        match &msg {
            super::msg::Sender::Ack { local_sender_id, .. } => {
                let idx = match usize::try_from(local_sender_id.as_u64()) {
                    Ok(idx) => idx,
                    // A sender ID that doesn't fit in usize cannot be a valid socket index on this
                    // platform; treat it as an unroutable ACK and drop it.
                    Err(_) => return Err(msg),
                };
                match self.inner.get_mut(idx) {
                    Some(tx) => tx.send(msg),
                    None => Err(msg),
                }
            }
        }
    }
}
