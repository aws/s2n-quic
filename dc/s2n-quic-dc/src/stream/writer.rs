// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! stream Writer: Fragmentation and flow control
//!
//! The Writer breaks application data into MTU-sized frames and submits them to the pipeline.
//! It manages both local flow control (how much data we can have in flight) and remote flow
//! control (the peer's MAX_DATA window). The pipeline handles retransmission, ACKs, and
//! congestion control.
//!
//! ## Completion Channel Semantics
//!
//! The Writer uses a specialized completion channel (datagram_completion) that distinguishes
//! between normal and abnormal closure:
//!
//! 1. **Normal (graceful) closure**: When the Writer (receiver) is dropped normally after
//!    sending FIN, the `should_transmit` flag remains true so the pipeline continues best-effort
//!    transmission of queued frames. Completion notifications are silently dropped since the
//!    application no longer cares. This allows the application to drop the Writer immediately
//!    after calling shutdown() without blocking transmission.
//!
//! 2. **Abnormal (panic) closure**: When the Writer is dropped during a panic, both
//!    `should_transmit` and `receiver_alive` flags are cleared, and a QueueReset with
//!    ABNORMAL_TERMINATION is sent to the peer. The pipeline will cancel all pending
//!    transmissions and not attempt to send them. This ensures the peer is notified when
//!    the sender crashes.
//!
//! The Drop implementation checks `std::thread::panicking()` to distinguish between these cases.

// TODOs:
//
// Flow control:
//
// * Auto-tune max_inflight_bytes based on completion queue delivery rate. Currently using a
//   fixed budget. If completions arrive quickly, grow the budget to keep the pipe full. If
//   slow, shrink to avoid buffering data that doesn't contribute to throughput. Similar in
//   spirit to recv_budget in the existing streams.
//
// Performance:
//
// * Pace out frame transmissions at 1us interval — right now we're passing `None` for
//   transmission_time. We also need to remember the last transmission time so we don't go
//   backward if we do another burst.
//
// * MTU estimation is overly conservative. MAX_QUEUE_DATA_HEADER_OVERHEAD assumes worst-case
//   VarInt sizes for all fields (8 bytes each), but many fields have known values at frame
//   construction time (binding_id, queue_ids, offset). We should compute the actual header
//   size using the known varint-encoded lengths for fields we know, and only use worst-case
//   for fields the transport fills later (source_sender_id, packet_number). This could
//   reclaim 20-30 bytes per frame for typical streams.
//
// Observability:
//
// * No mechanism to report FIN acknowledgment to the application. After sending FIN, the
//   Writer relies on the pipeline to deliver it but has no poll_shutdown_complete or
//   similar. Currently by design (see Completion Channel Semantics), but limits the
//   application's ability to confirm graceful close.
//
// * No idle timeout detection at the stream level. If the peer disappears silently, the
//   Writer only learns about it when a completion eventually fails (PeerDead/TransmissionError).
//   The gap between the peer dying and the Writer finding out could be large.
//
// Testing:
//
// * Deterministic tests using bach for: flow control stalls and recovery, FIN delivery,
//   early data with QueueInit, completion failure handling, panic-drop behavior, and
//   multi-stream contention on shared pipeline resources.
use super::coop::{self, Coop, HasCoop};
use crate::{
    byte_vec::ByteVec,
    endpoint::{
        error::{self, Error},
        frame::{
            self, FailureReason, Frame, Header, HomogeneousBatch, Priority, SubmissionSender,
            TransmissionStatus, DEFAULT_TTL, MAX_QUEUE_DATA_HEADER_OVERHEAD,
            MAX_QUEUE_MSG_HEADER_OVERHEAD,
        },
        msg,
    },
    intrusive::{Entry, Queue},
    packet::{
        control,
        datagram::{QueuePair, ResetTarget},
    },
    path::secret::map::Entry as PathSecretEntry,
    stream::metrics::WriterMetrics,
    tracing::*,
};
use s2n_quic_core::{
    buffer::{self, writer::Storage},
    state::{event, is},
    task::waker,
    varint::VarInt,
};
use std::{
    alloc::{self, Layout},
    io,
    net::SocketAddr,
    pin::Pin,
    ptr::NonNull,
    sync::Arc,
    task::{Context, Poll},
};

/// Flags controlling per-message behavior for QueueMsg emission.
#[derive(Clone, Copy, Debug, Default)]
pub struct MsgFlags {
    /// End of stream — all frames in this message carry is_fin.
    pub is_fin: bool,
    /// Request receiver wakeup on message completion.
    /// Overridden to true when flow control budget is nearly exhausted.
    pub is_wakeup: bool,
}

/// The send half of an `s2n-quic-dc` stream.
///
/// `Writer` accepts an ordered byte stream from the application, fragments it
/// into transport frames, and enforces both local inflight limits and the
/// peer's advertised `MAX_DATA` credit.
///
/// # Expectations and guarantees
///
/// - Writes preserve byte order.
/// - Successful writes only mean data was queued to the transport pipeline, not
///   that the peer has already received or acknowledged it.
/// - When the `tokio` feature is enabled, `Writer` implements
///   [`tokio::io::AsyncWrite`].
/// - After FIN is sent, further writes are rejected. Today that surfaces as
///   `BrokenPipe`.
///
/// # Footguns
///
/// - [`write_from`](Self::write_from) and [`write_from_fin`](Self::write_from_fin)
///   may consume only part of the source buffer. Use the `write_all_*` helpers
///   if partial progress is inconvenient.
/// - [`shutdown`](Self::shutdown) queues FIN but does not wait for it to be
///   acknowledged.
/// - Dropping a writer outside of a panic performs a best-effort shutdown. In a
///   panic, the writer instead sends an abnormal reset and cancels queued work.
///
/// # Example
///
/// ```ignore
/// use s2n_quic_dc::stream::Writer;
///
/// async fn send_response(mut writer: Writer) -> std::io::Result<()> {
///     let mut body: &[u8] = b"hello from s2n-quic-dc";
///     writer.write_all_from_fin(&mut body).await?;
///     Ok(())
/// }
/// ```
pub struct Writer(WriterAllocPtr);

#[repr(C)]
struct WriterAlloc {
    /// MUST live at offset 0 — the credit pool casts `NonNull<Slot>` back to
    /// `NonNull<WriterAlloc>` via the registered `drop_fn`. Enforced at compile
    /// time by [`crate::assert_slot_at_offset_zero!`] below.
    slot: crate::credit::Slot,
    inner: Inner,
}

crate::assert_slot_at_offset_zero!(WriterAlloc);

/// Owning pointer to a `WriterAlloc`. Derefs to `Inner` so the writer body keeps
/// its `self.0.field` ergonomics, while drop is staged through the credit slot's
/// abandon/grant state machine: a parked acquire can transfer ownership of the
/// allocation to the pool, which then calls [`drop_writer_alloc`] to free it.
struct WriterAllocPtr(NonNull<WriterAlloc>);

// SAFETY: `WriterAllocPtr` owns the heap allocation exclusively. `Inner`'s fields
// are all `Send` (and not `Sync`), and `credit::Slot` is `Send`/`Sync`. The pool
// only ever reads/writes `Slot` fields under its own state machine; it never
// touches `Inner`.
unsafe impl Send for WriterAllocPtr {}

