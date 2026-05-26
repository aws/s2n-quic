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
        id::LocalSenderId,
        msg,
    },
    intrusive,
    packet::datagram::{QueuePair, ResetTarget},
    path::secret::map::Entry as PathSecretEntry,
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
    io,
    net::SocketAddr,
    pin::Pin,
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
pub struct Reader(Box<Inner>);

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
    stream_rx: msg::queue::Stream,
    /// Path secret entry providing MTU and crypto material
    path_secret_entry: Arc<PathSecretEntry>,
    /// Stream identifier
    stream_id: VarInt,
    /// Reassembly buffer for out-of-order data
    reassembler: Reassembler,
    /// Remote flow control: maximum offset we've advertised to the sender
    remote_max_data: VarInt,
    /// Window size for flow control
    window_size: u64,
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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Status {
    /// Server-only: awaiting client validation before releasing credits
    PendingValidation,
    /// Flow is open for reads
    #[default]
    Open,
    /// Reset received from peer
    Reset,
    /// All data received and consumed (FIN reached)
    Complete,
}

impl Status {
    is!(is_pending_validation, PendingValidation);
    is!(is_open, Open);
    is!(is_reset, Reset);
    is!(is_complete, Complete);
    is!(is_terminal, Reset | Complete);

    event! {
        on_validated(PendingValidation => Open);
        on_reset(PendingValidation | Open => Reset);
        on_complete(Open => Complete);
    }
}

impl Reader {
    pub(crate) fn new_client(
        frame_tx: SubmissionSender,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        stream_rx: msg::queue::Stream,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let remote_max_data = parameters.local_recv_max_data;
        let window_size = remote_max_data.as_u64();

        Self(Box::new(Inner {
            frame_tx,
            completion_rx: frame::failure_completion_channel(),
            stream_rx,
            path_secret_entry,
            stream_id,
            reassembler: Reassembler::new(),
            remote_max_data,
            window_size,
            send_flow_update_after_fin: false,
            status: Status::Open,
            reset_error_code: None,
            #[cfg(debug_assertions)]
            eof_counter: 0,
            coop: Coop::default(),
        }))
    }

    pub(crate) fn new_server(
        frame_tx: SubmissionSender,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        stream_rx: msg::queue::Stream,
        peer_fin_received: bool,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let window_size = parameters.local_recv_max_data.as_u64();

        Self(Box::new(Inner {
            frame_tx,
            completion_rx: frame::failure_completion_channel(),
            stream_rx,
            path_secret_entry,
            stream_id,
            reassembler: Reassembler::new(),
            remote_max_data: VarInt::ZERO,
            window_size,
            send_flow_update_after_fin: !peer_fin_received,
            status: Status::Open,
            reset_error_code: None,
            #[cfg(debug_assertions)]
            eof_counter: 0,
            coop: Coop::default(),
        }))
    }

    pub(crate) fn new_server_pending(
        frame_tx: SubmissionSender,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        stream_rx: msg::queue::Stream,
        peer_fin_received: bool,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let window_size = parameters.local_recv_max_data.as_u64();

        Self(Box::new(Inner {
            frame_tx,
            completion_rx: frame::failure_completion_channel(),
            stream_rx,
            path_secret_entry,
            stream_id,
            reassembler: Reassembler::new(),
            remote_max_data: VarInt::ZERO,
            window_size,
            send_flow_update_after_fin: !peer_fin_received,
            status: Status::PendingValidation,
            reset_error_code: None,
            #[cfg(debug_assertions)]
            eof_counter: 0,
            coop: Coop::default(),
        }))
    }

    /// Waits for the reader to become valid for application use.
    ///
    /// Client-side readers are already validated, so this is usually a no-op.
    /// Server-side readers may need to wait until the transport confirms that
    /// the incoming flow is acceptable.
    ///
    /// # Guarantees
    ///
    /// - Once this returns `Ok(())`, subsequent reads will no longer fail with
    ///   "stream not yet validated".
    /// - Calling it multiple times is harmless.
    ///
    /// # Footguns
    ///
    /// This method has no built-in timeout. If validation is part of a request
    /// deadline, wrap it in your own timeout.
    pub(crate) async fn validate(&mut self) -> io::Result<()> {
        core::future::poll_fn(|cx| self.0.poll_validate(cx)).await
    }

