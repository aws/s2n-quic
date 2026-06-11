// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Server-side queue state and dispatch.
//!
//! ## Architecture
//!
//! Server queue management is split into two layers:
//!
//! - [`ServerState`] — shared state stored on the path secret Entry. Owns the
//!   pinned page table and freed-slot accumulator. Created once per peer.
//!
//! - [`ServerView`] — lightweight cached view stored on each recv::Context.
//!   Caches raw pointers into the page table for O(1) dispatch without holding
//!   the RwLock on every packet.
//!
//! ## Slot lifecycle (server side)
//!
//! ```text
//! client creates stream    →  slot allocated, binding_id = session_binding_id
//! first server packet      →  bind_and_send_stream: bind + open + push (atomic)
//! data packets             →  send_stream / send_control
//! stream complete          →  ControlReceiver / StreamReceiver dropped
//!                          →  freed_state.record(queue_id, ...) → QueueFree to client
//! ```

use super::{
    freed::{FreedBatchTx, FreedInner},
    half::AutoWake,
    handle::{ControlReceiver, OnFree, StreamReceiver},
    page_table::{PageTable, SenderView},
    slot::BindState,
    Error,
};
use crate::{endpoint::msg, intrusive};
use s2n_quic_core::varint::VarInt;
use std::sync::Arc;

use crate::path::secret::map::Entry as PathSecretEntry;

// ── BindResult ────────────────────────────────────────────────────────────────

/// Outcome of `ServerView::bind_and_send_stream`.
pub enum BindResult {
    /// The slot already had a matching binding — packet pushed.
    /// `release_bytes` is the number of newly-buffered bytes the caller must
    /// return to the recv credit pool.
    Bound { waker: AutoWake, release_bytes: u64 },
    /// A new binding was created.  The caller must hand the receivers to the
    /// stream handshake task.
    NewBinding {
        waker: AutoWake,
        stream: StreamReceiver,
        control: ControlReceiver,
        /// Newly-buffered bytes from the bind frame's payload that exceed the
        /// unbacked initial window. Caller must release these to the recv pool.
        release_bytes: u64,
    },
}

// ── ServerState (shared, on path secret Entry) ───────────────────────────────

/// Shared server-side queue state for a single peer connection.
///
/// Stored on the path secret Entry. Contains the page table (slot storage) and
/// the freed-slot accumulator. Does not hold any channel references — those are
/// passed in at call sites by the recv::Context.
#[derive(Debug)]
pub struct ServerState {
    pub(crate) pages: PageTable,
    pub(crate) freed: FreedInner,
    pub(crate) max_queue_id: u64,
    /// Per-stream unbacked initial recv window — the bytes a peer may send
    /// before any pool-backed window grants flow. Configured at endpoint
    /// construction; passed into slot bind to seed `initial_window_remaining`.
    pub(crate) initial_recv_window: u64,
}

impl ServerState {
    pub fn new(max_queues: VarInt, initial_recv_window: u64) -> Self {
        Self {
            pages: PageTable::new(),
            freed: FreedInner::new(),
            max_queue_id: max_queues.as_u64().saturating_sub(1),
            initial_recv_window,
        }
    }

    /// Create a `ServerView` for use on a dispatch worker.
    pub fn view(self: &Arc<Self>) -> ServerView {
        ServerView {
            state: self.clone(),
            view: SenderView::new(),
        }
    }

    /// Push Reset into all allocated slots.
    ///
    /// Does NOT permanently close the slots — this is a transient peer-dead
    /// notification.  After cooldown expires, new bindings proceed normally.
    pub fn broadcast_reset(&self, error_code: VarInt, waker_sink: &mut impl FnMut(AutoWake)) {
        let mut view = SenderView::new();
        view.for_each_slot(&self.pages, |slot| {
            let (sw, cw) = slot.broadcast_reset(error_code);
            waker_sink(sw);
            waker_sink(cw);
        });
    }
}

// ── ServerView (per recv::Context) ───────────────────────────────────────────

/// Per-worker cached view for dispatching inbound packets.
///
/// Holds a single `Arc<ServerState>` (keeps page table + freed state alive)
/// plus a local `SenderView` pointer cache for O(1) dispatch.
pub struct ServerView {
    state: Arc<ServerState>,
    view: SenderView,
}