impl WriterAllocPtr {
    /// Allocate a `WriterAlloc` initialized with `inner` and an idle (rc=APP)
    /// `Slot` registered against [`drop_writer_alloc`].
    fn new(inner: Inner) -> Self {
        let layout = Layout::new::<WriterAlloc>();
        let raw = unsafe { alloc::alloc(layout) } as *mut WriterAlloc;
        let ptr = NonNull::new(raw).unwrap_or_else(|| alloc::handle_alloc_error(layout));
        unsafe {
            std::ptr::write(
                ptr.as_ptr(),
                WriterAlloc {
                    slot: crate::credit::Slot::new(drop_writer_alloc),
                    inner,
                },
            );
        }
        Self(ptr)
    }

    /// Pointer to the embedded `Slot` for handing to the credit pool.
    #[inline]
    fn slot_ptr(&self) -> NonNull<crate::credit::Slot> {
        self.0.cast()
    }
}

impl core::ops::Deref for WriterAllocPtr {
    type Target = Inner;
    #[inline]
    fn deref(&self) -> &Inner {
        unsafe { &(*self.0.as_ptr()).inner }
    }
}

impl core::ops::DerefMut for WriterAllocPtr {
    #[inline]
    fn deref_mut(&mut self) -> &mut Inner {
        unsafe { &mut (*self.0.as_ptr()).inner }
    }
}

impl Drop for WriterAllocPtr {
    fn drop(&mut self) {
        // Always go through `abandon` — its CAS is the single source of truth for who owns the
        // allocation. On a never-parked-or-already-consumed slot the CAS fails harmlessly and
        // returns `Granted(0)` (because `poll_granted` consumed the prior grant, or because no
        // grant was ever delivered).
        //
        // SAFETY: `abandon`'s relaxed contract (post step-5) permits calling it in any APP-owned
        // or LINKED state, exactly the range the slot can be in when the writer drops.
        let slot = unsafe { &(*self.0.as_ptr()).slot };
        match unsafe { slot.abandon() } {
            crate::credit::AbandonResult::Abandoned => {
                // The slot was LINKED and is now DEAD. The pool's pop walk will call
                // `drop_writer_alloc` to free the allocation; we must not touch it again.
                return;
            }
            crate::credit::AbandonResult::Granted(n) => {
                // We own the allocation. The slot may carry unconsumed credit (a grant the
                // writer never observed via `poll_granted`) and `Inner.pending_credits` may
                // hold credit acquired-but-not-yet-attached when the send batch aborted (peer
                // reset, transmission failure, frame channel closed mid-batch). Release both
                // back to the pool so the budget recovers; otherwise repeated mid-batch
                // failures would slowly bleed the pool dry.
                let inner = unsafe { &(*self.0.as_ptr()).inner };
                let to_release = n.saturating_add(inner.pending_credits);
                if to_release > 0 {
                    inner.send_credit_pool.release(to_release);
                }
            }
            crate::credit::AbandonResult::Closed => {
                // Pool was dropped concurrently. We own the allocation; do not touch the pool.
            }
        }
        // SAFETY: The slot is APP-owned and we hold the only reference. Drop `Inner` and free
        // the heap block.
        unsafe {
            std::ptr::drop_in_place(&raw mut (*self.0.as_ptr()).inner);
            alloc::dealloc(self.0.as_ptr().cast(), Layout::new::<WriterAlloc>());
        }
    }
}

/// `drop_fn` invoked by the credit pool when it pops a dead slot — i.e. the
/// writer was dropped while its slot was linked, the pool then dequeued the
/// dead entry and now owns the allocation. Drops `Inner` and frees the block.
unsafe fn drop_writer_alloc(ptr: NonNull<crate::credit::Slot>) {
    // SAFETY: `Slot` lives at offset 0 of `WriterAlloc` (see
    // `assert_slot_at_offset_zero!`), so the cast points back to the original
    // allocation. The pool guarantees this is called exactly once.
    let ptr = ptr.cast::<WriterAlloc>();
    std::ptr::drop_in_place(&raw mut (*ptr.as_ptr()).inner);
    alloc::dealloc(ptr.as_ptr().cast(), Layout::new::<WriterAlloc>());
}

struct Inner {
    /// Channel to submit frames to the wheel
    frame_tx: SubmissionSender,
    /// Receiver for completion notifications from the pipeline
    completion_rx: frame::CompletionReceiver,
    /// Control-side channel for receiving MAX_DATA frames
    control_rx: crate::queue::ControlReceiver,
    /// Next msg_id for QueueMsg frames (monotonic per stream)
    next_msg_id: u64,
    /// When non-zero, a partially-sent segment is in progress: the current
    /// msg_id has already emitted chunks [0, pending_chunk_index) and the
    /// segment must be resumed before advancing to the next msg_id.
    pending_chunk_index: u32,
    /// The declared message_size of the pending segment (needed so resumed
    /// chunks reference the same size the receiver allocated).
    pending_segment_size: usize,
    /// The declared chunk_size of the pending segment (needed so resumed
    /// chunks use the same chunk_size the receiver expects).
    pending_chunk_size: u16,
    /// The stream_offset of the pending segment (all chunks in a segment
    /// share the same stream_offset).
    pending_stream_offset: VarInt,
    /// Path secret entry providing MTU and crypto material
    path_secret_entry: Arc<PathSecretEntry>,
    /// Cached packet size (MTU minus header overhead) for QueueData fragmentation
    packet_size: u16,
    /// Cached packet size for QueueMsg fragmentation (accounts for larger header)
    msg_packet_size: u16,
    /// The peer's queue slot index for this stream
    dest_queue_id: VarInt,
    /// Acceptor ID for server routing
    acceptor_id: VarInt,
    /// Next byte offset to send
    next_offset: VarInt,
    /// Number of bytes currently in flight (not yet acknowledged)
    inflight_bytes: u64,
    /// Maximum number of bytes allowed in flight (local flow control)
    max_inflight_bytes: u64,
    /// The peer's initial receive window from handshake params. Used to cap the
    /// init segment so the message completes as soon as the server grants credits.
    initial_remote_max_data: u64,
    /// Remote flow control budget: maximum offset we can send to
    remote_max_data: VarInt,
    /// Current status of the writer
    status: Status,
    /// Reset error code if the stream was reset by the peer
    reset_error_code: Option<VarInt>,
    /// Cooperative yield budget
    coop: Coop,
    /// Clock used to stamp `enqueued_at` on application-originated frames and to
    /// record sojourn time measurements on completion.
    clock: crate::time::DefaultClock,
    /// Per-outcome sojourn time histograms shared with the application.
    metrics: Arc<WriterMetrics>,
    /// Priority tier this writer acquires credits at. Stable over the writer's
    /// lifetime; mutated only via `set_priority` (future acquires only — already
    /// parked acquires retain the priority they were parked under).
    priority: crate::credit::Priority,
    /// Endpoint-shared send credit pool. Cloned at construction so the writer's
    /// `Slot` (offset 0 of `WriterAlloc`) can stay registered with this pool.
    send_credit_pool: crate::sync::Arc<crate::credit::Pool>,
    /// Credits granted by the most recent `poll_acquire` that have not yet been
    /// attached to a frame. Carry-over after a batch consumes less than was
    /// granted is released back to the pool.
    pending_credits: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Status {
    /// No data sent yet. First write will include dest_acceptor_id.
    #[default]
    Init,
    /// QueueData-init sent, waiting for server confirmation (any control frame).
    /// Writes continue with dest_acceptor_id until confirmed.
    InitSent,
    /// Server confirmed the binding. Writes omit dest_acceptor_id.
    Open,
    /// FIN sent
    FinSent,
    /// Shutdown completed
    Shutdown,
}

impl Status {
    is!(is_init, Init);
    is!(is_init_sent, InitSent);
    is!(is_open, Open);
    is!(is_fin_sent, FinSent);
    is!(is_shutdown, Shutdown);
    is!(is_terminal, FinSent | Shutdown);
    is!(is_confirmed, Open | FinSent | Shutdown);