    #[inline]
    pub(crate) fn is_validated(&self) -> bool {
        !self.0.status.is_pending_validation()
    }

    /// Returns the stream identifier assigned when the flow was created.
    #[inline]
    pub fn stream_id(&self) -> u64 {
        self.0.stream_id.as_u64()
    }

    /// Returns the handshake peer address used to identify this stream.
    ///
    /// This is the stable endpoint identity for the peer, even if data is
    /// exchanged across multiple data paths.
    #[inline]
    pub fn peer_addr(&self) -> SocketAddr {
        *self.0.path_secret_entry.peer()
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
        self.0.poll_read_into(cx, buf)
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
            self.stream_id.as_u64(),
            self.eof_counter,
        );
    }

    #[cfg(not(debug_assertions))]
    #[inline]
    fn on_eof_returned(&mut self) {}

    #[inline]
    fn poll_validate(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        self.poll_completions(cx)?;

        if !self.status.is_pending_validation() {
            return Poll::Ready(Ok(()));
        }

        let mut app_buf = buffer::writer::storage::Empty;
        match self.poll_stream_rx(cx, &mut app_buf)? {
            Poll::Ready(()) => {
                if self.status.is_pending_validation() {
                    Poll::Pending
                } else {
                    Poll::Ready(Ok(()))
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }

    #[inline]
    fn poll_read_into<S>(&mut self, cx: &mut Context, buf: &mut S) -> Poll<io::Result<usize>>
    where
        S: buffer::writer::Storage,
    {
        waker::debug_assert_contract(cx, |cx| {
            coop::poll(self, cx, |this, cx| this.poll_read_into_inner(cx, buf))
        })
    }

    #[inline(always)]
    fn poll_read_into_inner<S>(&mut self, cx: &mut Context, buf: &mut S) -> Poll<io::Result<usize>>
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
                other => return other.map_ok(|()| 0usize),
            }
        };

        if self.status.is_pending_validation() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stream not yet validated - call validate() first",
            )));
        }

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
            self.maybe_send_max_data()?;
        }

        if self.reassembler.is_reading_complete() {
            debug!(
                stream_id = self.stream_id.as_u64(),
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
        let interpose = !self.status.is_pending_validation();

        match self.stream_rx.poll_swap(cx) {
            Poll::Ready(Ok(queue)) => {
                for msg in queue {
                    match msg.into_inner() {
                        msg::Stream::Data {
                            offset,
                            mut payload,
                            fin,
                        } => {
                            let Some(payload_end_offset) =
                                offset.as_u64().checked_add(payload.len() as u64)
                            else {
                                debug!(
                                    stream_id = self.stream_id.as_u64(),
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
                                    stream_id = self.stream_id.as_u64(),
                                    offset = offset.as_u64(),
                                    payload_len = payload.len(),
                                    payload_end_offset,
                                    remote_max_data = self.remote_max_data.as_u64(),
                                    "Peer exceeded advertised receive window"
                                );
                                return self.queue_control_error();
                            }

                            trace!(
                                stream_id = self.stream_id.as_u64(),
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
                                        stream_id = self.stream_id.as_u64(),
                                        ?err,
                                        "Invalid storage/fin combination"
                                    );
                                    return self.protocol_error();
                                }
                            };

                            if let Err(err) = write_data_reader(
                                &mut self.reassembler,
                                &mut reader,
                                app_buf,
                                interpose,
                            ) {
                                debug!(
                                    stream_id = self.stream_id.as_u64(),
                                    ?err,
                                    "Failed to write to reassembler"
                                );
                                return self.protocol_error();
                            }
                        }
                        msg::Stream::QueueValidated => {
                            if self.status.on_validated().is_ok() {
                                debug!(stream_id = self.stream_id.as_u64(), "Flow validated");
                            } else {
                                debug!(
                                    stream_id = self.stream_id.as_u64(),
                                    "QueueValidated received in unexpected state"
                                );
                            }
                        }
                        msg::Stream::Reset { error_code } => {
                            debug!(
                                stream_id = self.stream_id.as_u64(),
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
            Poll::Ready(Err(_)) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "stream channel closed",
            ))),
            Poll::Pending => Poll::Pending,
        }
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

    fn maybe_send_max_data(&mut self) -> io::Result<()> {
        if let Some(final_size) = self.reassembler.final_size() {
            if !self.send_flow_update_after_fin {
                return Ok(());
            }

            if self.remote_max_data.as_u64() >= final_size {
                return Ok(());
            }
        }

        let consumed = self.reassembler.consumed_len();
        let current_max = self.remote_max_data.as_u64();
        let threshold = current_max.saturating_sub(self.window_size / 2);

        if consumed >= threshold {
            let new_max_data = consumed + self.window_size;
            let new_max_data = VarInt::new(new_max_data)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "max_data overflow"))?;

            // Frame send errors are propagated: if we cannot communicate flow
            // control credits the peer may stall, so it is better to surface
            // the failure immediately.
            self.send_max_data_frame(new_max_data)?;
            self.remote_max_data = new_max_data;
        } else {
            trace!(
                stream_id = self.stream_id.as_u64(),
                consumed,
                current_max,
                threshold,
                window_size = self.window_size,
                "maybe_send_max_data: below threshold, not sending"
            );
        }

        Ok(())
    }

    fn poll_completions(&mut self, cx: &mut Context) -> io::Result<()> {
        match self.completion_rx.poll_swap(cx) {
            Poll::Ready(Some(queue)) => {
                let mut failure = None;

                for completed in queue.iter() {
                    if let frame::TransmissionStatus::Failed(reason) = completed.status {
                        if let Some(existing) = failure {
                            debug!(
                                stream_id = self.stream_id.as_u64(),
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
        let Some(remote_queue_id) = self.stream_rx.remote_queue_id() else {
            return Ok(());
        };

        let frame = Frame {
            source_sender_id: LocalSenderId::UNSPECIFIED,
            header: Header::QueueMaxData {
                queue_pair: QueuePair {
                    source_queue_id: self.stream_rx.queue_id(),
                    dest_queue_id: remote_queue_id,
                },
                stream_id: self.stream_id,
                maximum_data,
            },
            payload: ByteVec::new(),
            path_secret_entry: self.path_secret_entry.clone(),
            completion: Some(self.completion_rx.sender()),
            status: frame::TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        };

        self.send_frame(frame)?;

        trace!(
            stream_id = self.stream_id.as_u64(),
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
        let Some(remote_queue_id) = self.stream_rx.remote_queue_id() else {
            return Ok(());
        };

        let frame = Frame {
            source_sender_id: LocalSenderId::UNSPECIFIED,
            header: Header::QueueReset {
                dest_queue_id: remote_queue_id,
                stream_id: self.stream_id,
                reset_target,
                error_code,
            },
            payload: ByteVec::new(),
            path_secret_entry: self.path_secret_entry.clone(),
            completion: None,
            status: frame::TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        };

        self.send_frame(frame)?;

        debug!(
            stream_id = self.stream_id.as_u64(),
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
    interpose: bool,
) -> Result<(), buffer::Error<R::Error>>
where
    S: buffer::writer::Storage + ?Sized,
    R: buffer::reader::Reader + ?Sized,
{
    if interpose && reassembler.is_empty() {
        let mut interposer = Interposer::new(app_buf, reassembler);
        interposer.read_from(reader)
    } else {
        reassembler.write_reader(reader)
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        debug!(
            stream_id = self.0.stream_id.as_u64(),
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
                stream_id = self.0.stream_id.as_u64(),
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
                stream_id = self.0.stream_id.as_u64(),
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
