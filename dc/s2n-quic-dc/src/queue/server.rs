// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Server-side queue dispatch.
//!
//! The server does not allocate local queue slots; the client chooses the
//! slot indices and sends them in the `dest_queue_id` field.  The server's
//! job is:
//!
//! 1. **Bind-and-send** (`bind_and_send_stream`): on the first packet for a
//!    stream, set the slot's `binding_id` and open the receiver halves
//!    atomically under the half locks (no CAS, no race between concurrent
//!    packets).  Return the new `StreamReceiver` / `ControlReceiver` for the
//!    handshake path.
//!
//! 2. **Dispatch** (`send_stream`, `send_control`): on subsequent packets,
//!    validate `binding_id` and push the entry.
//!
//! ## Slot lifecycle (server side)
//!
//! ```text
//! client creates stream    →  slot allocated, binding_id = session_binding_id
//! first server packet      →  bind_and_send_stream: bind + open + push (atomic)
//! data packets             →  send_stream / send_control
//! stream complete          →  ControlReceiver / StreamReceiver dropped
//!                          →  freed_sender.record(queue_id) → QueueFree to client
//! ```

use super::{
    freed::FreedSender,
    half::AutoWake,
    handle::{ControlReceiver, OnFree, StreamReceiver},
    page_table::{PageTable, SenderView},
    slot::BindState,
    Error,
};
use crate::{endpoint::msg, intrusive};
use s2n_quic_core::varint::VarInt;


// ── BindResult ────────────────────────────────────────────────────────────────

/// Outcome of `ServerDispatch::bind_and_send_stream`.
pub enum BindResult {
    /// The slot already had a matching binding — packet pushed.
    Bound(AutoWake),
    /// A new binding was created.  The caller must hand the receivers to the
    /// stream handshake task.
    NewBinding {
        waker: AutoWake,
        stream: StreamReceiver,
        control: ControlReceiver,
    },
}

// ── ServerDispatch ────────────────────────────────────────────────────────────

/// Dispatches inbound packets for a single peer connection.
///
/// `ServerDispatch` owns a `SenderView` that caches raw pointers into the
/// pinned page table, so repeated dispatch calls never re-acquire the
/// `RwLock` unless a page growth has occurred.
pub struct ServerDispatch {
    page_table: PageTable,
    /// Per-dispatch cached view — avoids RwLock on every packet.
    view: SenderView,
    freed: FreedSender,
    /// The max queue_id we advertised to the client.
    max_queue_id: u64,
}

impl ServerDispatch {
    pub fn new(freed: FreedSender, max_queues: VarInt) -> Self {
        let page_table = PageTable::new();
        let view = page_table.sender_view();
        Self {
            page_table,
            view,
            freed,
            max_queue_id: max_queues.as_u64().saturating_sub(1),
        }
    }

    /// Attempt to bind a slot and push the first stream entry.
    ///
    /// `queue_id` — the slot index chosen by the client.
    /// `binding_id` — the per-stream binding credential (client-chosen).
    ///
    /// The binding check and entry push happen inside the combined half locks
    /// so there is no window where two concurrent packets can both create a
    /// fresh binding for the same slot.
    ///
    /// Returns `Err(Unallocated)` if `queue_id` exceeds the cap or if the
    /// slot cannot be looked up.
    pub fn bind_and_send_stream(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Stream>,
    ) -> Result<BindResult, Error<intrusive::Entry<msg::Stream>>> {
        let index = queue_id.as_u64() as usize;

        if queue_id.as_u64() > self.max_queue_id {
            return Err(Error::CapExceeded(entry));
        }

        // Grow the page table on demand — the client controls queue_id space
        // up to the validated cap above.
        if index >= self.page_table.total_slots() {
            self.page_table.grow_to_fit(index);
        }

        let Some(slot) = self.view.get(index) else {
            return Err(Error::Unallocated(entry));
        };

        // bind_and_push_stream performs the binding check and entry push
        // atomically inside the combined half locks — no CAS needed.
        match slot.bind_and_push_stream(binding_id, entry)? {
            BindState::AlreadyBound(waker) => Ok(BindResult::Bound(waker)),
            BindState::NewBinding(waker) => {
                let slot_ptr = slot.as_ptr();
                let state = self.page_table.state.clone();
                let stream = StreamReceiver::new(
                    slot_ptr,
                    OnFree::Server(self.freed.clone(), state.clone()),
                );
                let control = ControlReceiver::new(
                    slot_ptr,
                    OnFree::Server(self.freed.clone(), state),
                );
                Ok(BindResult::NewBinding {
                    waker,
                    stream,
                    control,
                })
            }
        }
    }

    /// Push to an already-bound stream slot.
    #[inline]
    pub fn send_stream(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Stream>,
    ) -> Result<AutoWake, Error<intrusive::Entry<msg::Stream>>> {
        let index = queue_id.as_u64() as usize;
        let Some(slot) = self.view.get(index) else {
            return Err(Error::Unallocated(entry));
        };
        slot.push_stream(binding_id, entry)
    }

    /// Push to an already-bound control slot.
    #[inline]
    pub fn send_control(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Control>,
    ) -> Result<AutoWake, Error<intrusive::Entry<msg::Control>>> {
        let index = queue_id.as_u64() as usize;
        let Some(slot) = self.view.get(index) else {
            return Err(Error::Unallocated(entry));
        };
        slot.push_control(binding_id, entry)
    }

    /// Broadcast-close all slots — called when the path secret entry is evicted.
    ///
    /// `AutoWake` tokens are passed to `waker_sink` — the caller can `.take()`
    /// to batch wakers, or drop to wake immediately.
    pub fn close(&mut self, waker_sink: &mut impl FnMut(super::half::AutoWake)) {
        self.view.for_each_slot(|slot| {
            let (sw, cw) = slot.broadcast_close();
            waker_sink(sw);
            waker_sink(cw);
        });
    }
}