    event! {
        on_init_sent(Init => InitSent);
        on_confirmed(InitSent => Open);
        on_send_fin(InitSent | Open => FinSent);
        on_shutdown(Init | InitSent | Open | FinSent => Shutdown);
    }
}

impl Writer {
    pub(crate) fn new_client(
        frame_tx: SubmissionSender,
        path_secret_entry: Arc<PathSecretEntry>,
        dest_queue_id: VarInt,
        acceptor_id: VarInt,
        control_rx: crate::queue::ControlReceiver,
        clock: crate::time::DefaultClock,
        metrics: Arc<WriterMetrics>,
        send_credit_pool: crate::sync::Arc<crate::credit::Pool>,
        priority: crate::credit::Priority,
    ) -> Self {
        let completion_rx = frame::completion_channel();
        let parameters = path_secret_entry.parameters();
        let mtu = parameters.max_datagram_size();
        let packet_size = mtu.saturating_sub(MAX_QUEUE_DATA_HEADER_OVERHEAD);
        let msg_packet_size = mtu.saturating_sub(MAX_QUEUE_MSG_HEADER_OVERHEAD);
        let max_inflight_bytes = parameters.local_send_max_data.as_u64();
        let initial_remote_max_data = parameters.remote_max_data.as_u64();
        let remote_max_data = VarInt::ZERO;

        Self(WriterAllocPtr::new(Inner {
            frame_tx,
            completion_rx,
            control_rx,
            next_msg_id: 0,
            pending_chunk_index: 0,
            pending_segment_size: 0,
            pending_chunk_size: 0,
            pending_stream_offset: VarInt::ZERO,
            path_secret_entry,
            packet_size,
            msg_packet_size,
            dest_queue_id,
            acceptor_id,
            next_offset: VarInt::ZERO,
            inflight_bytes: 0,
            max_inflight_bytes,
            initial_remote_max_data,
            remote_max_data,
            status: Status::Init,
            reset_error_code: None,
            coop: Coop::default(),
            clock,
            metrics,
            priority,
            send_credit_pool,
            pending_credits: 0,
        }))
    }

    pub(crate) fn new_server(
        frame_tx: SubmissionSender,
        path_secret_entry: Arc<PathSecretEntry>,
        dest_queue_id: VarInt,
        acceptor_id: VarInt,
        control_rx: crate::queue::ControlReceiver,
        clock: crate::time::DefaultClock,
        metrics: Arc<WriterMetrics>,
        send_credit_pool: crate::sync::Arc<crate::credit::Pool>,
        priority: crate::credit::Priority,
    ) -> Self {
        let completion_rx = frame::completion_channel();
        let parameters = path_secret_entry.parameters();
        let mtu = parameters.max_datagram_size();
        let packet_size = mtu.saturating_sub(MAX_QUEUE_DATA_HEADER_OVERHEAD);
        let msg_packet_size = mtu.saturating_sub(MAX_QUEUE_MSG_HEADER_OVERHEAD);
        let max_inflight_bytes = parameters.local_send_max_data.as_u64();
        let initial_remote_max_data = parameters.remote_max_data.as_u64();

        Self(WriterAllocPtr::new(Inner {
            frame_tx,
            completion_rx,
            control_rx,
            next_msg_id: 0,
            pending_chunk_index: 0,
            pending_segment_size: 0,
            pending_chunk_size: 0,
            pending_stream_offset: VarInt::ZERO,
            path_secret_entry,
            packet_size,
            msg_packet_size,
            dest_queue_id,
            acceptor_id,
            next_offset: VarInt::ZERO,
            inflight_bytes: 0,
            max_inflight_bytes,
            initial_remote_max_data,
            remote_max_data: VarInt::new(initial_remote_max_data).unwrap_or(VarInt::MAX),
            status: Status::Open,
            reset_error_code: None,
            coop: Coop::default(),
            clock,
            metrics,
            priority,
            send_credit_pool,
            pending_credits: 0,
        }))
    }

    /// Returns the priority this writer is currently acquiring credits at.
    #[inline]
    pub fn priority(&self) -> crate::credit::Priority {
        self.0.priority
    }

    /// Set the priority used for **future** credit acquires. Already-parked
    /// acquires keep the priority they were parked under (the slot is linked
    /// in that tier's wait list and cannot be migrated without racing the
    /// distributor).
    #[inline]
    pub fn set_priority(&mut self, priority: crate::credit::Priority) {
        self.0.priority = priority;
    }

    /// Writes bytes from the source buffer into the stream.
    ///
    /// The writer may accept only part of the source buffer before returning.
    /// If the caller needs to drain the entire buffer, prefer
    /// [`write_all_from`](Self::write_all_from).
    ///
    /// # Semantics
    ///
    /// Progress can be limited by:
    ///
    /// - the peer's current `MAX_DATA` credit,
    /// - the local inflight-byte budget,
    /// - the current packet size.
    ///
    /// # Footguns
    ///
    /// A successful return does not mean the bytes were acknowledged. It only
    /// means they were handed off to the transport pipeline.
    pub async fn write_from<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        core::future::poll_fn(|cx| self.poll_write_from(cx, buf, false)).await
    }

