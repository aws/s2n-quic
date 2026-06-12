// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! stream Reader: Reassembly and flow control
//!
//! The Reader receives out-of-order datagrams from the pipeline's flow queue stream channel,
//! reassembles them into an ordered byte stream using the Reassembler from s2n-quic-core,
//! and manages remote flow control by sending MAX_DATA frames to the peer.
//!
//! ## Design
//!
//! The Reader maintains a Reassembler buffer that handles out-of-order data. When datagrams
//! arrive through the flow queue stream channel, they're written into the Reassembler at their
//! offset. The Reassembler handles all the complexity of buffering gaps and delivering
//! contiguous data to the application.
//!
//! Flow control is managed by tracking how much data the application has consumed and
//! periodically sending MAX_DATA frames to increase the sender's window. The initial window
//! is advertised during flow establishment.

// TODOs:
//
// Correctness:
//
// * poll_stream_rx processes all messages in the queue but short-circuits on Reset/error.
//   Messages after the Reset in the same queue batch are silently dropped. This is probably
//   fine (reset is terminal), but any data messages preceding the reset in the queue should
//   still be written to the reassembler before we transition to Reset. Currently they are,
//   since we iterate in order and break on Reset, but if the queue ordering isn't guaranteed
//   this could lose data.
//
// * maybe_send_max_data uses consumed_len as the basis for the threshold check, but
//   consumed_len only advances when contiguous data is copied out. If the application reads
//   slowly while data arrives out-of-order, we could buffer large amounts in the reassembler
//   without ever sending MAX_DATA, which is correct (don't open the window for more data we
//   can't consume), but it means the sender will stall even though the reassembler has room.
//   We should consider whether buffered (non-contiguous) data should contribute to the
//   threshold.
//
// * No cap on reassembler memory. The Reassembler can buffer up to remote_max_data worth of
//   out-of-order data. If the window is large (6-7 MiB) and all of it arrives out-of-order,
//   the reassembler holds it all. This is by design but should be monitored — the actual
//   memory bound is the window size.
//
// Flow control:
//
// * Auto-tune window_size based on application drain rate. Currently fixed. If the
//   application drains slowly, shrink the window to reduce buffering. If it drains quickly,
//   grow the window to avoid sender stalls. This is the Reader-side analog of the Writer's
//   local budget auto-tuning.
//
// * Consider using completion notifications for MAX_DATA frames to provide backpressure and
//   limit the number of inflight MAX_DATA updates. This would also allow waking the
//   application when a new MAX_DATA is actually sent (vs queued).
//
// * The threshold for sending MAX_DATA (`consumed >= current_max - window/2`) can fire
//   repeatedly on every read once crossed. Each call to maybe_send_max_data recalculates and
//   may send another update even if consumed hasn't advanced since the last one. The fix is
//   to only send when the new MAX_DATA would actually increase the advertised value.
//
// Performance:
//
// * poll_read_into calls poll_stream_rx and then tries to copy out. If poll_stream_rx
//   returns Pending and the reassembler already has buffered data, we still try to copy
//   (which is fine). But if poll_stream_rx returns an error, we return the error immediately
//   without first draining any already-buffered contiguous data. On Reset, this may discard
//   data that was already reliably delivered and buffered.
//
// Observability:
//
// * No mechanism to expose reassembler buffering stats (gaps, buffered bytes, etc.) to the
//   application or metrics layer. Could be useful for diagnosing throughput issues.
//
// Testing:
//
// * Deterministic tests using bach for: out-of-order reassembly, FIN handling with gaps,
//   MAX_DATA generation and pacing, reset mid-stream, drop semantics (STOP_SENDING), and
//   interaction between slow application reads and flow control.