impl ServerView {
    /// Record a queue_id as freed without binding it.
    ///
    /// Used when the server rejects an init (e.g. acceptor not found) and needs
    /// to signal the client to reclaim the peer-side slot via QueueFree.
    pub fn record_freed(
        &self,
        queue_id: VarInt,
        path_entry: &Arc<PathSecretEntry>,
        endpoint_tx: &mut FreedBatchTx,
    ) {
        self.state.freed.record(queue_id, path_entry, endpoint_tx);
    }

    /// Attempt to bind a slot and push the first stream entry.
    pub fn bind_and_send_stream(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Stream>,
        path_entry: &Arc<PathSecretEntry>,
        endpoint_tx: &mut FreedBatchTx,
    ) -> Result<BindResult, Error<intrusive::Entry<msg::Stream>>> {
        let index = queue_id.as_u64() as usize;

        if queue_id.as_u64() > self.state.max_queue_id {
            return Err(Error::CapExceeded(entry));
        }

        if index >= self.view.total_slots() {
            self.view.grow_to_fit(index, &self.state.pages);
        }

        let Some(slot) = self.view.get(index, &self.state.pages) else {
            return Err(Error::Unallocated(entry));
        };

        let (state, release_bytes) =
            slot.bind_and_push_stream(binding_id, entry, self.state.initial_recv_window)?;
        match state {
            BindState::AlreadyBound(waker) => Ok(BindResult::Bound {
                waker,
                release_bytes,
            }),
            BindState::NewBinding(waker) => {
                let slot_ptr = slot.as_ptr();
                let on_free = OnFree::Server {
                    path_entry: path_entry.clone(),
                    endpoint_tx: endpoint_tx.clone(),
                };
                let stream = StreamReceiver::new(slot_ptr, on_free.clone());
                let control = ControlReceiver::new(slot_ptr, on_free);
                Ok(BindResult::NewBinding {
                    waker,
                    stream,
                    control,
                    release_bytes,
                })
            }
        }
    }

    /// Bind a slot for a QueueMsg init frame without pushing data.
    ///
    /// The actual data delivery happens through `push_msg` after binding.
    /// Returns `Bound` if the slot is already allocated with a matching binding.
    pub fn bind_for_msg(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        path_entry: &Arc<PathSecretEntry>,
        endpoint_tx: &mut FreedBatchTx,
    ) -> Result<BindResult, Error<()>> {
        let index = queue_id.as_u64() as usize;

        if queue_id.as_u64() > self.state.max_queue_id {
            return Err(Error::CapExceeded(()));
        }

        if index >= self.view.total_slots() {
            self.view.grow_to_fit(index, &self.state.pages);
        }

        let Some(slot) = self.view.get(index, &self.state.pages) else {
            return Err(Error::Unallocated(()));
        };

        // Check if already bound — use the same atomic check as push_stream
        let stored = slot.binding_id_raw();
        if stored & super::slot::UNALLOCATED_BIT == 0 {
            // Slot is allocated. If binding matches, it's already bound.
            if stored == binding_id.as_u64() {
                return Ok(BindResult::Bound {
                    waker: AutoWake::default(),
                    release_bytes: 0,
                });
            }
            // Binding mismatch — stale or future
            if binding_id.as_u64() < stored {
                return Err(Error::StaleBinding(()));
            } else {
                return Err(Error::FutureBinding(()));
            }
        }

        // Slot is unallocated — bind it
        match slot.allocate_and_open(binding_id, self.state.initial_recv_window) {
            Ok(true) => {
                let slot_ptr = slot.as_ptr();
                let on_free = OnFree::Server {
                    path_entry: path_entry.clone(),
                    endpoint_tx: endpoint_tx.clone(),
                };
                let stream = StreamReceiver::new(slot_ptr, on_free.clone());
                let control = ControlReceiver::new(slot_ptr, on_free);
                Ok(BindResult::NewBinding {
                    waker: AutoWake::default(),
                    stream,
                    control,
                    release_bytes: 0,
                })
            }
            Ok(false) => Ok(BindResult::Bound {
                waker: AutoWake::default(),
                release_bytes: 0,
            }),
            Err(_) => Err(Error::SenderClosed),
        }
    }

    #[inline]
    pub fn send_stream(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Stream>,
    ) -> Result<(AutoWake, u64), Error<intrusive::Entry<msg::Stream>>> {
        let index = queue_id.as_u64() as usize;
        let Some(slot) = self.view.get(index, &self.state.pages) else {
            return Err(Error::Unallocated(entry));
        };
        slot.push_stream(binding_id, entry)
    }