    /// Write all data from a buffer
    ///
    /// This method loops until `buf` is empty or the stream returns an error.
    ///
    /// # Guarantee
    ///
    /// On success, every byte that was present in `buf` when the call started
    /// has been queued to the transport.
    pub async fn write_all_from<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        let mut total = 0;
        loop {
            total += self.write_from(buf).await?;
            if buf.buffer_is_empty() {
                return Ok(total);
            }
        }
    }

    /// Writes bytes from the source buffer and marks the stream finished once
    /// that buffer is empty.
    ///
    /// If this call only consumes part of `buf`, FIN is not sent yet. FIN is
    /// attached to the last chunk, which is the first successful call where the
    /// provided buffer becomes empty.
    ///
    /// If the caller wants one call that keeps going until both the payload and
    /// FIN are queued, prefer [`write_all_from_fin`](Self::write_all_from_fin).
    ///
    /// # Footguns
    ///
    /// Keep passing the same logical payload until the source buffer is empty.
    /// Starting over with a new buffer after a partial return changes the final
    /// stream contents.
    pub async fn write_from_fin<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        core::future::poll_fn(|cx| self.poll_write_from(cx, buf, true)).await
    }

    /// Write all data from a buffer and send FIN
    ///
    /// This method loops until the entire buffer has been queued and the final
    /// chunk has been marked with FIN.
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn send_bytes(
    ///     writer: &mut s2n_quic_dc::stream::Writer,
    ///     mut bytes: &[u8],
    /// ) -> std::io::Result<()> {
    ///     writer.write_all_from_fin(&mut bytes).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn write_all_from_fin<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        let mut total = 0;
        loop {
            total += self.write_from_fin(buf).await?;
            if buf.buffer_is_empty() {
                return Ok(total);
            }
        }
    }

    /// Sends a complete message using pre-allocated reassembly on the receiver.
    ///
    /// The entire contents of `buf` are treated as one atomic message. The receiver
    /// pre-allocates a contiguous buffer and only wakes the application once all
    /// chunks arrive. This eliminates per-frame allocation and per-frame wakeups
    /// for large messages.
    ///
    /// This method loops until `buf` is fully drained or the stream errors.
    pub async fn write_msg<S>(&mut self, buf: &mut S, flags: MsgFlags) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        let total = buf.buffered_len();
        core::future::poll_fn(|cx| {
            let slot = self.0.slot_ptr();
            self.0.poll_write_msg(cx, slot, buf, flags)
        })
        .await?;
        Ok(total)
    }

    /// Returns the handshake peer address used to identify this stream.
    ///
    /// This remains the stable peer identity even if data is sent across
    /// multiple data paths.
    #[inline]
    pub fn peer_addr(&self) -> SocketAddr {
        *self.0.path_secret_entry.peer()
    }

    /// Poll-based form of [`write_from`](Self::write_from) and
    /// [`write_from_fin`](Self::write_from_fin).
    ///
    /// Pass `is_fin = true` when the remaining bytes in `buf` represent the end
    /// of the stream.
    pub fn poll_write_from<S>(
        &mut self,
        cx: &mut Context,
        buf: &mut S,
        is_fin: bool,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::reader::storage::Infallible,
    {
        let slot = self.0.slot_ptr();
        self.0.poll_write_from(cx, slot, buf, is_fin)
    }

    /// Locally half-closes the write side of the stream.
    ///
    /// This is the explicit half-close operation for the write side.
    ///
    /// # Guarantees
    ///
    /// - It is idempotent.
    /// - On success, the writer will not accept more application bytes.
    ///
    /// # Footguns
    ///
    /// - Success does not guarantee a FIN frame was emitted immediately. In
    ///   particular, if shutdown happens while the writer is still waiting for
    ///   flow establishment (`InitSent`), the local shutdown succeeds but
    ///   no FIN can be sent yet because the peer queue ID is still unknown.
    /// - Even when a FIN frame is emitted, success only means it was queued
    ///   locally. It does not mean the peer has observed it yet.
    pub fn shutdown(&mut self) -> io::Result<()> {
        self.0.shutdown()
    }

    pub(crate) fn force_shutdown(&mut self) {
        self.0.completion_rx.cancel();
        self.0.status.on_shutdown().ok();
    }
}

impl Inner {
    #[inline]
    fn queue_pair(&self) -> QueuePair {
        QueuePair {
            source_queue_id: self.control_rx.queue_id(),
            dest_queue_id: self.dest_queue_id,
        }
    }

    #[inline]
    fn dest_acceptor_id(&self) -> Option<VarInt> {
        if self.status.is_confirmed() {
            None
        } else {
            Some(self.acceptor_id)
        }
    }

    /// Priority field to encode on the next outgoing frame. Init frames (those carrying
    /// `dest_acceptor_id`) emit the writer's actual priority; non-init frames emit the default
    /// because the wire format only encodes priority when `dest_acceptor_id.is_some()`. Mirrors
    /// `Header::canonicalize_for_wire` so production code never produces a non-canonical header.
    #[inline]
    fn wire_priority(&self) -> crate::credit::Priority {
        if self.dest_acceptor_id().is_some() {
            self.priority
        } else {
            crate::credit::Priority::default()
        }
    }
}

impl HasCoop for Inner {
    #[inline]
    fn coop(&mut self) -> &mut Coop {
        &mut self.coop
    }
}