use crate::{
    byte_vec::ByteVec,
    endpoint::{
        error::{self, Error},
        frame::{self, FailureReason, Frame, Header, SubmissionSender, DEFAULT_TTL},
        msg,
    },
    intrusive,
    packet::datagram::{QueuePair, ResetTarget},
    path::secret::map::Entry as PathSecretEntry,
    stream::metrics::ReaderMetrics,
    tracing::{debug, trace},
};
use s2n_quic_core::{
    buffer::{
        self,
        duplex::Interposer,
        reader::{storage::Infallible as _, Incremental},
        reassembler::Reassembler,
        writer::{Storage as _, Writer as _},
    },
    ready,
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

/// The receive half of an `s2n-quic-dc` stream.
///
/// `Reader` presents an ordered byte stream even though the transport delivers
/// datagrams out of order. Incoming payloads are reassembled internally and are
/// only exposed to the application once the next contiguous bytes are ready.
///
/// # Expectations and guarantees
///
/// - Reads are in-order. Gaps stay hidden until missing data arrives.
/// - `read_into` returning `Ok(0)` means EOF: the peer's FIN has been fully
///   received and all preceding bytes have been consumed.
/// - If the peer resets the stream after some bytes were already buffered,
///   those buffered bytes are still readable before the reset becomes visible.
/// - When the `tokio` feature is enabled, `Reader` also implements
///   [`tokio::io::AsyncRead`].
///
/// # Footguns
///
/// - In debug builds, repeatedly calling `read_into` after it already returned
///   `Ok(0)` triggers a debug assertion so applications notice accidental
///   post-EOF spin loops.
/// - Dropping a reader before the peer finishes sending is treated as
///   cancellation and sends `STOP_SENDING` to the peer.
/// - [`peer_addr`](Self::peer_addr) is the handshake address associated with
///   the path secret, not a promise about the exact data path currently in use.
///
/// # Example
///
/// ```ignore
/// use s2n_quic_dc::stream::Reader;
///
/// async fn drain(mut reader: Reader) -> std::io::Result<Vec<u8>> {
///     let mut body = Vec::new();
///     while !reader.read_to_end(&mut body).await?.is_complete() {}
///
///     Ok(body)
/// }
/// ```
pub struct Reader(ReaderAllocPtr);

#[repr(C)]
struct ReaderAlloc {
    /// MUST live at offset 0 — the credit pool casts `NonNull<Slot>` back to
    /// `NonNull<ReaderAlloc>` via the registered `drop_fn`. Enforced at compile
    /// time by [`crate::assert_slot_at_offset_zero!`] below.
    slot: crate::credit::Slot,
    inner: Inner,
}

crate::assert_slot_at_offset_zero!(ReaderAlloc);

/// Owning pointer to a `ReaderAlloc`. Derefs to `Inner` so the reader body keeps
/// its `self.0.field` ergonomics, while drop is staged through the credit slot's
/// abandon/grant state machine: a parked acquire can transfer ownership of the
/// allocation to the recv credit pool, which then calls [`drop_reader_alloc`]
/// to free it.
struct ReaderAllocPtr(NonNull<ReaderAlloc>);

// SAFETY: `ReaderAllocPtr` owns the heap allocation exclusively. `Inner`'s
// fields are all `Send` (and not `Sync`), and `credit::Slot` is `Send`/`Sync`.
// The pool only ever reads/writes `Slot` fields under its own state machine; it
// never touches `Inner`.
unsafe impl Send for ReaderAllocPtr {}

impl ReaderAllocPtr {
    /// Allocate a `ReaderAlloc` initialized with `inner` and an idle (rc=APP)
    /// `Slot` registered against [`drop_reader_alloc`].
    fn new(inner: Inner) -> Self {
        let layout = Layout::new::<ReaderAlloc>();
        let raw = unsafe { alloc::alloc(layout) } as *mut ReaderAlloc;
        let ptr = NonNull::new(raw).unwrap_or_else(|| alloc::handle_alloc_error(layout));
        unsafe {
            std::ptr::write(
                ptr.as_ptr(),
                ReaderAlloc {
                    slot: crate::credit::Slot::new(drop_reader_alloc),
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

impl core::ops::Deref for ReaderAllocPtr {
    type Target = Inner;
    #[inline]
    fn deref(&self) -> &Inner {
        unsafe { &(*self.0.as_ptr()).inner }
    }
}

impl core::ops::DerefMut for ReaderAllocPtr {
    #[inline]
    fn deref_mut(&mut self) -> &mut Inner {
        unsafe { &mut (*self.0.as_ptr()).inner }
    }
}

impl Drop for ReaderAllocPtr {
    fn drop(&mut self) {
        // Two distinct quantities must return to the recv pool on drop:
        //
        // 1. The advertised-but-unfilled window. Each `poll_acquire` commits its
        //    grant straight into `remote_max_data`; the dispatch side only
        //    releases credit as bytes actually arrive. The window the reader
        //    advertised beyond what the peer sent is therefore still debited.
        //    `StreamReceiver::finish_recv_accounting` computes that leftover
        //    (`remote_max_data - max_received_offset - initial_window_remaining`)
        //    under the slot lock, idempotently and race-free against any
        //    concurrent dispatch release. We do this BEFORE `abandon` so it runs
        //    regardless of which ownership branch the CAS lands on.
        // 2. An unobserved distributor grant. If the distributor granted while
        //    we were unparked, the grant sits in the slot unseen by
        //    `poll_granted`; the `abandon` CAS surfaces it as `Granted(n)`.
        let inner = unsafe { &(*self.0.as_ptr()).inner };
        let leftover = inner
            .stream_rx
            .finish_recv_accounting(inner.remote_max_data.as_u64());
        if leftover > 0 {
            inner.recv_credit_pool.release(leftover);
        }

        // SAFETY: `abandon`'s relaxed contract permits calling it in any
        // APP-owned or LINKED state, exactly the range the slot can be in
        // when the reader drops.
        let slot = unsafe { &(*self.0.as_ptr()).slot };
        match unsafe { slot.abandon() } {
            crate::credit::AbandonResult::Abandoned => {
                // The slot was LINKED and is now DEAD. The pool's pop walk
                // will call `drop_reader_alloc` to free the allocation; we
                // must not touch it again. (The leftover release above already
                // ran and touched only the pool, not the slot allocation.)
                return;
            }
            crate::credit::AbandonResult::Granted(n) => {
                // We own the allocation. If the pool delivered a grant we
                // never observed, return it — otherwise it would leak from
                // the pool's accounting until restart.
                if n > 0 {
                    inner.recv_credit_pool.release(n);
                }
            }
            crate::credit::AbandonResult::Closed => {
                // Pool was dropped concurrently. We own the allocation; do
                // not touch the pool.
            }
        }
        // SAFETY: The slot is APP-owned and we hold the only reference. Drop
        // `Inner` and free the heap block.
        unsafe {
            std::ptr::drop_in_place(&raw mut (*self.0.as_ptr()).inner);
            alloc::dealloc(self.0.as_ptr().cast(), Layout::new::<ReaderAlloc>());
        }
    }
}

/// `drop_fn` invoked by the credit pool when it pops a dead slot — i.e. the
/// reader was dropped while its slot was linked, the pool then dequeued the
/// dead entry and now owns the allocation. Drops `Inner` and frees the block.
unsafe fn drop_reader_alloc(ptr: NonNull<crate::credit::Slot>) {
    // SAFETY: `Slot` lives at offset 0 of `ReaderAlloc` (see
    // `assert_slot_at_offset_zero!`), so the cast points back to the original
    // allocation. The pool guarantees this is called exactly once.
    let ptr = ptr.cast::<ReaderAlloc>();
    std::ptr::drop_in_place(&raw mut (*ptr.as_ptr()).inner);
    alloc::dealloc(ptr.as_ptr().cast(), Layout::new::<ReaderAlloc>());
}

use super::coop::{self, Coop, HasCoop};

/// Outcome of [`Reader::read_to_end`].
///
/// `Complete` means EOF was reached. `BufferFull` means the provided storage ran
/// out of remaining capacity before EOF and `read_to_end` should be called again
/// with more capacity to continue draining the stream.
#[must_use = "ReadToEnd indicates whether EOF was reached or another call is needed with more buffer capacity"]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadToEnd {
    /// EOF was reached. The `usize` is the number of bytes written into the buffer during this call.
    Complete(usize),
    /// The buffer ran out of capacity before EOF. The `usize` is the number of bytes written into the buffer during this call.
    BufferFull(usize),
}

impl ReadToEnd {
    /// Returns `true` when [`Reader::read_to_end`] reached EOF.
    #[inline]
    pub fn is_complete(self) -> bool {
        matches!(self, Self::Complete(_))
    }
}

struct Inner {
    /// Channel to submit frames to the wheel
    frame_tx: SubmissionSender,
    /// Receiver for failed completion notifications from the pipeline
    completion_rx: frame::CompletionReceiver,
    /// Stream-side channel for receiving data from the pipeline
    stream_rx: crate::queue::StreamReceiver,
    /// The peer's queue slot index (for sending MAX_DATA / Reset back)
    dest_queue_id: VarInt,
    /// Path secret entry providing MTU and crypto material
    path_secret_entry: Arc<PathSecretEntry>,
    /// Reassembly buffer for out-of-order data
    reassembler: Reassembler,
    /// Remote flow control: maximum offset we've advertised to the sender
    remote_max_data: VarInt,
    /// Unbacked window the reader may still advertise WITHOUT acquiring pool credit.
    ///
    /// Seeded to `local_recv_max_data` at construction (same value the slot seeds into its
    /// `initial_window_remaining`, see `path/secret/map/entry.rs`). The peer is already bounded by
    /// this handshake parameter, so this first window is "free" — the dispatch side never releases
    /// pool credit for bytes that land inside it, and `finish_recv_accounting` subtracts the unused
    /// remainder at termination. Advertising it therefore must NOT draw from the pool.
    ///
    /// This is what guarantees the server's first MAX_DATA — the one that confirms the binding and
    /// unblocks the peer writer out of `InitSent` — always goes out even when the recv pool is
    /// drained. Window growth *beyond* the unbacked window is pool-backed as before. Each MAX_DATA
    /// extension draws from this first, and only the remainder is acquired from the pool.
    unbacked_remaining: u64,
    /// Bootstrap window and top-up threshold (no longer the acquire ceiling). The advertised
    /// window now tracks the writer's hinted high watermark, grown multiplicatively on blocked
    /// signals up to the recv pool's `max_single_acquire`.
    window_size: u64,
    /// Highest absolute offset the writer has told us it wants to send (running max of the
    /// per-frame `peer_max_offset` hint and any blocked `desired_offset`). Zero until the first
    /// hint arrives, which keeps the bootstrap window behavior unchanged.
    peer_max_offset: u64,
    /// `consumed_len` at the most recent window-growth doubling. Growth is gated on `consumed`
    /// advancing past this mark, which paces doublings to roughly once per drained window and
    /// dedups the burst of blocked frames (and any retransmits/reorders) that arrive at the same
    /// consumption level. Keying on the *writer's* absolute `desired_offset` would be wrong: a bulk
    /// writer's high watermark is constant, so a single doubling would latch and the window would
    /// never open past `2 * window_size` while the writer stayed blocked.
    acted_blocked_offset: u64,
    /// Multiplicative window-growth factor (slow-start). Starts at 1×, doubles on each blocked
    /// signal that outstrips the current cap, clamped so the window can't exceed the pool's
    /// `max_single_acquire`. Held until the stream goes idle (never reset mid-stream).
    growth_ratio: u32,
    /// Endpoint-wide recv credit pool. Window-extension acquires draw from
    /// here; dispatch-side releases happen elsewhere as bytes arrive.
    recv_credit_pool: crate::sync::Arc<crate::credit::Pool>,
    /// Priority tier for `poll_acquire` calls against the recv pool.
    priority: crate::credit::Priority,
    /// Whether this endpoint should emit a flow update after FIN is consumed.
    /// Server-side readers set this to true so FIN consumption can act as an
    /// acceptance signal to the peer. Client-side readers set it to false since
    /// post-FIN credit updates are unnecessary once the peer is done sending.
    send_flow_update_after_fin: bool,
    /// Current status of the reader
    status: Status,
    /// Reset error code if the stream was reset by the peer
    reset_error_code: Option<VarInt>,
    /// Counts total EOF returns in debug builds so a second `Ok(0)` can trip a
    /// debug assertion and catch post-EOF spin loops.
    #[cfg(debug_assertions)]
    eof_counter: u8,
    /// Cooperative yield budget
    coop: Coop,
    /// Frames swapped out of the stream slot but not yet processed because the per-frame coop
    /// budget was exhausted mid-batch. Drained FIFO on the next `poll_stream_rx`, BEFORE any
    /// fresh `poll_swap`, preserving wire order. Empty in steady state — only non-empty between
    /// the budget-exhausted break and the next poll's drain.
    pending_rx: intrusive::Queue<msg::Stream>,
    /// Clock used to stamp `enqueued_at` on application-originated frames and to
    /// record the completion time when measuring sojourn durations.
    clock: crate::time::DefaultClock,
    /// Per-outcome sojourn time histograms shared with the application.
    metrics: Arc<ReaderMetrics>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Status {
    /// Flow is open for reads
    #[default]
    Open,
    /// Reset received from peer
    Reset,
    /// All data received and consumed (FIN reached)
    Complete,
}

impl Status {
    is!(is_open, Open);
    is!(is_reset, Reset);
    is!(is_complete, Complete);
    is!(is_terminal, Reset | Complete);

    event! {
        on_reset(Open => Reset);
        on_complete(Open => Complete);
    }
}

impl Reader {
    pub(crate) fn new_client(
        frame_tx: SubmissionSender,
        path_secret_entry: Arc<PathSecretEntry>,
        dest_queue_id: VarInt,
        stream_rx: crate::queue::StreamReceiver,
        clock: crate::time::DefaultClock,
        metrics: Arc<ReaderMetrics>,
        recv_credit_pool: crate::sync::Arc<crate::credit::Pool>,
        priority: crate::credit::Priority,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let remote_max_data = parameters.local_recv_max_data;
        let window_size = remote_max_data.as_u64();

        // Publish the initial advertised window so the dispatch side clamps its per-arrival credit
        // release to it from the first frame. The client advertises its full window up front, so
        // that is the ceiling. (`fetch_max`, so this composes with the bind-time seed.)
        stream_rx.advertise_window(window_size);

        Self(ReaderAllocPtr::new(Inner {
            frame_tx,
            completion_rx: frame::failure_completion_channel(),
            stream_rx,
            dest_queue_id,
            path_secret_entry,
            reassembler: Reassembler::new(),
            remote_max_data,
            // The client advertises its full unbacked window (`local_recv_max_data`) up front via
            // `remote_max_data`, so none remains to advertise later: invariant
            // `advertised_at_construction + unbacked_remaining == initial_window` ⇒ 0 here.
            unbacked_remaining: 0,
            window_size,
            peer_max_offset: 0,
            acted_blocked_offset: 0,
            growth_ratio: 1,
            recv_credit_pool,
            priority,
            send_flow_update_after_fin: false,
            status: Status::Open,
            reset_error_code: None,
            #[cfg(debug_assertions)]
            eof_counter: 0,
            coop: Coop::default(),
            pending_rx: intrusive::Queue::new(),
            clock,
            metrics,
        }))
    }

    pub(crate) fn new_server(
        frame_tx: SubmissionSender,
        path_secret_entry: Arc<PathSecretEntry>,
        dest_queue_id: VarInt,
        stream_rx: crate::queue::StreamReceiver,
        peer_fin_received: bool,
        clock: crate::time::DefaultClock,
        metrics: Arc<ReaderMetrics>,
        recv_credit_pool: crate::sync::Arc<crate::credit::Pool>,
        priority: crate::credit::Priority,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let window_size = parameters.local_recv_max_data.as_u64();

        // The server advertises 0 to the peer, but its unbacked initial window (`window_size`) is
        // the real dispatch-side ceiling during bootstrap: data within it is accepted before the
        // first MAX_DATA and must not be released as pool credit (it was never acquired). Publish
        // that ceiling so dispatch clamps to it. (`fetch_max`, composes with the bind-time seed.)
        stream_rx.advertise_window(window_size);

        Self(ReaderAllocPtr::new(Inner {
            frame_tx,
            completion_rx: frame::failure_completion_channel(),
            stream_rx,
            dest_queue_id,
            path_secret_entry,
            reassembler: Reassembler::new(),
            // Server starts at zero advertised window; its first consumption forces the MAX_DATA
            // that confirms the binding to the peer writer. That whole first window is unbacked
            // (the peer is bounded by the handshake `local_recv_max_data`), so it is advertised
            // without drawing pool credit — which is what lets binding confirmation proceed even
            // when the recv pool is drained.
            remote_max_data: VarInt::ZERO,
            unbacked_remaining: window_size,
            window_size,
            peer_max_offset: 0,
            acted_blocked_offset: 0,
            growth_ratio: 1,
            recv_credit_pool,
            priority,
            send_flow_update_after_fin: !peer_fin_received,
            status: Status::Open,
            reset_error_code: None,
            #[cfg(debug_assertions)]
            eof_counter: 0,
            coop: Coop::default(),
            pending_rx: intrusive::Queue::new(),
            clock,
            metrics,
        }))
    }

    /// Returns the stream identifier assigned when the flow was created.
    #[inline]
    pub fn binding_id(&self) -> u64 {
        self.0.stream_rx.binding_id().as_u64()
    }

    /// Returns the handshake peer address used to identify this stream.
    ///
    /// This is the stable endpoint identity for the peer, even if data is
    /// exchanged across multiple data paths.
    #[inline]
    pub fn peer_addr(&self) -> SocketAddr {
        *self.0.path_secret_entry.peer()
    }

    /// Returns the application data stored in the path secret entry for this stream.
    ///
    /// Application data is set at handshake time via the `make_application_data` callback
    /// and provides a way to carry negotiated per-connection state (e.g. authorization
    /// context, routing hints) into every stream opened on that path secret without
    /// maintaining a separate mapping.
    #[inline]
    pub fn application_data(&self) -> Option<&crate::path::secret::map::ApplicationData> {
        self.0.path_secret_entry.application_data().as_ref()
    }

    pub(crate) fn send_reset(&mut self, error_code: VarInt) {
        if self.0.status.is_terminal() {
            return;
        }
        let _ = self.0.send_reset_frame(error_code, ResetTarget::Both);
        self.0.status.on_reset().ok();
        self.0.reassembler.reset();
    }

    /// Transitions the reader to reset state without sending a reset frame.
    ///
    /// This is used when the caller will emit a reset via another path and only
    /// needs to suppress Drop-time STOP_SENDING behavior.
    pub(crate) fn force_reset(&mut self) {
        if self.0.status.is_terminal() {
            return;
        }
        self.0.status.on_reset().ok();
        self.0.reassembler.reset();
    }

    /// Reads the next contiguous bytes into the destination buffer.
    ///
    /// The returned byte count may be smaller than `buf`'s remaining capacity.
    /// A return value of `0` means EOF.
    ///
    /// # Semantics
    ///
    /// This call waits until one of the following happens:
    ///
    /// - contiguous stream data becomes available,
    /// - the peer's FIN is fully consumed and EOF can be reported,
    /// - a terminal error is ready to surface.
    ///
    /// Out-of-order packets may be received before this completes, but they stay
    /// buffered until the missing prefix arrives.
    ///
    /// # Footguns
    ///
    /// - `Ok(0)` is EOF, not "no bytes available right now".
    /// - In debug builds, repeatedly calling `read_into` after the first
    ///   `Ok(0)` triggers a debug assertion to catch EOF polling loops.
    /// - Use a loop if you need to fill a buffer or drain the whole stream.
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn read_frame(
    ///     reader: &mut s2n_quic_dc::stream::Reader,
    /// ) -> std::io::Result<Vec<u8>> {
    ///     let mut frame = [0; 4096];
    ///     let n = reader.read_into(&mut frame[..]).await?;
    ///     Ok(frame[..n].to_vec())
    /// }
    /// ```
    pub async fn read_into<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::writer::Storage,
    {
        core::future::poll_fn(|cx| self.poll_read_into(cx, buf)).await
    }

    /// Reads all remaining stream data into `buf`.
    ///
    /// Loops over [`read_into`][Self::read_into] until it returns `Ok(0)` (EOF),
    /// propagating any error immediately.
    ///
    /// # Buffer requirements
    ///
    /// `S` must be a buffer that can always accept more bytes — for example
    /// [`bytes::BytesMut`] or [`Vec<u8>`], which grow on demand. If `buf` has no
    /// remaining capacity (empty at call time or later filled for fixed-size
    /// storage), this method returns [`ReadToEnd::BufferFull`] so the caller can
    /// provide additional capacity and call again.
    ///
    /// # Footguns
    ///
    /// `BufferFull` does not mean EOF. It only means the stream still has data
    /// left and the destination buffer needs more capacity.
    ///
    /// Zero-copy, vectored destinations such as [`crate::byte_vec::ByteVec`]
    /// preserve the received chunking, which often means many small MTU-sized
    /// [`bytes::Bytes`] values. That can be a good fit for short-lived or
    /// scatter/gather processing, but if the buffered value will stay resident
    /// in memory for a while it is usually better to copy it into a more
    /// compact layout once enough bytes have accumulated.
    ///
    /// # Example
    ///
    /// ```ignore
    /// async fn collect_all(
    ///     reader: &mut s2n_quic_dc::stream::Reader,
    /// ) -> std::io::Result<Vec<u8>> {
    ///     let mut out = Vec::new();
    ///     while !reader.read_to_end(&mut out).await?.is_complete() {}
    ///     Ok(out)
    /// }
    /// ```
    pub async fn read_to_end<S>(&mut self, buf: &mut S) -> io::Result<ReadToEnd>
    where
        S: buffer::writer::Storage,
    {
        let mut len = 0;
        loop {
            if !buf.has_remaining_capacity() {
                return Ok(ReadToEnd::BufferFull(len));
            }

            let read_len = self.read_into(buf).await?;
            len += read_len;
            if read_len == 0 {
                return Ok(ReadToEnd::Complete(len));
            }
        }
    }

    /// Poll-based form of [`read_into`](Self::read_into).
    ///
    /// This follows the usual `Future::poll` contract: on `Pending`, the reader
    /// arranges for `cx.waker()` to be notified when progress may be possible.
    pub fn poll_read_into<S>(&mut self, cx: &mut Context, buf: &mut S) -> Poll<io::Result<usize>>
    where
        S: buffer::writer::Storage,
    {
        let slot = self.0.slot_ptr();
        // SAFETY: `slot` is the slot embedded at offset 0 of this reader's
        // `ReaderAlloc`. It is the only legal slot for `poll_acquire` because
        // the pool stores a waker for *this* task into it.
        unsafe { self.0.poll_read_into(cx, buf, slot) }
    }
}

impl HasCoop for Inner {
    #[inline]
    fn coop(&mut self) -> &mut Coop {
        &mut self.coop
    }
}

impl Inner {
    /// Checks the stream queue for a pending reset that was never polled.
    ///
    /// If the peer was declared dead (idle timeout), the queue contains a Reset
    /// we never consumed. Transitioning to reset here prevents the drop path
    /// from sending STOP_SENDING to a dead peer.
    fn drain_pending_reset(&mut self) {
        if self.status.is_reset() {
            return;
        }
        // Drain locally-stashed frames (older, behind the budget boundary) BEFORE pulling fresh
        // ones off the slot — a Reset stashed by a budget break must still suppress the
        // drop-time STOP_SENDING. Drain rather than iter so this mirrors the `try_swap` arm
        // below (both consume their respective queues; entries are dropped on the way out).
        for entry in core::mem::take(&mut self.pending_rx) {
            if matches!(&*entry, msg::Stream::Reset { .. }) {
                self.status.on_reset().ok();
                return;
            }
        }
        let Ok(queue) = self.stream_rx.try_swap() else {
            return;
        };
        for entry in queue {
            if matches!(&*entry, msg::Stream::Reset { .. }) {
                self.status.on_reset().ok();
                return;
            }
        }
    }

    fn reset_io_error(&self) -> io::Error {
        self.reset_error_code.map_or_else(
            || io::Error::from(io::ErrorKind::ConnectionReset),
            |code| {
                let err: Error = code.into();
                io::Error::new(err.io_error_kind(), err)
            },
        )
    }

    #[inline]
    fn ready_eof(&mut self) -> Poll<io::Result<usize>> {
        self.on_eof_returned();
        Poll::Ready(Ok(0))
    }

    #[cfg(debug_assertions)]
    #[inline]
    fn on_eof_returned(&mut self) {
        self.eof_counter = self.eof_counter.saturating_add(1);
        debug_assert!(
            self.eof_counter == 1,
            "Reader returned EOF again on stream {} (EOF count: {}). `read_into` returning Ok(0) means the peer's FIN was fully consumed and no more data will arrive. Stop calling `read_into` after the first Ok(0); repeated post-EOF reads usually mean the application treated EOF as \"try again later\" and is now spinning after the stream has completed.",
            self.stream_rx.binding_id().as_u64(),
            self.eof_counter,
        );
    }

    #[cfg(not(debug_assertions))]
    #[inline]
    fn on_eof_returned(&mut self) {}

    #[inline]
    unsafe fn poll_read_into<S>(
        &mut self,
        cx: &mut Context,
        buf: &mut S,
        slot: NonNull<crate::credit::Slot>,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::writer::Storage,
    {
        waker::debug_assert_contract(cx, |cx| {
            coop::poll(self, cx, |this, cx| unsafe {
                this.poll_read_into_inner(cx, buf, slot)
            })
        })
    }

    #[inline(always)]
    unsafe fn poll_read_into_inner<S>(
        &mut self,
        cx: &mut Context,
        buf: &mut S,
        slot: NonNull<crate::credit::Slot>,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::writer::Storage,
    {
        self.poll_completions(cx)?;

        // Once the stream is fully consumed, signal EOF without touching the
        // (potentially already-closed) stream channel.
        if self.status.is_complete() {
            return self.ready_eof();
        }

        // If the stream was previously reset, drain any buffered data first
        // (matching TCP semantics: data in the receive buffer before a RST is
        // still readable).  Once the reassembler is empty every subsequent
        // call returns the sticky error.
        if self.status.is_reset() && self.reassembler.is_empty() {
            self.reassembler.reset(); // free cursor metadata
            return Poll::Ready(Err(self.reset_io_error()));
        }

        let mut tracker = buf.track_write();

        // If already in reset state, skip the channel poll — no new messages
        // will arrive.  Drain the reassembler and surface the error when empty.
        // Otherwise poll for new stream messages.  Defer any channel error
        // (BrokenPipe or ConnectionReset) while the reassembler still has data,
        // delivering all buffered bytes to the application first.
        let deferred_err = if self.status.is_reset() {
            Some(self.reset_io_error())
        } else {
            let stream_result = self.poll_stream_rx(cx, &mut tracker);
            match stream_result {
                Poll::Ready(Ok(())) => None,
                // Defer the error while the reassembler still has data to give
                // to the application (either all writes complete, or a reset
                // arrived but data was already buffered).
                Poll::Ready(Err(e))
                    if self.reassembler.is_writing_complete() || !self.reassembler.is_empty() =>
                {
                    Some(e)
                }
                // The reassembler may already hold data from a prior poll (e.g.
                // poll_validate consumed early data). Fall through so we drain
                // the reassembler and call maybe_send_max_data.
                Poll::Pending if !self.reassembler.is_empty() => None,
                Poll::Pending => {
                    // Even with no data available, try to extend the window so
                    // the peer can start (or keep) transmitting. Pending here
                    // means the pool didn't grant — that's fine, we're already
                    // returning Pending anyway.
                    // SAFETY: caller's invariant — `slot` is this reader's idle slot.
                    unsafe { self.maybe_send_max_data(cx, slot)? };
                    return Poll::Pending;
                }
                other => return other.map_ok(|()| 0usize),
            }
        };

        if tracker.has_remaining_capacity() {
            self.reassembler.infallible_copy_into(&mut tracker);
        }

        let bytes_read = tracker.written_len();

        // Only update flow-control while the channel is healthy.  When
        // `deferred_err` is set the sender's channel has already closed, which
        // means no more data is coming and there is nothing to send MAX_DATA to.
        // Attempting to send in that state would produce an error that discards
        // the data we just buffered, which is wrong.
        if deferred_err.is_none() {
            // SAFETY: caller's invariant — `slot` is this reader's idle slot.
            unsafe { self.maybe_send_max_data(cx, slot)? };
        }

        if self.reassembler.is_reading_complete() {
            debug!(
                binding_id = self.stream_rx.binding_id().as_u64(),
                final_size = ?self.reassembler.final_size(),
                consumed_len = self.reassembler.consumed_len(),
                "Reader complete - all data consumed"
            );
            self.status.on_complete().ok();
            if bytes_read == 0 {
                return self.ready_eof();
            }
            return Poll::Ready(Ok(bytes_read));
        }

        if bytes_read > 0 {
            return Poll::Ready(Ok(bytes_read));
        }

        // No data was consumed.  If the channel had a deferred error, surface
        // it now that the reassembler is exhausted.
        if let Some(e) = deferred_err {
            return Poll::Ready(Err(e));
        }

        Poll::Pending
    }

    fn poll_stream_rx<S>(&mut self, cx: &mut Context, app_buf: &mut S) -> Poll<io::Result<()>>
    where
        S: buffer::writer::Storage + ?Sized,
    {
        // Drain any frames stashed by a previous budget-exhausted break BEFORE pulling a fresh
        // batch off the slot — preserves wire order, since `pending_rx` always holds frames
        // that arrived earlier than anything still in the slot. When `pending_rx` is non-empty
        // we skip `poll_swap` entirely so we don't register the slot waker without need; the
        // gate's self-wake (when budget exhausts again) covers re-entry.
        let mut queue = if !self.pending_rx.is_empty() {
            core::mem::take(&mut self.pending_rx)
        } else {
            match self.stream_rx.poll_swap(cx) {
                Poll::Ready(Ok(q)) => q,
                Poll::Ready(Err(_)) => {
                    return Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::BrokenPipe,
                        "stream channel closed",
                    )))
                }
                Poll::Pending => return Poll::Pending,
            }
        };

        while let Some(entry) = queue.pop_front() {
            // Per-frame coop accounting.
            //
            // Break-safe: `entry` was just popped but not yet consumed (its `into_inner` is in
            // the match below). On budget-exhaust we push it back to the front of `queue` so
            // the next drain sees it first, then move `queue` into `pending_rx`. `pending_rx`
            // is provably empty here (we drained it above; no frame can have entered it since
            // the field is only written here). Returning `Ready(Ok(()))` lets the outer
            // `poll_read_into_inner` deliver the bytes we already wrote into the reassembler;
            // the gate yields on the *next* poll because budget is now zero.
            if !self.coop.consume() {
                debug_assert!(self.pending_rx.is_empty());
                queue.push_front(entry);
                self.pending_rx = queue;
                return Poll::Ready(Ok(()));
            }
            match entry.into_inner() {
                msg::Stream::Data {
                    offset,
                    peer_max_offset,
                    mut payload,
                    fin,
                    blocked,
                } => {
                    // Track the writer's desired high watermark and, if it signaled it is
                    // blocked, possibly grow the window. Done before the receive-window
                    // check so a blocked signal still drives growth even on an empty frame.
                    self.peer_max_offset = self.peer_max_offset.max(peer_max_offset.as_u64());
                    if blocked {
                        self.on_blocked_signal(peer_max_offset.as_u64());
                    }

                    let Some(payload_end_offset) =
                        offset.as_u64().checked_add(payload.len() as u64)
                    else {
                        debug!(
                            binding_id = self.stream_rx.binding_id().as_u64(),
                            offset = offset.as_u64(),
                            payload_len = payload.len(),
                            "Incoming data offset overflowed"
                        );
                        return self.protocol_error();
                    };

                    // Server bootstrap special-case:
                    // `remote_max_data == 0` is used for server-side
                    // streams before initial validation/credit release.
                    // In that state the first bytes are accepted without
                    // hard receive-window enforcement; once credits are
                    // advertised (`remote_max_data > 0`) the check below
                    // is enforced for all subsequent packets.
                    if self.remote_max_data != VarInt::ZERO
                        && payload_end_offset > self.remote_max_data.as_u64()
                    {
                        debug!(
                            binding_id = self.stream_rx.binding_id().as_u64(),
                            offset = offset.as_u64(),
                            payload_len = payload.len(),
                            payload_end_offset,
                            remote_max_data = self.remote_max_data.as_u64(),
                            "Peer exceeded advertised receive window"
                        );
                        return self.queue_control_error();
                    }

                    trace!(
                        binding_id = self.stream_rx.binding_id().as_u64(),
                        offset = offset.as_u64(),
                        len = payload.len(),
                        is_fin = fin,
                        "Received data"
                    );

                    let mut incremental = Incremental::new(offset);
                    let mut reader = match incremental.with_storage(&mut payload, fin) {
                        Ok(r) => r,
                        Err(err) => {
                            debug!(
                                binding_id = self.stream_rx.binding_id().as_u64(),
                                ?err,
                                "Invalid storage/fin combination"
                            );
                            return self.protocol_error();
                        }
                    };

                    if let Err(err) = write_data_reader(&mut self.reassembler, &mut reader, app_buf)
                    {
                        debug!(
                            binding_id = self.stream_rx.binding_id().as_u64(),
                            ?err,
                            "Failed to write to reassembler"
                        );
                        return self.protocol_error();
                    }
                }
                msg::Stream::Blocked { desired_offset } => {
                    // Standalone blocked signal (cold-start path): no payload, just drives
                    // window growth. The subsequent maybe_send_max_data in the caller
                    // emits the extension.
                    trace!(
                        binding_id = self.stream_rx.binding_id().as_u64(),
                        desired_offset = desired_offset.as_u64(),
                        "Received QueueDataBlocked"
                    );
                    self.peer_max_offset = self.peer_max_offset.max(desired_offset.as_u64());
                    self.on_blocked_signal(desired_offset.as_u64());
                }
                msg::Stream::Reset { error_code } => {
                    debug!(
                        binding_id = self.stream_rx.binding_id().as_u64(),
                        error_code = error_code.as_u64(),
                        "Stream reset by peer"
                    );
                    self.reset_error_code = Some(error_code);
                    self.status.on_reset().ok();
                    // Only clear the reassembler immediately when it is
                    // already empty.  If data was buffered before the
                    // reset arrived, leave it intact so poll_read_into_inner
                    // can drain it to the application first (TCP semantics:
                    // data in the receive buffer before a RST is readable).
                    if self.reassembler.is_empty() {
                        self.reassembler.reset();
                    }
                    let reset_error: Error = error_code.into();
                    return Poll::Ready(Err(io::Error::new(
                        reset_error.io_error_kind(),
                        reset_error,
                    )));
                }
            }
        }

        Poll::Ready(Ok(()))
    }

    fn protocol_error(&mut self) -> Poll<io::Result<()>> {
        let error_code = error::FRAME_DECODE_ERROR;
        self.reset_error_code = Some(error_code);
        self.status.on_reset().ok();
        self.reassembler.reset();
        let _ = self.send_reset_frame(error_code, ResetTarget::Both);
        let reset_error: Error = error_code.into();
        Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidData, reset_error)))
    }

    fn queue_control_error(&mut self) -> Poll<io::Result<()>> {
        let error_code = error::QUEUE_CONTROL_ERROR;
        self.reset_error_code = Some(error_code);
        self.status.on_reset().ok();
        self.reassembler.reset();
        let _ = self.send_reset_frame(error_code, ResetTarget::Both);
        let reset_error: Error = error_code.into();
        Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidData, reset_error)))
    }

    /// Maximum window-growth ratio: the most we can scale `window_size` before the resulting
    /// acquire would exceed the pool's per-request ceiling. Clamping the *ratio* (not just the
    /// byte target) is what makes the doubling terminate.
    #[inline]
    fn max_growth_ratio(&self) -> u32 {
        let max_single = self.recv_credit_pool.max_single_acquire(self.priority);
        let ratio = max_single / self.window_size.max(1);
        (ratio as u32).max(1)
    }

    /// React to a blocked signal (in-band `blocked` bit or standalone `QueueDataBlocked`).
    ///
    /// Doubles the growth ratio when the writer still wants past our current runway (`desired >
    /// cap`) AND the application has made progress since the last doubling (`consumed >
    /// acted_blocked_offset`). The two conditions together mean: the writer is *persistently*
    /// blocked — it keeps hitting the window edge even as we drain — so we widen the runway. The
    /// `consumed`-advanced gate both paces growth (≈ one doubling per drained window) and dedups the
    /// burst of blocked frames / retransmits that share a consumption level.
    ///
    /// Growth terminates on its own: once the runway outpaces the writer (`desired <= cap`) the
    /// writer stops setting the blocked bit, so no more signals arrive; and `growth_ratio` is hard
    /// capped at `max_growth_ratio` regardless. The ratio is held (never reset mid-stream) to avoid
    /// thrashing during steady state.
    fn on_blocked_signal(&mut self, desired: u64) {
        let consumed = self.reassembler.consumed_len();
        let cap =
            consumed.saturating_add(self.window_size.saturating_mul(self.growth_ratio as u64));
        if desired > cap && consumed >= self.acted_blocked_offset {
            self.acted_blocked_offset = consumed.saturating_add(self.window_size);
            let prev = self.growth_ratio;
            self.growth_ratio = self
                .growth_ratio
                .saturating_mul(2)
                .min(self.max_growth_ratio());
            trace!(
                binding_id = self.stream_rx.binding_id().as_u64(),
                desired,
                consumed,
                cap,
                prev_growth_ratio = prev,
                growth_ratio = self.growth_ratio,
                "on_blocked_signal: growing window"
            );
        }
    }

    /// Decide whether to extend the advertised window and, if so, acquire the
    /// new credit from the recv pool before sending MAX_DATA.
    ///
    /// `slot` must point to the embedded `Slot` of the `ReaderAlloc` that owns
    /// this `Inner`; the pool stores a waker pointing at our task on `Pending`.
    ///
    /// Re-entry safety: a prior call may have parked the slot. Each invocation
    /// drains any delivered grant via `poll_granted` and short-circuits on
    /// `Pending` so we never call `prepare_park` twice on the same slot — that
    /// would violate the credit pool's refcount=APP precondition. This mirrors
    /// the writer's `poll_acquire_credits` pattern at
    /// [stream/writer.rs:poll_acquire_credits].
    ///
    /// `Poll::Pending` here means "leave the advertised window where it is."
    /// It does not stall application reads — the reader keeps draining
    /// whatever data is already buffered. The distributor will wake the
    /// reader's task when credit becomes available.
    ///
    /// # Safety
    ///
    /// `slot` must be the slot embedded at offset 0 of this reader's
    /// `ReaderAlloc`. It is the only legal slot because `Pool::poll_acquire`
    /// writes a waker for *this* task into it.
    unsafe fn maybe_send_max_data(
        &mut self,
        cx: &mut Context,
        slot: NonNull<crate::credit::Slot>,
    ) -> io::Result<()> {
        // Drain any grant the distributor delivered while we were away. This
        // also tells us whether the slot is still parked from a prior poll —
        // in which case we MUST NOT issue a fresh acquire (the slot's
        // `prepare_park` requires refcount=APP and would panic on RC_LINKED).
        let slot_ref = unsafe { slot.as_ref() };
        let prior_grant = match slot_ref.poll_granted() {
            crate::credit::GrantResult::Pending => {
                // Slot is still linked into the pool's tier. The distributor
                // will wake us when it grants; nothing more to do this turn.
                return Ok(());
            }
            crate::credit::GrantResult::Closed => {
                // Pool was dropped; surface as broken pipe so subsequent
                // polls don't loop.
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "recv credit pool closed",
                ));
            }
            crate::credit::GrantResult::Granted(n) => n,
        };

        if let Some(final_size) = self.reassembler.final_size() {
            if !self.send_flow_update_after_fin {
                if prior_grant > 0 {
                    self.recv_credit_pool.release(prior_grant);
                }
                return Ok(());
            }

            if self.remote_max_data.as_u64() >= final_size {
                if prior_grant > 0 {
                    self.recv_credit_pool.release(prior_grant);
                }
                return Ok(());
            }
        }

        let consumed = self.reassembler.consumed_len();
        let current_max = self.remote_max_data.as_u64();
        let threshold = current_max.saturating_sub(self.window_size / 2);

        // Target window: sized to the writer's hinted demand, capped by the (possibly grown) local
        // window, and never below what we've already consumed. Before any hint arrives
        // (`peer_max_offset == 0`) preserve the original fixed bootstrap window so server/client
        // startup is unchanged.
        let cap =
            consumed.saturating_add(self.window_size.saturating_mul(self.growth_ratio as u64));
        let target_max_data = if self.peer_max_offset == 0 {
            consumed.saturating_add(self.window_size)
        } else {
            self.peer_max_offset.max(consumed).min(cap)
        };

        // Trigger on hint-or-threshold: extend when the writer wants more room than we've advertised
        // (`target > current_max`) OR consumption crossed the top-up threshold.
        let wants_more = target_max_data > current_max;
        if consumed < threshold && !wants_more {
            trace!(
                binding_id = self.stream_rx.binding_id().as_u64(),
                consumed,
                current_max,
                threshold,
                target_max_data,
                peer_max_offset = self.peer_max_offset,
                growth_ratio = self.growth_ratio,
                window_size = self.window_size,
                "maybe_send_max_data: below threshold and no new demand, not sending"
            );
            if prior_grant > 0 {
                self.recv_credit_pool.release(prior_grant);
            }
            return Ok(());
        }

        let delta = target_max_data.saturating_sub(current_max);
        if delta == 0 {
            if prior_grant > 0 {
                self.recv_credit_pool.release(prior_grant);
            }
            return Ok(());
        }

        trace!(
            binding_id = self.stream_rx.binding_id().as_u64(),
            consumed,
            current_max,
            target_max_data,
            delta,
            peer_max_offset = self.peer_max_offset,
            growth_ratio = self.growth_ratio,
            "maybe_send_max_data: extending window"
        );

        // Cover the extension from three sources, cheapest first:
        //   1. `unbacked_remaining` — the initial window we may advertise for free (no pool
        //      credit). This always lets the first/confirming MAX_DATA go out, even against a
        //      drained pool, which is what prevents the binding-confirmation deadlock.
        //   2. `prior_grant` — credit the distributor already delivered to our slot.
        //   3. a fresh `poll_acquire` for whatever remains.
        let from_unbacked = self.unbacked_remaining.min(delta);
        self.unbacked_remaining -= from_unbacked;
        let mut granted = from_unbacked;

        let remaining = delta - granted;
        let from_prior = prior_grant.min(remaining);
        let surplus = prior_grant - from_prior;
        if surplus > 0 {
            self.recv_credit_pool.release(surplus);
        }
        granted += from_prior;

        let need = delta - granted;
        if need > 0 {
            // SAFETY: caller's invariant — `slot` is this reader's idle slot,
            // and `poll_granted` above confirmed it is currently APP-owned
            // (RC=1), satisfying `poll_acquire`'s precondition.
            match unsafe {
                self.recv_credit_pool
                    .poll_acquire(cx, slot, need, self.priority)
            } {
                Poll::Ready(n) => {
                    granted = granted.saturating_add(n);
                }
                Poll::Pending => {
                    // We parked for `need` more credit. Any credit already
                    // collected (unbacked + prior grant) is consumed by
                    // advertising it now — there is no `pending_credits` carry
                    // on the reader, so dropping it would strand it. Advertise
                    // the partial and let the next poll re-evaluate when the
                    // distributor delivers the rest.
                    //
                    // Recv-side stall signal: we wanted to grow the window but
                    // the pool couldn't satisfy it. A partial may still go out
                    // below; `granted == 0` is the harder stall, counted there.
                    self.metrics.flow.max_data_credit_parked.add(1);
                }
            }
        }

        if granted == 0 {
            // Hard stall: the writer asked for more room (we passed the
            // hint/threshold gate above) but we collected no credit at all, so
            // the window does not move this turn. The peer writer stays blocked.
            self.metrics.flow.max_data_granted_zero.add(1);
            return Ok(());
        }

        let new_max_data = current_max.saturating_add(granted);
        let new_max_data = VarInt::new(new_max_data)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "max_data overflow"))?;

        // Frame send errors are propagated: if we cannot communicate flow
        // control credits the peer may stall, so it is better to surface
        // the failure immediately.
        if let Err(err) = self.send_max_data_frame(new_max_data) {
            // Frame submission failed — undo the credit we collected for this
            // extension so accounting stays balanced. Only `granted -
            // from_unbacked` came from the pool (prior grant + fresh acquire);
            // `from_unbacked` was drawn from `unbacked_remaining`, never the
            // pool, so releasing it would inject phantom pool credit. Return
            // the unbacked portion to its own budget instead so it is not lost.
            let pool_backed = granted - from_unbacked;
            if pool_backed > 0 {
                self.recv_credit_pool.release(pool_backed);
            }
            self.unbacked_remaining += from_unbacked;
            return Err(err);
        }
        self.remote_max_data = new_max_data;
        // Publish the grown window so the dispatch side can clamp its per-arrival
        // credit release to what we actually advertised (and acquired). Monotonic
        // and lock-free; see `Slot::advertise_window`.
        self.stream_rx.advertise_window(new_max_data.as_u64());

        Ok(())
    }

    fn poll_completions(&mut self, cx: &mut Context) -> io::Result<()> {
        match self.completion_rx.poll_swap(cx) {
            Poll::Ready(Some(queue)) => {
                let mut failure = None;

                // Snapshot current time once for all sojourn measurements in
                // this completion batch.
                let completed_at = self.clock.now();

                for completed in queue.iter() {
                    // Record sojourn time for frames that carry an enqueue stamp.
                    if let Some(enqueued_at) = completed.enqueued_at {
                        let failure_reason = match completed.status {
                            frame::TransmissionStatus::Failed(r) => Some(r),
                            _ => None,
                        };
                        self.metrics
                            .sojourn
                            .record(enqueued_at, completed_at, failure_reason);
                    }

                    if let frame::TransmissionStatus::Failed(reason) = completed.status {
                        if let Some(existing) = failure {
                            debug!(
                                binding_id = self.stream_rx.binding_id().as_u64(),
                                first = ?existing,
                                additional = ?reason,
                                "observed additional transmission failure"
                            );
                        } else {
                            failure = Some(reason);
                        }
                    }
                }

                if let Some(reason) = failure {
                    return match reason {
                        FailureReason::UnknownPathSecret => Err(io::Error::new(
                            io::ErrorKind::ConnectionRefused,
                            "path secret rejected by peer",
                        )),
                        FailureReason::PeerDead => Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "peer declared dead (idle timeout)",
                        )),
                        FailureReason::TransmissionError => Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "transmission failed after retries",
                        )),
                        FailureReason::Cancelled => Err(io::Error::new(
                            io::ErrorKind::Interrupted,
                            "transmission cancelled",
                        )),
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

    fn send_max_data_frame(&mut self, maximum_data: VarInt) -> io::Result<()> {
        let frame = Frame {
            header: Header::QueueMaxData {
                queue_pair: QueuePair {
                    source_queue_id: self.stream_rx.queue_id(),
                    dest_queue_id: self.dest_queue_id,
                },
                binding_id: self.stream_rx.binding_id(),
                maximum_data,
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

        trace!(
            binding_id = self.stream_rx.binding_id().as_u64(),
            maximum_data = maximum_data.as_u64(),
            "Sent MAX_DATA"
        );

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
                binding_id: self.stream_rx.binding_id(),
                reset_target,
                error_code,
                dest_acceptor_id: None,
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
            binding_id = self.stream_rx.binding_id().as_u64(),
            error_code = error_code.as_u64(),
            ?reset_target,
            "Sent QueueReset"
        );

        Ok(())
    }

    fn send_frame(&mut self, frame: Frame) -> io::Result<()> {
        self.frame_tx
            .send_batch(intrusive::Entry::new(frame))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "frame channel closed"))
    }
}

