// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Application-facing receiver handles.
//!
//! `StreamReceiver` and `ControlReceiver` are the read ends of a queue slot.
//! Each holds:
//! - A raw (but stable) pointer into the pinned page table.
//! - An `OnFree` discriminant that knows how to recycle the slot on drop and
//!   also keeps the underlying page table alive.

use super::{
    freed::FreedSender,
    half::{self, Closed},
    page_table::State,
    slot::Slot,
};
use crate::{endpoint::msg, intrusive};
use core::{
    ptr::NonNull,
    task::{Context, Poll},
};
use s2n_quic_core::varint::VarInt;
use std::sync::Arc;

// ── OnFree ────────────────────────────────────────────────────────────────────

/// Reclamation strategy chosen at construction time.
///
/// The `OnFree` value also acts as the lifetime guard: it holds an `Arc<State>`
/// that keeps the pinned page table alive for at least as long as the receiver.
#[derive(Clone)]
pub(crate) enum OnFree {
    /// Client: return the local slot index to the client free list.
    /// `_state` keeps the pinned page table alive for the receiver's lifetime.
    Client {
        _state: Arc<State>,
        local_free: Arc<super::client::LocalState>,
    },
    /// Server: notify the client that this queue_id is available again.
    Server(FreedSender, Arc<State>),
}

// ── StreamReceiver ────────────────────────────────────────────────────────────

pub struct StreamReceiver {
    slot: NonNull<Slot>,
    on_free: OnFree,
}

unsafe impl Send for StreamReceiver {}
unsafe impl Sync for StreamReceiver {}

impl StreamReceiver {
    pub(crate) fn new(slot: NonNull<Slot>, on_free: OnFree) -> Self {
        Self { slot, on_free }
    }

    #[inline]
    pub fn queue_id(&self) -> VarInt {
        self.slot().queue_id()
    }

    #[inline]
    fn slot(&self) -> &Slot {
        // SAFETY: pinned allocation kept alive by the OnFree lifetime guard.
        unsafe { self.slot.as_ref() }
    }

    #[inline]
    pub fn try_recv(&self) -> Result<Option<intrusive::Entry<msg::Stream>>, Closed> {
        self.slot().stream.pop()
    }

    #[inline]
    pub fn try_swap(&self) -> Result<intrusive::Queue<msg::Stream>, Closed> {
        self.slot().stream.try_swap()
    }

    #[inline]
    pub async fn recv(&self) -> Result<intrusive::Entry<msg::Stream>, Closed> {
        core::future::poll_fn(|cx| self.poll_recv(cx)).await
    }

    #[inline]
    pub fn poll_recv(
        &self,
        cx: &mut Context,
    ) -> Poll<Result<intrusive::Entry<msg::Stream>, Closed>> {
        self.slot().stream.poll_pop(cx)
    }

    #[inline]
    pub fn poll_swap(
        &self,
        cx: &mut Context,
    ) -> Poll<Result<intrusive::Queue<msg::Stream>, Closed>> {
        self.slot().stream.poll_swap(cx)
    }
}

impl Drop for StreamReceiver {
    fn drop(&mut self) {
        let slot = self.slot();
        let queue_id = slot.queue_id();
        let is_last = half::close_receiver(&slot.stream, &slot.control, true, || {
            slot.mark_unallocated();
        });
        if is_last {
            reclaim(queue_id, &self.on_free);
        }
    }
}

impl core::fmt::Debug for StreamReceiver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StreamReceiver")
            .field("queue_id", &self.queue_id())
            .finish()
    }
}

// ── ControlReceiver ───────────────────────────────────────────────────────────

pub struct ControlReceiver {
    slot: NonNull<Slot>,
    on_free: OnFree,
}

unsafe impl Send for ControlReceiver {}
unsafe impl Sync for ControlReceiver {}

impl ControlReceiver {
    pub(crate) fn new(slot: NonNull<Slot>, on_free: OnFree) -> Self {
        Self { slot, on_free }
    }

    #[inline]
    pub fn queue_id(&self) -> VarInt {
        self.slot().queue_id()
    }

    #[inline]
    fn slot(&self) -> &Slot {
        unsafe { self.slot.as_ref() }
    }

    #[inline]
    pub fn try_recv(&self) -> Result<Option<intrusive::Entry<msg::Control>>, Closed> {
        self.slot().control.pop()
    }

    #[inline]
    pub fn try_swap(&self) -> Result<intrusive::Queue<msg::Control>, Closed> {
        self.slot().control.try_swap()
    }

    #[inline]
    pub async fn recv(&self) -> Result<intrusive::Entry<msg::Control>, Closed> {
        core::future::poll_fn(|cx| self.poll_recv(cx)).await
    }

    #[inline]
    pub fn poll_recv(
        &self,
        cx: &mut Context,
    ) -> Poll<Result<intrusive::Entry<msg::Control>, Closed>> {
        self.slot().control.poll_pop(cx)
    }

    #[inline]
    pub fn poll_swap(
        &self,
        cx: &mut Context,
    ) -> Poll<Result<intrusive::Queue<msg::Control>, Closed>> {
        self.slot().control.poll_swap(cx)
    }
}

impl Drop for ControlReceiver {
    fn drop(&mut self) {
        let slot = self.slot();
        let queue_id = slot.queue_id();
        let is_last = half::close_receiver(&slot.stream, &slot.control, false, || {
            slot.mark_unallocated();
        });
        if is_last {
            reclaim(queue_id, &self.on_free);
        }
    }
}

impl core::fmt::Debug for ControlReceiver {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ControlReceiver")
            .field("queue_id", &self.queue_id())
            .finish()
    }
}

// ── Shared reclaim helper ─────────────────────────────────────────────────────

fn reclaim(queue_id: VarInt, on_free: &OnFree) {
    match on_free {
        OnFree::Client { local_free, .. } => {
            local_free.free.lock().unwrap().push_freed(queue_id.as_u64() as usize);
        }
        OnFree::Server(freed_sender, _state) => {
            freed_sender.record(queue_id);
        }
    }
}

// ── AllocResult ───────────────────────────────────────────────────────────────

/// Returned by `ClientAllocator::try_alloc`.
pub struct AllocResult {
    pub stream: StreamReceiver,
    pub control: ControlReceiver,
    /// The local slot index in our page table.
    ///
    /// Send this to the peer as our `local_queue_id` so it routes inbound
    /// packets back to us.
    pub local_queue_id: VarInt,
    /// The peer's freed queue slot index.
    ///
    /// Use this when addressing outbound packets to the peer's queue.
    pub dest_queue_id: VarInt,
    /// The binding credential for this stream.  Send to the peer so it can
    /// route packets back to this slot.
    pub binding_id: VarInt,
}