impl Inner {
    #[inline]
    fn poll_write_from<S>(
        &mut self,
        cx: &mut Context,
        slot: NonNull<crate::credit::Slot>,
        buf: &mut S,
        is_fin: bool,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::reader::storage::Infallible,
    {
        waker::debug_assert_contract(cx, |cx| {
            coop::poll(self, cx, |this, cx| {
                this.poll_write_from_inner(cx, slot, buf, is_fin)
            })
        })
    }

    fn poll_write_msg<S>(
        &mut self,
        cx: &mut Context,
        slot: NonNull<crate::credit::Slot>,
        buf: &mut S,
        flags: MsgFlags,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::reader::storage::Infallible,
    {
        waker::debug_assert_contract(cx, |cx| {
            coop::poll(self, cx, |this, cx| {
                this.poll_write_msg_inner(cx, slot, buf, flags)
            })
        })
    }

    fn poll_write_msg_inner<S>(
        &mut self,
        cx: &mut Context,
        slot: NonNull<crate::credit::Slot>,
        buf: &mut S,
        flags: MsgFlags,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::reader::storage::Infallible,
    {
        if self.status.is_shutdown() {
            if let Some(error_code) = self.reset_error_code {
                let reset_error: Error = error_code.into();
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    reset_error,
                )));
            }
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }

        if self.status.is_fin_sent() {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }

        self.poll_completions(cx)?;
        let _ = self.poll_remote_budget(cx)?;

        // Acquire credits for the planned send batch. Init state bypasses the flow-control
        // window (init frame always goes out); steady-state uses the full window clamped by
        // the buffer. The pool further clamps by `max_single_acquire`, so we may get less than
        // `want` and just send whatever we got.
        let want = if self.status.is_init() {
            (buf.buffered_len() as u64).min(self.packet_size as u64)
        } else {
            self.flow_budget().min(buf.buffered_len() as u64)
        };
        match unsafe { self.poll_acquire_credits(cx, slot, want) } {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
        }

        // Handle Init state: first write triggers QueueMsg-init with dest_acceptor_id.
        // send_msg already includes dest_acceptor_id when status is not confirmed.
        // Force the first segment unconditionally since this bootstrap message
        // triggers the peer to send MAX_DATA.
        if self.status.is_init() {
            if buf.buffer_is_empty() {
                if flags.is_fin {
                    let (written, _) = self.send_queue_data_init(buf, true)?;
                    return Poll::Ready(Ok(written));
                }

                return Poll::Ready(Ok(0));
            }
            let written = self.send_msg(buf, flags, true)?;
            if written > 0 {
                self.status.on_init_sent().ok();
            }
            if buf.buffer_is_empty() {
                return Poll::Ready(Ok(0));
            }
            return Poll::Pending;
        }

        if self.status.is_init_sent() {
            return Poll::Pending;
        }

        if buf.buffer_is_empty() {
            if flags.is_fin {
                self.send_data(buf, true)?;
                return Poll::Ready(Ok(0));
            }
            return Poll::Ready(Ok(0));
        }

        loop {
            let written = self.send_msg(buf, flags, false)?;

            if buf.buffer_is_empty() {
                return Poll::Ready(Ok(0));
            }

            if written == 0 {
                return Poll::Pending;
            }

            if !self.coop.consume() {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }

            // Refresh budget: drain completions, process MAX_DATA, and top up credits before
            // retrying. If the credit acquire parks, exit; the next poll picks up where we left.
            self.poll_completions(cx)?;
            let _ = self.poll_remote_budget(cx)?;
            let want = self.flow_budget().min(buf.buffered_len() as u64);
            match unsafe { self.poll_acquire_credits(cx, slot, want) } {
                Poll::Ready(Ok(())) => {}
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }

    #[inline(always)]
    fn poll_write_from_inner<S>(
        &mut self,
        cx: &mut Context,
        slot: NonNull<crate::credit::Slot>,
        buf: &mut S,
        is_fin: bool,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::reader::storage::Infallible,
    {
        if self.pending_chunk_index > 0 {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot use write_from while a partial write_msg segment is pending; \
                 call write_msg again to complete the in-progress message",
            )));
        }

        if self.status.is_shutdown() {
            if let Some(error_code) = self.reset_error_code {
                let reset_error: Error = error_code.into();
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    reset_error,
                )));
            }
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }

        if self.status.is_fin_sent() {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }

        self.poll_completions(cx)?;
        let _ = self.poll_remote_budget(cx)?;

        // Acquire credits for the planned send batch before producing frames. The init frame
        // (`Init` state) bypasses the flow-control window — it must always go out — so its
        // credit ask is just `min(buf_len, packet_size)`. Steady-state asks for the full
        // flow-control window clamped by the buffer. Control-only paths (FIN-on-empty) acquire
        // zero, which `poll_acquire_credits` short-circuits.
        let want = if self.status.is_init() {
            (buf.buffered_len() as u64).min(self.packet_size as u64)
        } else {
            self.flow_budget().min(buf.buffered_len() as u64)
        };
        match unsafe { self.poll_acquire_credits(cx, slot, want) } {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
        }

        if self.status.is_init() {
            let (written, is_fin) = self.send_queue_data_init(buf, is_fin)?;

            if written > 0 || is_fin {
                return Poll::Ready(Ok(written));
            }

            return Poll::Pending;
        }

        if self.status.is_init_sent() {
            if is_fin && buf.buffer_is_empty() {
                // Buffer is empty and the caller wants to close the write side.
                // Send QueueInitFin so the server can deliver EOF before MAX_DATA arrives.
                self.send_fin_packet()?;
                return Poll::Ready(Ok(0));
            }
            trace!(
                binding_id = self.control_rx.binding_id().as_u64(),
                "Writer blocked in InitSent - waiting for remote MAX_DATA"
            );
            return Poll::Pending;
        }

        let available = self.min_send_budget();
        if available == 0 && (!is_fin || !buf.buffer_is_empty()) {
            return Poll::Pending;
        }

        let written = self.send_data(buf, is_fin)?;

        Poll::Ready(Ok(written))
    }

    /// Checks the control queue for a pending reset that was never polled.
    ///
    /// If the peer was declared dead (idle timeout), the queue contains a Reset
    /// we never consumed. Transitioning to shutdown here prevents the drop path
    /// from sending a FIN or QueueReset to a dead peer.
    fn drain_pending_reset(&mut self) {
        if self.status.is_shutdown() {
            return;
        }
        let Ok(queue) = self.control_rx.try_swap() else {
            return;
        };
        for entry in queue {
            if matches!(&*entry, msg::Control::Reset { .. }) {
                self.status.on_shutdown().ok();
                return;
            }
        }
    }

    fn shutdown(&mut self) -> io::Result<()> {
        if self.status.is_shutdown() {
            return Ok(());
        }

        if self.status.is_fin_sent() {
            self.status.on_shutdown().unwrap();
            return Ok(());
        }

        if self.pending_chunk_index > 0 {
            // A partial QueueMsg segment was sent — the receiver's MsgTable has
            // an incomplete entry that can never be filled. A FIN would bypass
            // the MsgTable (via QueueData), leaving a permanent gap in the
            // reassembler. Send a Reset so the receiver can poison the table and
            // surface an error to the application.
            let error_code = error::SENDER_CANCELLED;
            let _ = self.send_reset_frame(error_code, ResetTarget::Stream);
            self.status.on_shutdown().ok();
            return Ok(());
        }

        if self.status.is_init() {
            let mut empty = bytes::Bytes::new();
            self.send_queue_data_init(&mut empty, true)?;
        } else {
            self.send_fin_packet()?;
        }
        self.status.on_shutdown().unwrap();

        Ok(())
    }

    fn send_reset_frame(
        &mut self,
        error_code: VarInt,
        reset_target: ResetTarget,
    ) -> io::Result<()> {
        let frame = Frame {
            header: Header::QueueReset {
                dest_queue_id: self.dest_queue_id,
                binding_id: self.control_rx.binding_id(),
                reset_target,
                error_code,
                dest_acceptor_id: self.dest_acceptor_id(),
            },
            payload: ByteVec::new(),
            path_secret_entry: self.path_secret_entry.clone(),
            completion: None,
            status: frame::TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            enqueued_at: None,
            flow_credits: 0,
        };

        self.send_frame(frame)?;

        debug!(
            binding_id = self.control_rx.binding_id().as_u64(),
            error_code = error_code.as_u64(),
            ?reset_target,
            "Sent QueueReset"
        );

        Ok(())
    }

    fn send_fin_packet(&mut self) -> io::Result<()> {
        let frame = Frame {
            header: Header::QueueData {
                queue_pair: self.queue_pair(),
                binding_id: self.control_rx.binding_id(),
                offset: self.next_offset,
                is_fin: true,
                dest_acceptor_id: self.dest_acceptor_id(),
                priority: self.wire_priority(),
            },
            payload: ByteVec::new(),
            path_secret_entry: self.path_secret_entry.clone(),
            completion: Some(self.completion_rx.sender()),
            status: frame::TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            enqueued_at: Some(self.clock.now()),
            flow_credits: 0,
        };

        self.send_frame(frame)?;

        debug!(
            binding_id = self.control_rx.binding_id().as_u64(),
            "Sent FIN"
        );
        self.status.on_send_fin().unwrap();

        Ok(())
    }

    fn poll_completions(&mut self, cx: &mut Context) -> io::Result<()> {
        match self.completion_rx.poll_swap(cx) {
            Poll::Ready(Some(queue)) => {
                let mut freed_bytes = 0u64;
                let mut failure = None;

                // Snapshot current time once for all sojourn measurements in
                // this completion batch (avoids repeated clock reads).
                let completed_at = self.clock.now();

                for completed in queue.iter() {
                    // Record sojourn time for frames that carry an enqueue stamp.
                    if let Some(enqueued_at) = completed.enqueued_at {
                        let failure_reason = match completed.status {
                            TransmissionStatus::Failed(r) => Some(r),
                            _ => None,
                        };
                        self.metrics
                            .sojourn
                            .record(enqueued_at, completed_at, failure_reason);
                    }

                    match completed.status {
                        TransmissionStatus::Acknowledged => {
                            freed_bytes += completed.payload.len() as u64;
                        }
                        TransmissionStatus::Failed(reason) => {
                            failure.get_or_insert(reason);
                            freed_bytes += completed.payload.len() as u64;

                            debug!(
                                binding_id = self.control_rx.binding_id().as_u64(),
                                ?reason,
                                "Transmission failed"
                            );
                        }
                        TransmissionStatus::Pending => {
                            warn!(
                                binding_id = self.control_rx.binding_id().as_u64(),
                                "Received completion with Pending status"
                            );
                        }
                    }
                }

                self.inflight_bytes = self.inflight_bytes.saturating_sub(freed_bytes);

                trace!(
                    binding_id = self.control_rx.binding_id().as_u64(),
                    freed_bytes,
                    inflight_bytes = self.inflight_bytes,
                    "Completions received"
                );

                if let Some(reason) = failure {
                    return match reason {
                        FailureReason::UnknownPathSecret => {
                            self.status.on_shutdown().ok();
                            Err(io::Error::new(
                                io::ErrorKind::ConnectionRefused,
                                "path secret rejected by peer",
                            ))
                        }
                        FailureReason::PeerDead => {
                            self.status.on_shutdown().ok();
                            Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                "peer declared dead (idle timeout)",
                            ))
                        }
                        FailureReason::TransmissionError => {
                            let error_code = error::RETRANSMISSIONS_EXHAUSTED;
                            let _ = self.send_reset_frame(error_code, ResetTarget::Both);
                            self.status.on_shutdown().ok();
                            Err(io::Error::new(
                                io::ErrorKind::BrokenPipe,
                                "transmission failed after retries",
                            ))
                        }
                        FailureReason::Cancelled => {
                            self.status.on_shutdown().ok();
                            Err(io::Error::new(
                                io::ErrorKind::Interrupted,
                                "transmission cancelled",
                            ))
                        }
                    };
                }

                Ok(())
            }
            Poll::Ready(None) => Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "completion channel closed",
            )),
            Poll::Pending => Ok(()),
        }
    }

    fn poll_remote_budget(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        match self.control_rx.poll_swap(cx) {
            Poll::Ready(Ok(queue)) => {
                debug!(
                    binding_id = self.control_rx.binding_id().as_u64(),
                    status = ?self.status,
                    msg_count = queue.len(),
                    "poll_remote_budget received messages"
                );
                for msg in queue {
                    match msg.into_inner() {
                        msg::Control::Frames { mut payload } => {
                            if self.handle_control_frames(&mut payload).is_err() {
                                let error_code = error::FRAME_DECODE_ERROR;
                                self.reset_error_code = Some(error_code);
                                self.status.on_shutdown().ok();

                                let _ = self.send_reset_frame(error_code, ResetTarget::Both);

                                let reset_error: Error = error_code.into();
                                return Poll::Ready(Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    reset_error,
                                )));
                            }

                            self.try_establish_flow();
                        }
                        msg::Control::MaxData { maximum_data } => {
                            self.apply_max_data(maximum_data);
                            self.try_establish_flow();
                        }
                        msg::Control::Reset { error_code } => {
                            self.reset_error_code = Some(error_code);
                            self.status.on_shutdown().ok();
                            let reset_error: Error = error_code.into();
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::ConnectionReset,
                                reset_error,
                            )));
                        }
                    }
                }

                Poll::Ready(Ok(()))
            }
            Poll::Ready(Err(_)) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "control channel closed",
            ))),
            Poll::Pending => {
                trace!(
                    binding_id = self.control_rx.binding_id().as_u64(),
                    status = ?self.status,
                    "poll_remote_budget pending - no control messages"
                );
                Poll::Pending
            }
        }
    }

    /// Applies a received MAX_DATA value, keeping the highest observed window.
    fn apply_max_data(&mut self, maximum_data: VarInt) {
        let prev_max = self.remote_max_data;
        self.remote_max_data = self.remote_max_data.max(maximum_data);
        trace!(
            binding_id = self.control_rx.binding_id().as_u64(),
            prev_max = prev_max.as_u64(),
            new_max = self.remote_max_data.as_u64(),
            "Received MAX_DATA"
        );
    }

    /// Transitions the writer to `Open` if it is currently `InitSent`.
    fn try_establish_flow(&mut self) {
        if self.status.on_confirmed().is_ok() {
            debug!(
                binding_id = self.control_rx.binding_id().as_u64(),
                "Flow established"
            );
        }
    }

    fn handle_control_frames(&mut self, payload: &mut [u8]) -> Result<(), s2n_codec::DecoderError> {
        use s2n_quic_core::frame::{FrameMut, MaxData};

        let frames_iter = control::decoder::ControlFramesMut::new(payload);

        for frame in frames_iter {
            match frame? {
                FrameMut::MaxData(MaxData { maximum_data }) => {
                    self.apply_max_data(maximum_data);
                }
                frame => {
                    trace!(
                        binding_id = self.control_rx.binding_id().as_u64(),
                        frame = ?frame,
                        "Ignoring control frame"
                    );
                }
            }
        }

        Ok(())
    }

    fn send_queue_data_init<S>(&mut self, buf: &mut S, is_fin: bool) -> io::Result<(usize, bool)>
    where
        S: buffer::reader::storage::Infallible,
    {
        let (payload, bytes_read, actual_fin) = self.prepare_early_data(buf, is_fin)?;

        let flow_credits = self.take_credits(payload.len());
        let frame = Frame {
            header: Header::QueueData {
                queue_pair: self.queue_pair(),
                binding_id: self.control_rx.binding_id(),
                offset: VarInt::ZERO,
                is_fin: actual_fin,
                dest_acceptor_id: Some(self.acceptor_id),
                priority: self.wire_priority(),
            },
            payload,
            path_secret_entry: self.path_secret_entry.clone(),
            completion: Some(self.completion_rx.sender()),
            status: frame::TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            enqueued_at: Some(self.clock.now()),
            flow_credits,
        };

        self.send_frame(frame)?;

        self.status.on_init_sent().unwrap();

        if actual_fin {
            self.status.on_send_fin().unwrap();
        }

        debug!(
            binding_id = self.control_rx.binding_id().as_u64(),
            bytes_read,
            is_fin = actual_fin,
            "Sent QueueInit with early data"
        );

        Ok((bytes_read, actual_fin))
    }

    fn prepare_early_data<S>(
        &mut self,
        buf: &mut S,
        is_fin: bool,
    ) -> io::Result<(ByteVec, usize, bool)>
    where
        S: buffer::reader::storage::Infallible,
    {
        if is_fin && buf.buffer_is_empty() {
            return Ok((ByteVec::new(), 0, true));
        }

        if buf.buffer_is_empty() {
            return Ok((ByteVec::new(), 0, false));
        }

        if self.remaining_offset_capacity() == 0 {
            return Err(offset_overflow_error());
        }

        let mtu = self.packet_size as usize;
        let chunk_len = mtu
            .min(buf.buffered_len())
            .min(self.remaining_offset_capacity());

        let mut payload = ByteVec::new();
        {
            let mut writer = payload.with_write_limit(chunk_len);
            buf.infallible_copy_into(&mut writer);
        }

        let bytes_read = payload.len();

        self.advance_offset(bytes_read)?;

        let actual_is_fin = is_fin && buf.buffer_is_empty();

        Ok((payload, bytes_read, actual_is_fin))
    }

    /// Local + remote send budget, ignoring credit gating.
    ///
    /// Used to compute how much credit we *want* to acquire from the pool. The credit clamp is
    /// applied separately by `min_send_budget`.
    fn flow_budget(&self) -> u64 {
        let local_available = self.max_inflight_bytes.saturating_sub(self.inflight_bytes);
        let remote_available = self
            .remote_max_data
            .as_u64()
            .saturating_sub(self.next_offset.as_u64());

        local_available.min(remote_available)
    }

    /// Effective send budget for the current poll: the flow-control window further clamped by the
    /// credits already held (`pending_credits`). The send loops use this so they never produce a
    /// frame they can't attribute to a credit.
    fn min_send_budget(&self) -> u64 {
        self.flow_budget().min(self.pending_credits)
    }

    /// Drain any delivered grant into `pending_credits`, then top up by `want` bytes from the pool
    /// if we don't already have enough. Returns `Pending` if either the slot is still parked from a
    /// prior poll or a fresh acquire just parked.
    ///
    /// `want` should be `flow_budget()` clamped by the buffered length the caller intends to send;
    /// the pool further clamps by `max_single_acquire`. We always send whatever the pool gives us.
    ///
    /// # Safety
    ///
    /// `slot` must point to the `Slot` embedded in this writer's `WriterAlloc` (offset 0). It is
    /// the only legal slot for this `Inner` because `Pool::poll_acquire` writes a waker for *this*
    /// task into it.
    unsafe fn poll_acquire_credits(
        &mut self,
        cx: &mut Context,
        slot: NonNull<crate::credit::Slot>,
        want: u64,
    ) -> Poll<io::Result<()>> {
        // Drain any grant the distributor delivered while we were away.
        let slot_ref = unsafe { slot.as_ref() };
        match slot_ref.poll_granted() {
            crate::credit::GrantResult::Pending => {
                // Still parked from a previous poll. Don't issue a fresh acquire — that would
                // race the distributor for the same slot.
                return Poll::Pending;
            }
            crate::credit::GrantResult::Closed => {
                // Pool went away. Surface as broken pipe; future polls will keep returning the
                // same Closed (the sentinel persists), so this is idempotent.
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "credit pool closed",
                )));
            }
            crate::credit::GrantResult::Granted(n) => {
                self.pending_credits = self.pending_credits.saturating_add(n);
            }
        }

        if self.pending_credits >= want || want == 0 {
            return Poll::Ready(Ok(()));
        }

        let need = want - self.pending_credits;
        // SAFETY: caller's invariants: `slot` is this writer's idle slot.
        match unsafe {
            self.send_credit_pool
                .poll_acquire(cx, slot, need, self.priority)
        } {
            Poll::Ready(n) => {
                self.pending_credits = self.pending_credits.saturating_add(n);
                Poll::Ready(Ok(()))
            }
            Poll::Pending => Poll::Pending,
        }
    }

    /// Subtract `n` from `pending_credits`. Returns `n` so it can be assigned to
    /// `Frame.flow_credits` in one expression.
    #[inline]
    fn take_credits(&mut self, n: usize) -> u64 {
        let n = n as u64;
        debug_assert!(
            self.pending_credits >= n,
            "take_credits({n}) exceeds pending_credits {}",
            self.pending_credits
        );
        self.pending_credits = self.pending_credits.saturating_sub(n);
        n
    }

    fn send_data<S>(&mut self, buf: &mut S, is_fin: bool) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        let mtu = self.packet_size as usize;
        let mut written = 0;

        let mut need_fin_packet = is_fin && buf.buffer_is_empty();
        let mut frames = Queue::new();

        // Capture enqueue time once for all frames in this send batch so that
        // every frame shares the same reference point for sojourn measurement.
        let batch_enqueued_at = Some(self.clock.now());

        loop {
            if !need_fin_packet && buf.buffer_is_empty() {
                break;
            }

            let remaining_offset_capacity = self.remaining_offset_capacity();
            if !need_fin_packet && remaining_offset_capacity == 0 {
                if written == 0 {
                    return Err(offset_overflow_error());
                }
                break;
            }

            let available = self.min_send_budget();
            if !need_fin_packet && available == 0 {
                break;
            }

            let chunk_len = if need_fin_packet {
                0
            } else {
                mtu.min(buf.buffered_len())
                    .min(available as usize)
                    .min(remaining_offset_capacity)
            };

            let mut payload = ByteVec::new();
            if chunk_len > 0 {
                let mut writer = payload.with_write_limit(chunk_len);
                buf.infallible_copy_into(&mut writer);
            }

            let payload_len = payload.len();
            let offset = self.next_offset;
            let is_last_chunk = buf.buffer_is_empty();
            let include_fin = is_fin && is_last_chunk;

            let flow_credits = self.take_credits(payload_len);
            let frame = Frame {
                header: Header::QueueData {
                    queue_pair: self.queue_pair(),
                    binding_id: self.control_rx.binding_id(),
                    offset,
                    is_fin: include_fin,
                    dest_acceptor_id: self.dest_acceptor_id(),
                    priority: self.wire_priority(),
                },
                payload,
                path_secret_entry: self.path_secret_entry.clone(),
                completion: Some(self.completion_rx.sender()),
                status: frame::TransmissionStatus::default(),
                ttl: DEFAULT_TTL,
                enqueued_at: batch_enqueued_at,
                flow_credits,
            };

            frames.push_back(frame.into());

            self.advance_offset(payload_len)?;
            written += payload_len;

            trace!(
                binding_id = self.control_rx.binding_id().as_u64(),
                offset = offset.as_u64(),
                payload_len,
                is_fin = include_fin,
                "Sending QueueData"
            );

            if include_fin {
                self.status.on_send_fin().ok();
            }

            need_fin_packet = false;
        }

        self.send_batch(frames)?;

        Ok(written)
    }

    /// Send a complete message using QueueMsg frames (pre-allocated reassembly on receiver).
    ///
    /// The entire `buf` is treated as one message. It's split into QueueMsg frames
    /// each carrying a chunk_index. The receiver pre-allocates and reassembles
    /// without per-frame allocation.
    ///
    /// `flags` control per-message signaling: `is_fin` marks stream end,
    /// `is_wakeup` requests receiver to wake the application on completion.
    /// Both flags are set consistently on ALL frames of the message.
    ///
    /// Messages larger than MAX_CHUNKS * chunk_size are automatically split into
    /// multiple msg_ids, each independently reassembled by the receiver. Segment
    /// sizing is always based on `max_segment_size`; remote MAX_DATA gates when a
    /// new segment can start, and the method returns the number of bytes sent.
    /// `force_first` bypasses the budget check for the first segment (used by Init
    /// to bootstrap the connection before MAX_DATA is available).
    fn send_msg<S>(&mut self, buf: &mut S, flags: MsgFlags, force_first: bool) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        let total_size = buf.buffered_len();
        if total_size == 0 {
            if !flags.is_fin {
                return Ok(0);
            }
            return self.send_data(buf, true);
        }

        // Size-based routing: messages that fit in a single chunk use QueueData
        // (avoids the MsgTable/bitset overhead for small messages).
        // Skip this optimization when a partial QueueMsg segment is pending —
        // the receiver's MsgTable already has an entry expecting all chunks via
        // QueueMsg, and routing the remainder via QueueData would leave the
        // entry permanently incomplete.
        if total_size <= self.packet_size as usize && self.pending_chunk_index == 0 {
            if self.status.is_init() {
                let (written, _) = self.send_queue_data_init(buf, flags.is_fin)?;
                return Ok(written);
            }
            return self.send_data(buf, flags.is_fin);
        }

        let mtu = self.msg_packet_size as usize;
        // When resuming a pending segment, use the saved chunk_size so all
        // frames in the segment share the same declared chunk_size.
        let chunk_size = if self.pending_chunk_index > 0 {
            self.pending_chunk_size
        } else {
            mtu.min(total_size).max(1) as u16
        };
        let max_segment_size = crate::queue::msg_entry::MAX_CHUNKS as usize * chunk_size as usize;

        let batch_enqueued_at = Some(self.clock.now());
        let mut frames = Queue::new();
        let mut total_written = 0usize;

        let mut is_first = true;
        while !buf.buffer_is_empty() {
            // Gate on remote window only — the receiver enforces this limit and
            // sends MAX_DATA as it consumes. Local inflight budget is not checked
            // here because write_msg data is already materialized in the caller's
            // buffer; the send pipeline naturally throttles at wire speed.
            let remote_budget = self
                .remote_max_data
                .as_u64()
                .saturating_sub(self.next_offset.as_u64());

            // If resuming a partial segment, use the originally declared size.
            // For init (force_first), cap to initial_remote_max_data so the message
            // completes as soon as the server grants its first credits.
            // For normal path, use full max_segment_size — budget gates below.
            let segment_size = if self.pending_chunk_index > 0 {
                self.pending_segment_size
            } else if force_first {
                buf.buffered_len()
                    .min(max_segment_size)
                    .min(self.initial_remote_max_data as usize)
            } else {
                buf.buffered_len()
                    .min(max_segment_size)
                    .min((remote_budget as usize).max(chunk_size as usize))
            };

            let is_resuming = self.pending_chunk_index > 0;
            if !is_resuming && !(is_first && force_first) && (remote_budget as usize) < segment_size
            {
                break;
            }
            // Stop if we don't have enough credits for at least one chunk of this segment. The
            // outer poll loop will refresh `pending_credits` and re-enter; resume handles the
            // partial-segment case via `pending_chunk_index`.
            if !is_resuming && (self.pending_credits as usize) < chunk_size as usize {
                break;
            }
            is_first = false;

            let is_last_segment = if self.pending_chunk_index > 0 {
                // During resume, check against the remaining buffered data plus
                // what was already sent for this segment.
                let already_sent = self.pending_chunk_index as usize * chunk_size as usize;
                buf.buffered_len() + already_sent <= segment_size
            } else {
                buf.buffered_len() <= segment_size
            };

            // Preserve caller intent, but force wakeups as remote flow control
            // becomes tight so the receiver drains MsgTable entries and sends
            // MAX_DATA to unblock subsequent segments.
            let is_wakeup = flags.is_wakeup || remote_budget <= max_segment_size as u64;

            let stream_offset = if self.pending_chunk_index > 0 {
                self.pending_stream_offset
            } else {
                self.next_offset
            };
            let msg_id = self.next_msg_id;

            let segment_is_fin = is_last_segment && flags.is_fin;

            let start_chunk_index = self.pending_chunk_index;
            let mut chunk_index: u32 = start_chunk_index;
            let mut segment_remaining =
                segment_size - (start_chunk_index as usize * chunk_size as usize).min(segment_size);
            while segment_remaining > 0 {
                let chunk_len = (chunk_size as usize).min(segment_remaining);
                let mut payload = ByteVec::new();
                let mut writer = payload.with_write_limit(chunk_len);
                buf.infallible_copy_into(&mut writer);
                segment_remaining -= chunk_len;

                let flow_credits = self.take_credits(chunk_len);
                let frame = Frame {
                    header: Header::QueueMsg {
                        queue_pair: self.queue_pair(),
                        binding_id: self.control_rx.binding_id(),
                        msg_id: VarInt::new(msg_id).unwrap_or(VarInt::MAX),
                        stream_offset,
                        message_size: VarInt::new(segment_size as u64).unwrap_or(VarInt::MAX),
                        chunk_size: VarInt::new(chunk_size as u64).unwrap_or(VarInt::MAX),
                        chunk_index: VarInt::new(chunk_index as u64).unwrap_or(VarInt::MAX),
                        is_fin: segment_is_fin,
                        is_wakeup,
                        dest_acceptor_id: self.dest_acceptor_id(),
                        priority: self.wire_priority(),
                    },
                    payload,
                    path_secret_entry: self.path_secret_entry.clone(),
                    completion: Some(self.completion_rx.sender()),
                    status: frame::TransmissionStatus::default(),
                    ttl: DEFAULT_TTL,
                    enqueued_at: batch_enqueued_at,
                    flow_credits,
                };

                frames.push_back(frame.into());
                chunk_index += 1;

                // During init, send only the first chunk to probe the server.
                // The header declares the full message_size so the receiver
                // allocates the complete buffer, but we don't flood with data
                // before knowing the server has accepted.
                if force_first && chunk_index == 1 {
                    break;
                }
            }

            let chunks_sent = chunk_index - start_chunk_index;
            let bytes_sent = chunks_sent as usize * chunk_size as usize;
            let bytes_sent =
                bytes_sent.min(segment_size - start_chunk_index as usize * chunk_size as usize);

            if segment_remaining > 0 {
                // Partial segment: remember where to resume.
                self.pending_chunk_index = chunk_index;
                self.pending_segment_size = segment_size;
                self.pending_chunk_size = chunk_size;
                self.pending_stream_offset = stream_offset;

                self.metrics
                    .tx_msg_segment_size
                    .record_value(segment_size as u64);
                self.metrics
                    .tx_msg_chunks_per_segment
                    .record_value(chunks_sent as u64);

                self.advance_offset(bytes_sent)?;
                total_written += bytes_sent;
                break;
            } else {
                // Segment complete: advance msg_id and clear partial state.
                self.pending_chunk_index = 0;
                self.pending_segment_size = 0;
                self.pending_chunk_size = 0;
                self.pending_stream_offset = VarInt::ZERO;
                self.next_msg_id += 1;
            }

            self.metrics
                .tx_msg_segment_size
                .record_value(segment_size as u64);
            self.metrics
                .tx_msg_chunks_per_segment
                .record_value(chunks_sent as u64);

            self.advance_offset(bytes_sent)?;
            total_written += bytes_sent;

            if segment_is_fin {
                self.status.on_send_fin().ok();
            }
        }

        if !frames.is_empty() {
            self.send_batch(frames)?;
        }

        Ok(total_written)
    }

    fn remaining_offset_capacity(&self) -> usize {
        let remaining = VarInt::MAX
            .as_u64()
            .saturating_sub(self.next_offset.as_u64());

        usize::try_from(remaining).unwrap_or(usize::MAX)
    }

    fn advance_offset(&mut self, payload_len: usize) -> io::Result<()> {
        self.next_offset = self
            .next_offset
            .checked_add_usize(payload_len)
            .ok_or_else(offset_overflow_error)?;
        self.inflight_bytes += payload_len as u64;
        Ok(())
    }

    fn send_frame(&mut self, frame: Frame) -> io::Result<()> {
        self.frame_tx
            .send_batch(Entry::new(frame))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "frame channel closed"))
    }

    fn send_batch(&mut self, queue: Queue<Frame>) -> io::Result<()> {
        let priority = queue
            .iter()
            .next()
            .map(|f| f.priority())
            .unwrap_or(Priority::QueueData);
        self.frame_tx
            .send_batch(HomogeneousBatch { queue, priority })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "frame channel closed"))
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        debug!(
            binding_id = self.0.control_rx.binding_id().as_u64(),
            status = ?self.0.status,
            next_offset = self.0.next_offset.as_u64(),
            inflight_bytes = self.0.inflight_bytes,
            remote_max_data = self.0.remote_max_data.as_u64(),
            "Writer dropping"
        );

        self.0.drain_pending_reset();

        if std::thread::panicking() {
            self.0.completion_rx.cancel();

            let error_code = error::ABNORMAL_TERMINATION;
            let _ = self.0.send_reset_frame(error_code, ResetTarget::Both);
            debug!(
                binding_id = self.0.control_rx.binding_id().as_u64(),
                "Writer dropped during panic - sent QueueReset and cancelled transmissions"
            );
        } else {
            let _ = self.shutdown();
        }
        // The allocation itself is freed by `WriterAllocPtr::drop`, which runs
        // after this body returns. That drop branches on the credit slot's
        // state to handle the parked-acquire case (step 8).
    }
}

fn offset_overflow_error() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "stream offset overflow")
}

#[cfg(feature = "tokio")]
impl tokio::io::AsyncWrite for Writer {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut buf: &[u8],
    ) -> Poll<Result<usize, io::Error>> {
        self.poll_write_from(cx, &mut buf, false)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context,
        buf: &[std::io::IoSlice],
    ) -> Poll<Result<usize, io::Error>> {
        let mut buf = buffer::reader::storage::IoSlice::new(buf);
        self.poll_write_from(cx, &mut buf, false)
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        self.shutdown().into()
    }

    fn is_write_vectored(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests;
