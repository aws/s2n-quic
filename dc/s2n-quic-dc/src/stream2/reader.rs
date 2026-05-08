// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream2 Reader: Reassembly and flow control on top of the reliable datagram pipeline
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
// * Use buffer::duplex::Interposer to bypass the reassembler on the hot path. When a
//   datagram arrives at the head offset and the application buffer has remaining capacity,
//   the Interposer writes directly into the application buffer and calls skip() on the
//   reassembler to advance its cursor. This avoids a copy through the reassembler for the
//   common in-order case. The existing stream implementation does this in recv/shared.rs.
//   To integrate: poll_stream_rx needs access to the application buffer, and the write path
//   should use `Interposer::new(app_buf, &mut self.reassembler)` as the Writer target for
//   write_reader. Out-of-order data still goes into the reassembler as usual.
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
    datagram::batch::Batch,
    flow,
    packet::datagram::{partial::PartialDatagram, QueuePair, RoutingInfo},
    path::secret::map::Entry as PathSecretEntry,
    socket::channel,
    stream2::endpoint::{reset_error::ResetError, StreamMsg},
};
use s2n_quic_core::{
    buffer::{
        self,
        reader::{storage::Infallible, Incremental},
        reassembler::Reassembler,
    },
    state::{event, is},
    task::waker,
    varint::VarInt,
};
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tracing::{debug, trace};

/// Reader for stream2: handles reassembly and flow control
///
/// Boxed to avoid excessive stack usage when passing around in applications
pub struct Reader(Box<Inner>);

struct Inner {
    /// Channel to send batches to the wheel for control messages
    wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
    /// Stream-side channel for receiving data from the pipeline
    stream_rx: flow::queue::Stream<StreamMsg, crate::stream2::endpoint::ControlMsg, flow::Handle>,
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
    /// Current status of the reader
    status: Status,
    /// Reset error code if the stream was reset by the peer
    reset_error_code: Option<VarInt>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Status {
    /// Server-only: awaiting client validation before releasing credits.
    /// Buffers incoming data but blocks reads until validation completes.
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
        /// Transition to Open when validation completes
        on_validated(PendingValidation => Open);
        /// Transition to Reset when reset received
        on_reset(PendingValidation | Open => Reset);
        /// Transition to Complete when all data consumed
        on_complete(Open => Complete);
    }
}

impl Reader {
    /// Create a new Reader for a client connection
    pub(crate) fn new_client(
        wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        stream_rx: flow::queue::Stream<
            StreamMsg,
            crate::stream2::endpoint::ControlMsg,
            flow::Handle,
        >,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let remote_max_data = parameters.local_recv_max_data;
        let window_size = remote_max_data.as_u64();

        Self(Box::new(Inner {
            wheel_tx,
            stream_rx,
            path_secret_entry,
            stream_id,
            reassembler: Reassembler::new(),
            remote_max_data,
            window_size,
            status: Status::Open,
            reset_error_code: None,
        }))
    }

    /// Create a new Reader for a validated server connection
    pub(crate) fn new_server(
        wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        stream_rx: flow::queue::Stream<
            StreamMsg,
            crate::stream2::endpoint::ControlMsg,
            flow::Handle,
        >,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let window_size = parameters.local_recv_max_data.as_u64();

        Self(Box::new(Inner {
            wheel_tx,
            stream_rx,
            path_secret_entry,
            stream_id,
            reassembler: Reassembler::new(),
            remote_max_data: VarInt::ZERO,
            window_size,
            status: Status::Open,
            reset_error_code: None,
        }))
    }

    /// Create a new Reader for a server connection pending validation
    ///
    /// The stream buffers incoming data but blocks reads until `validate()` completes.
    pub(crate) fn new_server_pending(
        wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        stream_rx: flow::queue::Stream<
            StreamMsg,
            crate::stream2::endpoint::ControlMsg,
            flow::Handle,
        >,
    ) -> Self {
        let parameters = path_secret_entry.parameters();
        let window_size = parameters.local_recv_max_data.as_u64();

        Self(Box::new(Inner {
            wheel_tx,
            stream_rx,
            path_secret_entry,
            stream_id,
            reassembler: Reassembler::new(),
            remote_max_data: VarInt::ZERO,
            window_size,
            status: Status::PendingValidation,
            reset_error_code: None,
        }))
    }

