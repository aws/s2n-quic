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
    io,
    net::SocketAddr,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

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
pub struct Writer(Box<Inner>);

struct Inner {
    /// Channel to submit frames to the wheel
    frame_tx: SubmissionSender,
    /// Receiver for completion notifications from the pipeline
    completion_rx: frame::CompletionReceiver,
    /// Control-side channel for receiving MAX_DATA frames
    control_rx: crate::queue::ControlReceiver,
    /// Path secret entry providing MTU and crypto material
    path_secret_entry: Arc<PathSecretEntry>,
    /// Cached packet size (MTU minus header overhead) for fragmentation
    packet_size: u16,
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
    ) -> Self {
        let completion_rx = frame::completion_channel();
        let parameters = path_secret_entry.parameters();
        let mtu = parameters.max_datagram_size();
        let packet_size = mtu.saturating_sub(MAX_QUEUE_DATA_HEADER_OVERHEAD);
        let max_inflight_bytes = parameters.local_send_max_data.as_u64();
        let remote_max_data = VarInt::ZERO;

        Self(Box::new(Inner {
            frame_tx,
            completion_rx,
            control_rx,
            path_secret_entry,
            packet_size,
            dest_queue_id,
            acceptor_id,
            next_offset: VarInt::ZERO,
            inflight_bytes: 0,
            max_inflight_bytes,
            remote_max_data,
            status: Status::Init,
            reset_error_code: None,
            coop: Coop::default(),
            clock,
            metrics,
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
    ) -> Self {
        let completion_rx = frame::completion_channel();
        let parameters = path_secret_entry.parameters();
        let mtu = parameters.max_datagram_size();
        let packet_size = mtu.saturating_sub(MAX_QUEUE_DATA_HEADER_OVERHEAD);
        let max_inflight_bytes = parameters.local_send_max_data.as_u64();
        let initial_remote_max_data = parameters.remote_max_data;

        Self(Box::new(Inner {
            frame_tx,
            completion_rx,
            control_rx,
            path_secret_entry,
            packet_size,
            dest_queue_id,
            acceptor_id,
            next_offset: VarInt::ZERO,
            inflight_bytes: 0,
            max_inflight_bytes,
            remote_max_data: initial_remote_max_data,
            status: Status::Open,
            reset_error_code: None,
            coop: Coop::default(),
            clock,
            metrics,
        }))
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
        self.0.poll_write_from(cx, buf, is_fin)
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
        buf: &mut S,
        is_fin: bool,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::reader::storage::Infallible,
    {
        waker::debug_assert_contract(cx, |cx| {
            coop::poll(self, cx, |this, cx| {
                this.poll_write_from_inner(cx, buf, is_fin)
            })
        })
    }

    #[inline(always)]
    fn poll_write_from_inner<S>(
        &mut self,
        cx: &mut Context,
        buf: &mut S,
        is_fin: bool,
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

        self.send_fin_packet()?;
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
            },
            payload: ByteVec::new(),
            path_secret_entry: self.path_secret_entry.clone(),
            completion: Some(self.completion_rx.sender()),
            status: frame::TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            enqueued_at: Some(self.clock.now()),
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
                        self.metrics.sojourn.record(enqueued_at, completed_at, failure_reason);
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

        let frame = Frame {
            header: Header::QueueData {
                queue_pair: self.queue_pair(),
                binding_id: self.control_rx.binding_id(),
                offset: VarInt::ZERO,
                is_fin: actual_fin,
                dest_acceptor_id: Some(self.acceptor_id),
            },
            payload,
            path_secret_entry: self.path_secret_entry.clone(),
            completion: Some(self.completion_rx.sender()),
            status: frame::TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            enqueued_at: Some(self.clock.now()),
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

    fn min_send_budget(&self) -> u64 {
        let local_available = self.max_inflight_bytes.saturating_sub(self.inflight_bytes);
        let remote_available = self
            .remote_max_data
            .as_u64()
            .saturating_sub(self.next_offset.as_u64());

        local_available.min(remote_available)
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

            let frame = Frame {
                header: Header::QueueData {
                    queue_pair: self.queue_pair(),
                    binding_id: self.control_rx.binding_id(),
                    offset,
                    is_fin: include_fin,
                    dest_acceptor_id: self.dest_acceptor_id(),
                },
                payload,
                path_secret_entry: self.path_secret_entry.clone(),
                completion: Some(self.completion_rx.sender()),
                status: frame::TransmissionStatus::default(),
                ttl: DEFAULT_TTL,
                enqueued_at: batch_enqueued_at,
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
        self.frame_tx
            .send_batch(HomogeneousBatch {
                queue,
                priority: Priority::QueueData,
            })
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