#[inline]
fn write_data_reader<S, R>(
    reassembler: &mut Reassembler,
    reader: &mut R,
    app_buf: &mut S,
) -> Result<(), buffer::Error<R::Error>>
where
    S: buffer::writer::Storage + ?Sized,
    R: buffer::reader::Reader + ?Sized,
{
    if reassembler.is_empty() {
        let mut interposer = Interposer::new(app_buf, reassembler);
        interposer.read_from(reader)
    } else {
        reassembler.write_reader(reader)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        debug!(
            binding_id = self.0.stream_rx.binding_id().as_u64(),
            status = ?self.0.status,
            final_size = ?self.0.reassembler.final_size(),
            consumed_len = self.0.reassembler.consumed_len(),
            total_received_len = self.0.reassembler.total_received_len(),
            is_writing_complete = self.0.reassembler.is_writing_complete(),
            is_reading_complete = self.0.reassembler.is_reading_complete(),
            "Reader dropping"
        );

        self.0.drain_pending_reset();

        if std::thread::panicking() {
            let error_code = error::ABNORMAL_TERMINATION;
            let _ = self.0.send_reset_frame(error_code, ResetTarget::Both);
            debug!(
                binding_id = self.0.stream_rx.binding_id().as_u64(),
                "Reader dropped during panic - sent QueueReset"
            );
        } else if !self.0.reassembler.is_writing_complete() && !self.0.status.is_reset() {
            let error_code = error::STOP_SENDING;
            // STOP_SENDING must target the *writer* on the peer side, which
            // polls the *control* channel.  Using ResetTarget::Stream would
            // route the reset to the peer's reader (stream queue) instead and
            // the peer's writer would never observe the signal.
            let _ = self.0.send_reset_frame(error_code, ResetTarget::Control);
            debug!(
                binding_id = self.0.stream_rx.binding_id().as_u64(),
                "Reader dropped before FIN received - sent STOP_SENDING"
            );
        }
    }
}

#[cfg(feature = "tokio")]
impl tokio::io::AsyncRead for Reader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut buf = buffer::writer::storage::BufMut::new(buf);
        ready!(self.poll_read_into(cx, &mut buf))?;
        Poll::Ready(Ok(()))
    }
}

#[cfg(test)]
mod tests;