    /// Wait for the stream to be validated by the client
    ///
    /// For streams that were already validated (confirmed non-duplicate), this is a no-op.
    /// For pending streams, this polls for the FlowValidated message from the pipeline.
    /// The application should wrap this in its own timeout.
    pub async fn validate(&mut self) -> io::Result<()> {
        core::future::poll_fn(|cx| self.0.poll_validate(cx)).await
    }

    /// Send a FlowReset to the peer for both halves and transition to terminal state.
    ///
    /// No-op if already in a terminal state. After this call, the Reader's Drop will not
    /// send anything additional.
    pub(crate) fn send_reset(&mut self, error_code: VarInt) {
        if self.0.status.is_terminal() {
            return;
        }
        let _ = self
            .0
            .send_reset_packet(error_code, crate::packet::datagram::ResetTarget::Both);
        self.0.status.on_reset().ok();
    }

    /// Read data into a buffer
    ///
    /// Returns the number of bytes read. Returns 0 if no data is available or EOF reached.
    pub async fn read_into<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::writer::Storage,
    {
        core::future::poll_fn(|cx| self.poll_read_into(cx, buf)).await
    }

    /// Poll-based read
    pub fn poll_read_into<S>(&mut self, cx: &mut Context, buf: &mut S) -> Poll<io::Result<usize>>
    where
        S: buffer::writer::Storage,
    {
        waker::debug_assert_contract(cx, |cx| self.0.poll_read_into(cx, buf))
    }
}