    #[inline]
    pub fn send_control(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        entry: intrusive::Entry<msg::Control>,
    ) -> Result<AutoWake, Error<intrusive::Entry<msg::Control>>> {
        let index = queue_id.as_u64() as usize;
        let Some(slot) = self.view.get(index, &self.state.pages) else {
            return Err(Error::Unallocated(entry));
        };
        slot.push_control(binding_id, entry)
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub fn send_msg<E>(
        &mut self,
        queue_id: VarInt,
        binding_id: VarInt,
        msg_id: u64,
        stream_offset: u64,
        peer_max_offset: u64,
        message_size: u32,
        chunk_size: u16,
        chunk_index: u32,
        payload_len: u32,
        is_fin: bool,
        is_wakeup: bool,
        blocked: bool,
        write_fn: impl FnOnce(*mut u8, u32) -> Result<(), E>,
    ) -> Result<(AutoWake, u64), super::MsgError<E>> {
        let index = queue_id.as_u64() as usize;
        let Some(slot) = self.view.get(index, &self.state.pages) else {
            return Err(super::MsgError::Queue(Error::Unallocated(())));
        };
        slot.push_msg(
            binding_id,
            msg_id,
            stream_offset,
            peer_max_offset,
            message_size,
            chunk_size,
            chunk_index,
            payload_len,
            is_fin,
            is_wakeup,
            blocked,
            write_fn,
        )
    }

    /// Broadcast-close all slots — called when the path secret entry is evicted.
    pub fn close(&mut self, waker_sink: &mut impl FnMut(AutoWake)) {
        self.view.for_each_slot(&self.state.pages, |slot| {
            let (sw, cw) = slot.broadcast_close();
            waker_sink(sw);
            waker_sink(cw);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue::{freed::freed_batch_channel, testing::*};
    use s2n_quic_core::varint::VarInt;

    fn v(n: u64) -> VarInt {
        VarInt::new(n).unwrap()
    }

    fn test_server(
        max_queues: u64,
    ) -> (
        ServerView,
        Arc<PathSecretEntry>,
        FreedBatchTx,
        super::super::freed::FreedBatchRx,
    ) {
        use s2n_quic_core::dc;
        let (tx, rx) = freed_batch_channel();
        let mut params = dc::testing::TEST_APPLICATION_PARAMS;
        params.max_queues = VarInt::new(max_queues).unwrap();
        let path_entry: Arc<PathSecretEntry> =
            PathSecretEntry::builder("127.0.0.1:4433".parse().unwrap())
                .endpoint_type(s2n_quic_core::endpoint::Type::Server)
                .params(params)
                .build();
        let crate::path::secret::map::entry::QueueState::Server(ref state) =
            *path_entry.queue_state()
        else {
            panic!("expected Server queue state");
        };
        let view = state.view();
        (view, path_entry, tx, rx)
    }

    #[test]
    fn bind_and_send_new() {
        let (mut server, path_entry, mut tx, _rx) = test_server(10);
        let result =
            server.bind_and_send_stream(v(0), v(1), make_stream_entry(), &path_entry, &mut tx);
        assert!(matches!(result, Ok(BindResult::NewBinding { .. })));
    }

    #[test]
    fn bind_and_send_existing() {
        let (mut server, path_entry, mut tx, _rx) = test_server(10);
        let result =
            server.bind_and_send_stream(v(0), v(1), make_stream_entry(), &path_entry, &mut tx);
        let Ok(BindResult::NewBinding {
            stream, control, ..
        }) = result
        else {
            panic!("expected NewBinding");
        };

        // Second send with same binding
        let result =
            server.bind_and_send_stream(v(0), v(1), make_stream_entry(), &path_entry, &mut tx);
        assert!(matches!(result, Ok(BindResult::Bound { .. })));

        drop((stream, control));
    }

    #[test]
    fn stale_binding_rejected() {
        let (mut server, path_entry, mut tx, _rx) = test_server(10);
        let result =
            server.bind_and_send_stream(v(0), v(2), make_stream_entry(), &path_entry, &mut tx);
        let Ok(BindResult::NewBinding {
            stream, control, ..
        }) = result
        else {
            panic!("expected NewBinding");
        };

        // Stale binding (1 < current 2)
        let result =
            server.bind_and_send_stream(v(0), v(1), make_stream_entry(), &path_entry, &mut tx);
        assert!(matches!(result, Err(Error::StaleBinding(_))));

        drop((stream, control));
    }

    #[test]
    fn future_binding_rejected() {
        let (mut server, path_entry, mut tx, _rx) = test_server(10);
        let result =
            server.bind_and_send_stream(v(0), v(1), make_stream_entry(), &path_entry, &mut tx);
        let Ok(BindResult::NewBinding {
            stream, control, ..
        }) = result
        else {
            panic!("expected NewBinding");
        };

        // Future binding (5 > current 1)
        let result =
            server.bind_and_send_stream(v(0), v(5), make_stream_entry(), &path_entry, &mut tx);
        assert!(matches!(result, Err(Error::FutureBinding(_))));

        drop((stream, control));
    }

    #[test]
    fn cap_exceeded() {
        let (mut server, path_entry, mut tx, _rx) = test_server(5);
        let result =
            server.bind_and_send_stream(v(5), v(1), make_stream_entry(), &path_entry, &mut tx);
        assert!(matches!(result, Err(Error::CapExceeded(_))));
    }

    #[test]
    fn send_stream_after_bind() {
        let (mut server, path_entry, mut tx, _rx) = test_server(10);
        let result =
            server.bind_and_send_stream(v(0), v(1), make_stream_entry(), &path_entry, &mut tx);
        let Ok(BindResult::NewBinding {
            stream, control, ..
        }) = result
        else {
            panic!("expected NewBinding");
        };

        let result = server.send_stream(v(0), v(1), make_stream_entry());
        assert!(result.is_ok());

        drop((stream, control));
    }

    #[test]
    fn send_control_after_bind() {
        let (mut server, path_entry, mut tx, _rx) = test_server(10);
        let result =
            server.bind_and_send_stream(v(0), v(1), make_stream_entry(), &path_entry, &mut tx);
        let Ok(BindResult::NewBinding {
            stream, control, ..
        }) = result
        else {
            panic!("expected NewBinding");
        };

        let result = server.send_control(v(0), v(1), make_control_entry());
        assert!(result.is_ok());

        drop((stream, control));
    }

    #[test]
    fn send_to_unbound_slot() {
        let (mut server, _path_entry, _tx, _rx) = test_server(10);
        let result = server.send_stream(v(0), v(1), make_stream_entry());
        assert!(matches!(result, Err(Error::Unallocated(_))));
    }

    #[test]
    fn close_wakes_receivers() {
        let (mut server, path_entry, mut tx, _rx) = test_server(10);
        let result =
            server.bind_and_send_stream(v(0), v(1), make_stream_entry(), &path_entry, &mut tx);
        let Ok(BindResult::NewBinding {
            stream, control, ..
        }) = result
        else {
            panic!("expected NewBinding");
        };

        let mut waker_count = 0;
        server.close(&mut |_| waker_count += 1);
        // At least the two bound halves should produce wakers
        assert!(waker_count >= 2);

        drop((stream, control));
    }

    #[test]
    fn freed_batch_submitted_on_receiver_drop() {
        use crate::{
            socket::channel::Budget,
            testing::{ext::*, sim},
        };

        sim(|| {
            let (mut server, path_entry, mut tx, mut rx) = test_server(10);

            async move {
                let mut budget = Budget::new(usize::MAX);

                let result = server.bind_and_send_stream(
                    v(0),
                    v(1),
                    make_stream_entry(),
                    &path_entry,
                    &mut tx,
                );
                let Ok(BindResult::NewBinding {
                    stream, control, ..
                }) = result
                else {
                    panic!("expected NewBinding");
                };

                // Drop both receivers — should trigger freed notification
                drop(stream);
                drop(control);

                // Batch should have been submitted
                let entry = crate::socket::channel::Receiver::<
                    crate::intrusive::Entry<crate::stream::endpoint::msg::Sender>,
                >::recv(&mut rx, &mut budget)
                .await;
                assert!(entry.is_some());
            }
            .primary()
            .spawn();
        });
    }

    #[test]
    fn rebind_after_free() {
        let (mut server, path_entry, mut tx, _rx) = test_server(10);

        // First binding
        let result =
            server.bind_and_send_stream(v(0), v(1), make_stream_entry(), &path_entry, &mut tx);
        let Ok(BindResult::NewBinding {
            stream, control, ..
        }) = result
        else {
            panic!("expected NewBinding");
        };
        drop(stream);
        drop(control);

        // Re-bind with higher binding_id
        let result =
            server.bind_and_send_stream(v(0), v(2), make_stream_entry(), &path_entry, &mut tx);
        assert!(matches!(result, Ok(BindResult::NewBinding { .. })));
    }
}