impl Inner {
    fn poll_validate(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        if !self.status.is_pending_validation() {
            return Poll::Ready(Ok(()));
        }

        // Poll for FlowValidated (or Reset/error)
        match self.poll_stream_rx(cx)? {
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

    fn poll_read_into<S>(&mut self, cx: &mut Context, buf: &mut S) -> Poll<io::Result<usize>>
    where
        S: buffer::writer::Storage,
    {
        // Must validate before reading
        if self.status.is_pending_validation() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "stream not yet validated - call validate() first",
            )));
        }

        // Check if we're in a terminal state
        if self.status.is_reset() {
            if let Some(error_code) = self.reset_error_code {
                let reset_error: ResetError = error_code.into();
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    reset_error,
                )));
            }
            return Poll::Ready(Err(io::ErrorKind::ConnectionReset.into()));
        }

        if self.status.is_complete() {
            return Poll::Ready(Ok(0));
        }

        // Process incoming messages to fill the reassembler
        let _ = self.poll_stream_rx(cx)?;

        // Copy from reassembler into destination buffer
        let bytes_read = if buf.remaining_capacity() > 0 {
            let mut tracker = buf.track_write();
            self.reassembler.infallible_copy_into(&mut tracker);
            tracker.written_len()
        } else {
            0
        };

        self.maybe_send_max_data()?;

        if self.reassembler.is_reading_complete() {
            self.status.on_complete().ok();
            return Poll::Ready(Ok(bytes_read));
        }

        if bytes_read > 0 {
            Poll::Ready(Ok(bytes_read))
        } else {
            Poll::Pending
        }
    }

    /// Poll the stream_rx channel for incoming messages
    ///
    /// NOTE: `poll_swap` both drains the queue and registers the waker in one call,
    /// so there's no need to loop. If the queue is empty, the waker is registered
    /// and we return Pending.
    fn poll_stream_rx(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        match self.stream_rx.poll_swap(cx) {
            Poll::Ready(Ok(queue)) => {
                // Process all messages in the queue
                for msg in queue {
                    match msg.into_inner() {
                        StreamMsg::Data {
                            offset,
                            mut payload,
                            fin,
                        } => {
                            trace!(
                                stream_id = self.stream_id.as_u64(),
                                offset = offset.as_u64(),
                                len = payload.len(),
                                is_fin = fin,
                                "Received data"
                            );

                            // Create an incremental reader from the BytesMut payload
                            let mut incremental = Incremental::new(offset);
                            let mut reader = match incremental.with_storage(&mut payload, fin) {
                                Ok(r) => r,
                                Err(err) => {
                                    debug!(
                                        stream_id = self.stream_id.as_u64(),
                                        ?err,
                                        "Invalid storage/fin combination"
                                    );
                                    let error_code =
                                        crate::stream2::endpoint::reset_error::FRAME_DECODE_ERROR;
                                    self.reset_error_code = Some(error_code);
                                    self.status.on_reset().ok();
                                    let _ = self.send_reset_packet(
                                        error_code,
                                        crate::packet::datagram::ResetTarget::Both,
                                    );
                                    let reset_error: ResetError = error_code.into();
                                    return Poll::Ready(Err(io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        reset_error,
                                    )));
                                }
                            };

                            // Write into the reassembler
                            if let Err(err) = self.reassembler.write_reader(&mut reader) {
                                debug!(
                                    stream_id = self.stream_id.as_u64(),
                                    ?err,
                                    "Failed to write to reassembler"
                                );
                                // Protocol error - send reset
                                let error_code =
                                    crate::stream2::endpoint::reset_error::FRAME_DECODE_ERROR;
                                self.reset_error_code = Some(error_code);
                                self.status.on_reset().ok();
                                let _ = self.send_reset_packet(
                                    error_code,
                                    crate::packet::datagram::ResetTarget::Both,
                                );
                                let reset_error: ResetError = error_code.into();
                                return Poll::Ready(Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    reset_error,
                                )));
                            }
                        }
                        StreamMsg::FlowValidated => {
                            if self.status.on_validated().is_ok() {
                                debug!(stream_id = self.stream_id.as_u64(), "Flow validated");
                            } else {
                                debug!(
                                    stream_id = self.stream_id.as_u64(),
                                    "FlowValidated received in unexpected state"
                                );
                            }
                        }
                        StreamMsg::Reset { error_code } => {
                            debug!(
                                stream_id = self.stream_id.as_u64(),
                                error_code = error_code.as_u64(),
                                "Stream reset by peer"
                            );
                            self.reset_error_code = Some(error_code);
                            self.status.on_reset().ok();
                            let reset_error: ResetError = error_code.into();
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
                io::ErrorKind::BrokenPipe,
                "stream channel closed",
            ))),
            Poll::Pending => Poll::Pending,
        }
    }

    /// Check if we should send a MAX_DATA update to the peer
    ///
    /// TODO: Consider using completion notifications for MAX_DATA frames to provide
    /// backpressure and limit the number of inflight updates. This would also allow
    /// waking the application when it's time to send a new frame.
    fn maybe_send_max_data(&mut self) -> io::Result<()> {
        // If the peer has already sent a FIN and we've advertised enough window to
        // cover the final offset, there is no point sending further MAX_DATA updates —
        // the sender will never need additional credit.
        if let Some(final_size) = self.reassembler.final_size() {
            if self.remote_max_data.as_u64() >= final_size {
                return Ok(());
            }
        }

        // Calculate how much budget has been consumed from the reassembler
        let consumed = self.reassembler.consumed_len();
        let current_max = self.remote_max_data.as_u64();

        // Send MAX_DATA when we've consumed roughly half the window
        let threshold = current_max.saturating_sub(self.window_size / 2);

        if consumed >= threshold {
            // Calculate new MAX_DATA value
            let new_max_data = consumed + self.window_size;
            let new_max_data = VarInt::new(new_max_data)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "max_data overflow"))?;

            self.send_max_data(new_max_data)?;
            self.remote_max_data = new_max_data;
        } else {
            tracing::trace!(
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

    /// Send a MAX_DATA frame to the peer
    fn send_max_data(&mut self, maximum_data: VarInt) -> io::Result<()> {
        use s2n_codec::EncoderValue;
        use s2n_quic_core::frame::MaxData;

        let Some(remote_queue_id) = self.stream_rx.remote_queue_id() else {
            // Can't send MAX_DATA before we know the peer's queue ID
            return Ok(());
        };

        let data_addr = self.path_secret_entry.data_addr();
        let mut builder = crate::datagram::batch::Builder::new(None, data_addr);

        // Encode MAX_DATA frame
        let frame = MaxData { maximum_data };
        let encoded_bytes = frame.encode_to_vec();
        let control_data = ByteVec::from(encoded_bytes);

        let flow_control = PartialDatagram::new_datagram(
            RoutingInfo::FlowControl {
                source_sender_id: VarInt::MAX,
                queue_pair: QueuePair {
                    source_queue_id: self.stream_rx.queue_id(),
                    dest_queue_id: remote_queue_id,
                },
                stream_id: self.stream_id,
            },
            control_data,
            ByteVec::new(),
            self.path_secret_entry.clone(),
            None,
        );

        builder
            .try_push(flow_control.into())
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "batch full"))?;

        let batch = builder.finish();
        self.wheel_tx
            .send_entry(batch.into())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "wheel channel closed"))?;

        trace!(
            stream_id = self.stream_id.as_u64(),
            maximum_data = maximum_data.as_u64(),
            "Sent MAX_DATA"
        );

        Ok(())
    }

    /// Send a reset packet to the peer
    fn send_reset_packet(
        &mut self,
        error_code: VarInt,
        reset_target: crate::packet::datagram::ResetTarget,
    ) -> io::Result<()> {
        let Some(remote_queue_id) = self.stream_rx.remote_queue_id() else {
            // Can't send reset before we know the peer's queue ID
            return Ok(());
        };

        let data_addr = self.path_secret_entry.data_addr();
        let mut builder = crate::datagram::batch::Builder::new(None, data_addr);

        let reset_packet = PartialDatagram::new_datagram(
            RoutingInfo::FlowReset {
                source_sender_id: VarInt::MAX,
                dest_queue_id: remote_queue_id,
                stream_id: self.stream_id,
                reset_target,
                error_code,
            },
            ByteVec::new(),
            ByteVec::new(),
            self.path_secret_entry.clone(),
            None,
        );

        builder
            .try_push(reset_packet.into())
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "batch full"))?;

        let batch = builder.finish();
        self.wheel_tx
            .send_entry(batch.into())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "wheel channel closed"))?;

        debug!(
            stream_id = self.stream_id.as_u64(),
            error_code = error_code.as_u64(),
            reset_target = ?reset_target,
            "Sent FlowReset"
        );

        Ok(())
    }
}

impl Drop for Reader {
    fn drop(&mut self) {
        // If we're panicking, send FlowReset with ABNORMAL_TERMINATION to reset both halves
        if std::thread::panicking() {
            let error_code = crate::stream2::endpoint::reset_error::ABNORMAL_TERMINATION;
            let _ = self
                .0
                .send_reset_packet(error_code, crate::packet::datagram::ResetTarget::Both);
            debug!(
                stream_id = self.0.stream_id.as_u64(),
                "Reader dropped during panic - sent FlowReset"
            );
        } else {
            // Normal drop - only send STOP_SENDING if we haven't received all data yet
            // If is_writing_complete() is true, the sender has sent FIN and likely gone away
            // This resets only the Stream half (peer's sender)
            if !self.0.reassembler.is_writing_complete() && !self.0.status.is_reset() {
                let error_code = crate::stream2::endpoint::reset_error::STOP_SENDING;
                let _ = self
                    .0
                    .send_reset_packet(error_code, crate::packet::datagram::ResetTarget::Stream);
                debug!(
                    stream_id = self.0.stream_id.as_u64(),
                    "Reader dropped before FIN received - sent STOP_SENDING"
                );
            }
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
        // Use BufMut wrapper to avoid initializing the unfilled portion
        let mut buf = buffer::writer::storage::BufMut::new(buf);
        s2n_quic_core::ready!(self.poll_read_into(cx, &mut buf))?;
        Poll::Ready(Ok(()))
    }
}
