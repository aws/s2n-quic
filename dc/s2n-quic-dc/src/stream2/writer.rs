// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream2 Writer: Fragmentation and flow control on top of the reliable datagram pipeline
//!
//! The Writer breaks application data into MTU-sized datagrams and submits them to the pipeline.
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
//!    transmission of queued datagrams. Completion notifications are silently dropped since the
//!    application no longer cares. This allows the application to drop the Writer immediately
//!    after calling shutdown() without blocking transmission.
//!
//! 2. **Abnormal (panic) closure**: When the Writer is dropped during a panic, both
//!    `should_transmit` and `receiver_alive` flags are cleared, and a FlowReset with
//!    ABNORMAL_TERMINATION is sent to the peer. The pipeline will cancel all pending
//!    transmissions and not attempt to send them. This ensures the peer is notified when
//!    the sender crashes.
//!
//! The Drop implementation checks `std::thread::panicking()` to distinguish between these cases.

// TODOs:
//
// Correctness:
//
// * send_data does not transition status to FinSent when it sends the final datagram with
//   is_fin=true. This means a subsequent write won't get BrokenPipe, and the Drop impl will
//   try to send FIN again (double-FIN). poll_write_from needs to call on_send_fin() after
//   send_data returns when the FIN was actually sent.
//
// * VarInt overflow in next_offset: adding payload_len to next_offset could overflow VarInt
//   (max 2^62-1) on extremely large streams. Should return an error instead of panicking.
//
// * poll_completions processes failures with "last one wins" semantics — if a batch has
//   both Acknowledged and Failed completions, only the last failure is reported. First
//   failure should take precedence since it represents the earliest delivery problem.
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
// * Pace out batch transmissions at 1us interval — right now we're passing `None` to the
//   batch builder. We also need to remember the last transmission time so we don't go
//   backward if we do another burst.
//
// * GSO segment count check (`current_builder.len() >= max_segments`) may not match
//   try_push's actual capacity logic. Consider unifying these checks or just relying on
//   the try_push fallback.
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
//   early data with FlowInit, completion failure handling, panic-drop behavior, and
//   multi-stream contention on shared pipeline resources.

use crate::{
    byte_vec::ByteVec,
    datagram::batch::Batch,
    flow,
    intrusive_queue::List,
    packet::{
        control,
        datagram::{
            partial::{PartialDatagram, MAX_FLOW_DATA_HEADER_OVERHEAD},
            QueuePair, RoutingInfo,
        },
    },
    path::secret::map::Entry as PathSecretEntry,
    socket::channel,
    stream2::endpoint::{reset_error::ResetError, ControlMsg},
};
use s2n_quic_core::{
    buffer::{self, writer::Storage},
    state::{event, is},
    task::waker,
    varint::VarInt,
};
use s2n_quic_platform::features;
use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tracing::{debug, trace};

/// Writer for stream2: handles fragmentation and flow control
///
/// Boxed to avoid excessive stack usage when passing around in applications
pub struct Writer(Box<Inner>);

struct Inner {
    /// Channel to send batches to the wheel
    wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
    /// Receiver for completion notifications from the pipeline
    completion_rx: channel::intrusive_queue::datagram_completion::Receiver<PartialDatagram>,
    /// Control-side channel for receiving MAX_DATA frames
    control_rx: flow::queue::Control<
        crate::stream2::endpoint::StreamMsg,
        crate::stream2::endpoint::ControlMsg,
        flow::Handle,
    >,
    /// Path secret entry providing MTU and crypto material
    path_secret_entry: Arc<PathSecretEntry>,
    /// Cached packet size (MTU) for fragmentation
    packet_size: u16,
    /// GSO configuration for batching
    gso: features::Gso,
    /// Stream identifier
    stream_id: VarInt,
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
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Status {
    /// Initial state before sending FlowInit
    #[default]
    Init,
    /// FlowInit sent, waiting for acknowledgment
    FlowInitSent,
    /// Flow established and open for writes
    Open,
    /// FIN sent
    FinSent,
    /// Shutdown completed
    Shutdown,
}

impl Status {
    is!(is_init, Init);
    is!(is_flow_init_sent, FlowInitSent);
    is!(is_open, Open);
    is!(is_fin_sent, FinSent);
    is!(is_shutdown, Shutdown);
    is!(is_terminal, FinSent | Shutdown);

    event! {
        /// Transition from Init to FlowInitSent
        on_send_flow_init(Init => FlowInitSent);
        /// Transition from FlowInitSent to Open when flow is established
        on_flow_established(FlowInitSent => Open);
        /// Transition from Open to FinSent when FIN is sent
        on_send_fin(FlowInitSent | Open => FinSent);
        /// Transition to Shutdown from any non-terminal state
        on_shutdown(Init | FlowInitSent | Open | FinSent => Shutdown);
    }
}

impl Writer {
    /// Create a new Writer for a client connection
    ///
    /// The Writer is returned immediately without sending FlowInit - that happens lazily on
    /// the first write with optional early data.
    pub(crate) fn new_client(
        wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
        path_secret_entry: Arc<PathSecretEntry>,
        gso: features::Gso,
        stream_id: VarInt,
        acceptor_id: VarInt,
        control_rx: flow::queue::Control<
            crate::stream2::endpoint::StreamMsg,
            crate::stream2::endpoint::ControlMsg,
            flow::Handle,
        >,
    ) -> Self {
        let completion_rx = channel::intrusive_queue::datagram_completion::new();
        let parameters = path_secret_entry.parameters();
        let mtu = parameters.max_datagram_size();
        let packet_size = mtu.saturating_sub(MAX_FLOW_DATA_HEADER_OVERHEAD);
        let max_inflight_bytes = parameters.local_send_max_data.as_u64();
        // Wait for the FlowInit to be accepted by the server
        let remote_max_data = VarInt::ZERO;

        Self(Box::new(Inner {
            wheel_tx,
            completion_rx,
            control_rx,
            path_secret_entry,
            packet_size,
            gso,
            stream_id,
            acceptor_id,
            next_offset: VarInt::ZERO,
            inflight_bytes: 0,
            max_inflight_bytes,
            remote_max_data,
            status: Status::Init,
            reset_error_code: None,
        }))
    }

    /// Create a new Writer for a server connection
    ///
    /// Server-side Writers start in the Open status since flow is already established by
    /// the time the application receives the stream.
    pub(crate) fn new_server(
        wheel_tx: channel::intrusive_queue::sync::Sender<Batch>,
        path_secret_entry: Arc<PathSecretEntry>,
        gso: features::Gso,
        stream_id: VarInt,
        control_rx: flow::queue::Control<
            crate::stream2::endpoint::StreamMsg,
            crate::stream2::endpoint::ControlMsg,
            flow::Handle,
        >,
    ) -> Self {
        let completion_rx = channel::intrusive_queue::datagram_completion::new();
        let parameters = path_secret_entry.parameters();
        let mtu = parameters.max_datagram_size();
        let packet_size = mtu.saturating_sub(MAX_FLOW_DATA_HEADER_OVERHEAD);
        let max_inflight_bytes = parameters.local_send_max_data.as_u64();
        let initial_remote_max_data = parameters.remote_max_data;

        Self(Box::new(Inner {
            wheel_tx,
            completion_rx,
            control_rx,
            path_secret_entry,
            packet_size,
            gso,
            stream_id,
            acceptor_id: VarInt::ZERO, // Not used on server side
            next_offset: VarInt::ZERO,
            inflight_bytes: 0,
            max_inflight_bytes,
            remote_max_data: initial_remote_max_data,
            status: Status::Open,
            reset_error_code: None,
        }))
    }

    /// Write data from a buffer
    ///
    /// Returns the number of bytes written. May return less than the full buffer if flow
    /// control blocks.
    pub async fn write_from<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        core::future::poll_fn(|cx| self.poll_write_from(cx, buf, false)).await
    }

    /// Write all data from a buffer
    ///
    /// Blocks until the entire buffer has been written.
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

    /// Write data from a buffer and send FIN
    pub async fn write_from_fin<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        core::future::poll_fn(|cx| self.poll_write_from(cx, buf, true)).await
    }

    /// Write all data from a buffer and send FIN
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

    /// Poll-based write with optional FIN
    pub fn poll_write_from<S>(
        &mut self,
        cx: &mut Context,
        buf: &mut S,
        is_fin: bool,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::reader::storage::Infallible,
    {
        waker::debug_assert_contract(cx, |cx| self.0.poll_write_from(cx, buf, is_fin))
    }

    /// Gracefully shutdown the writer
    ///
    /// This sends FIN if it hasn't been sent yet.
    pub fn shutdown(&mut self) -> io::Result<()> {
        self.0.shutdown()
    }

    /// Force the writer into its terminal state without sending anything on the wire.
    ///
    /// Used when a single reset has already been sent for both halves (e.g., accept queue
    /// overflow). The Drop impl becomes a no-op after this.
    pub(crate) fn force_shutdown(&mut self) {
        self.0.completion_rx.cancel();
        self.0.status.on_shutdown().ok();
    }
}

impl Inner {
    fn poll_write_from<S>(
        &mut self,
        cx: &mut Context,
        buf: &mut S,
        is_fin: bool,
    ) -> Poll<io::Result<usize>>
    where
        S: buffer::reader::storage::Infallible,
    {
        // Check if we're already shut down or have sent FIN
        if self.status.is_shutdown() {
            if let Some(error_code) = self.reset_error_code {
                let reset_error: ResetError = error_code.into();
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

        // Always poll for completions to free up local budget
        self.poll_completions(cx)?;

        // Always poll for remote budget to process pending control messages
        let _ = self.poll_remote_budget(cx)?;

        // Handle flow establishment with early data
        if self.status.is_init() {
            let (written, is_fin) = self.send_flow_init_with_early_data(buf, is_fin)?;

            if written > 0 || is_fin {
                return Poll::Ready(Ok(written));
            }

            // FlowInit sent but no data/fin, need to wait for establishment
            return Poll::Pending;
        }

        // Wait for flow establishment if still pending
        if self.status.is_flow_init_sent() {
            tracing::trace!(
                stream_id = self.stream_id.as_u64(),
                "Writer blocked in FlowInitSent - waiting for remote MAX_DATA"
            );
            return Poll::Pending; // poll_remote_budget will transition to Open
        }

        // Check if we have enough budget to send
        let available = self.min_send_budget();
        if available == 0 && !is_fin {
            // No budget and not sending FIN, must block
            return Poll::Pending;
        }

        // Send data (or empty FIN)
        let written = self.send_data(buf, is_fin)?;

        Poll::Ready(Ok(written))
    }

    fn shutdown(&mut self) -> io::Result<()> {
        // Guard: already shut down
        if self.status.is_shutdown() {
            return Ok(());
        }

        // Guard: can't shutdown if we've already sent FIN (would be double-fin)
        if self.status.is_fin_sent() {
            self.status.on_shutdown().unwrap();
            return Ok(());
        }

        // Force send FIN
        self.send_fin_packet()?;

        // Transition to shutdown
        self.status.on_shutdown().unwrap();

        Ok(())
    }

    /// Send a reset packet to the peer
    fn send_reset_packet(
        &mut self,
        error_code: VarInt,
        reset_target: crate::packet::datagram::ResetTarget,
    ) -> io::Result<()> {
        let Some(remote_queue_id) = self.control_rx.remote_queue_id() else {
            // Can't send reset before flow is established
            debug!(
                stream_id = self.stream_id.as_u64(),
                "Cannot send reset before flow established"
            );
            return Ok(());
        };

        let data_addr = self.path_secret_entry.data_addr();
        let mut builder = crate::datagram::batch::Builder::new(None, data_addr);

        // Create FlowReset datagram with empty header and payload
        let reset_packet = PartialDatagram::new_datagram(
            RoutingInfo::FlowReset {
                source_sender_id: VarInt::MAX,
                dest_queue_id: remote_queue_id,
                stream_id: self.stream_id,
                reset_target,
                error_code,
            },
            ByteVec::new(), // Empty header
            ByteVec::new(), // Empty payload
            self.path_secret_entry.clone(),
            None, // No completion notification needed for reset
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

    /// Send a standalone FIN packet
    fn send_fin_packet(&mut self) -> io::Result<()> {
        if self.status.is_init() {
            // Haven't sent FlowInit yet - send it with FIN
            let (_written, _is_fin) =
                self.send_flow_init_with_early_data(&mut buffer::reader::storage::Empty, true)?;

            // Note that we sent the FIN
            self.status.on_send_fin().unwrap();
        } else if self.status.is_open() {
            // Send empty FlowData with FIN
            let data_addr = self.path_secret_entry.data_addr();
            let mut builder = crate::datagram::batch::Builder::new(None, data_addr);

            let queue_pair = QueuePair {
                source_queue_id: self.control_rx.queue_id(),
                dest_queue_id: self
                    .control_rx
                    .remote_queue_id()
                    .expect("remote_queue_id must be set when Open"),
            };

            let datagram = PartialDatagram::new_datagram(
                RoutingInfo::FlowData {
                    source_sender_id: VarInt::MAX,
                    queue_pair,
                    stream_id: self.stream_id,
                    offset: self.next_offset,
                    is_fin: true,
                },
                ByteVec::new(),
                ByteVec::new(),
                self.path_secret_entry.clone(),
                Some(self.completion_rx.sender()),
            );

            builder
                .try_push(datagram.into())
                .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "batch full"))?;

            let batch = builder.finish();
            self.wheel_tx
                .send_entry(batch.into())
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "wheel channel closed"))?;

            debug!(stream_id = self.stream_id.as_u64(), "Sent FIN");
            self.status.on_send_fin().unwrap();
        }

        Ok(())
    }

    /// Poll the completion queue to free up local budget
    ///
    /// Uses poll_swap to avoid looping and ensures waker registration.
    ///
    /// IMPORTANT: Completions can indicate success (Acknowledged) or failure. Failures include:
    /// - TransmissionError: packet was lost and retransmission limit reached
    /// - PeerDead: peer declared dead (idle timeout)
    /// - UnknownPathSecret: path secret rejected by peer
    /// - Cancelled: sender was dropped
    ///
    /// Any failure means we must abandon the stream since we can't guarantee delivery.
    fn poll_completions(&mut self, cx: &mut Context) -> io::Result<()> {
        use crate::packet::datagram::partial::{FailureReason, PacketType, TransmissionStatus};

        // NOTE: `poll_swap` both drains and registers the waker in one go so no need to loop
        match self.completion_rx.poll_swap(cx) {
            Poll::Ready(Some(queue)) => {
                // Process all completions in the queue
                let mut freed_bytes = 0u64;
                let mut failure = None;

                for datagram in queue.iter() {
                    // Check transmission status
                    match datagram.status {
                        TransmissionStatus::Acknowledged => {
                            // Success - free up budget
                            if let PacketType::Datagram { payload, .. } = &datagram.packet_type {
                                freed_bytes += payload.len() as u64;
                            }
                        }
                        TransmissionStatus::Failed(reason) => {
                            // Failure - we need to abandon the stream
                            failure = Some(reason);

                            // Still free up budget since this data won't be retransmitted
                            if let PacketType::Datagram { payload, .. } = &datagram.packet_type {
                                freed_bytes += payload.len() as u64;
                            }

                            debug!(
                                stream_id = self.stream_id.as_u64(),
                                ?reason,
                                "Transmission failed"
                            );
                        }
                        TransmissionStatus::Pending => {
                            // This shouldn't happen - completions should be Acknowledged or Failed
                            tracing::warn!(
                                stream_id = self.stream_id.as_u64(),
                                "Received completion with Pending status"
                            );
                        }
                    }
                }

                self.inflight_bytes = self.inflight_bytes.saturating_sub(freed_bytes);

                trace!(
                    stream_id = self.stream_id.as_u64(),
                    freed_bytes,
                    inflight_bytes = self.inflight_bytes,
                    "Completions received"
                );

                // If any transmission failed, we need to return an error
                if let Some(reason) = failure {
                    return match reason {
                        FailureReason::UnknownPathSecret => {
                            // Don't send reset for UnknownPathSecret - peer doesn't know us
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
                            // Attempt to send reset to peer before giving up
                            let error_code =
                                crate::stream2::endpoint::reset_error::RETRANSMISSIONS_EXHAUSTED;
                            let _ = self.send_reset_packet(
                                error_code,
                                crate::packet::datagram::ResetTarget::Both,
                            );
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

    /// Poll for remote flow control budget (wait for MAX_DATA)
    ///
    /// Uses poll_swap to avoid looping and ensures waker registration
    fn poll_remote_budget(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        // NOTE: `poll_swap` both drains and registers the waker in one go so no need to loop
        match self.control_rx.poll_swap(cx) {
            Poll::Ready(Ok(queue)) => {
                tracing::debug!(
                    stream_id = self.stream_id.as_u64(),
                    status = ?self.status,
                    msg_count = queue.len(),
                    "poll_remote_budget received messages"
                );
                // Process all control messages in the queue
                for msg in queue {
                    match msg.into_inner() {
                        ControlMsg::Frames { mut payload } => {
                            // Parse control frames - if this fails, send reset to peer
                            if self.handle_control_frames(&mut payload).is_err() {
                                let error_code =
                                    crate::stream2::endpoint::reset_error::FRAME_DECODE_ERROR;
                                self.reset_error_code = Some(error_code);
                                self.status.on_shutdown().ok();

                                // Send reset to peer
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

                            // Transition to Open once we receive the first control message
                            // (the descriptor already has the remote_queue_id from the dispatcher)
                            if self.status.on_flow_established().is_ok() {
                                debug_assert!(self.control_rx.remote_queue_id().is_some());
                                debug!(stream_id = self.stream_id.as_u64(), "Flow established");
                            }
                        }
                        ControlMsg::Reset { error_code } => {
                            // Store the reset error code
                            self.reset_error_code = Some(error_code);
                            // Transition to shutdown
                            self.status.on_shutdown().ok();
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
                io::ErrorKind::ConnectionReset,
                "control channel closed",
            ))),
            Poll::Pending => {
                tracing::trace!(
                    stream_id = self.stream_id.as_u64(),
                    status = ?self.status,
                    "poll_remote_budget pending - no control messages"
                );
                Poll::Pending
            }
        }
    }

    /// Handle control frames from the peer
    fn handle_control_frames(&mut self, payload: &mut [u8]) -> Result<(), s2n_codec::DecoderError> {
        use s2n_quic_core::frame::{FrameMut, MaxData};

        // Create a decoder directly from the BytesMut
        let mut frames_iter = control::decoder::ControlFramesMut::new(payload);

        while let Some(frame) = frames_iter.next() {
            match frame? {
                FrameMut::MaxData(MaxData { maximum_data }) => {
                    let prev_max = self.remote_max_data;
                    self.remote_max_data = self.remote_max_data.max(maximum_data);
                    trace!(
                        stream_id = self.stream_id.as_u64(),
                        prev_max = prev_max.as_u64(),
                        new_max = self.remote_max_data.as_u64(),
                        "Received MAX_DATA"
                    );
                }
                // Ignore other frame types for now
                frame => {
                    trace!(
                        stream_id = self.stream_id.as_u64(),
                        frame = ?frame,
                        "Ignoring control frame"
                    );
                }
            }
        }

        Ok(())
    }

    /// Send FlowInit with optional early data
    ///
    /// Returns (bytes_written, is_fin_sent)
    fn send_flow_init_with_early_data<S>(
        &mut self,
        buf: &mut S,
        is_fin: bool,
    ) -> io::Result<(usize, bool)>
    where
        S: buffer::reader::storage::Infallible,
    {
        let (payload, bytes_read, actual_fin) = self.prepare_early_data(buf, is_fin);

        let data_addr = self.path_secret_entry.data_addr();
        let mut builder = crate::datagram::batch::Builder::new(None, data_addr);

        let flow_init = PartialDatagram::new_datagram(
            RoutingInfo::FlowInit {
                source_sender_id: VarInt::MAX,
                source_queue_id: self.control_rx.queue_id(),
                dest_acceptor_id: self.acceptor_id,
                attempt_id: VarInt::MAX,
                stream_id: self.stream_id,
                is_fin: actual_fin,
            },
            ByteVec::new(),
            payload,
            self.path_secret_entry.clone(),
            Some(self.completion_rx.sender()),
        );

        builder
            .try_push(flow_init.into())
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "batch full"))?;

        let batch = builder.finish();
        self.wheel_tx
            .send_entry(batch.into())
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "wheel channel closed"))?;

        // Transition state
        self.status.on_send_flow_init().unwrap();

        // If we sent the final packet then note as such
        if actual_fin {
            self.status.on_send_fin().unwrap();
        }

        debug!(
            stream_id = self.stream_id.as_u64(),
            bytes_read,
            is_fin = actual_fin,
            "Sent FlowInit with early data"
        );

        Ok((bytes_read, actual_fin))
    }

    /// Prepare early data payload for FlowInit
    ///
    /// Returns (payload, bytes_read, actual_is_fin)
    fn prepare_early_data<S>(&mut self, buf: &mut S, is_fin: bool) -> (ByteVec, usize, bool)
    where
        S: buffer::reader::storage::Infallible,
    {
        // If we're just sending FIN with no data, return early
        if is_fin && buf.buffer_is_empty() {
            return (ByteVec::new(), 0, true);
        }

        // Check if we have data and budget
        if buf.buffer_is_empty() {
            return (ByteVec::new(), 0, false);
        }

        // NOTE: early data bypasses the `min_send_budget` so we can send a single packet

        let mtu = self.packet_size as usize;
        let chunk_len = mtu.min(buf.buffered_len());

        let mut payload = ByteVec::new();
        {
            let mut writer = payload.with_write_limit(chunk_len);
            buf.infallible_copy_into(&mut writer);
        }

        let bytes_read = payload.len();

        // Update state
        self.next_offset += bytes_read;
        self.inflight_bytes += bytes_read as u64;

        // Check if this is the final chunk
        let actual_is_fin = is_fin && buf.buffer_is_empty();

        (payload, bytes_read, actual_is_fin)
    }

    /// Calculate minimum available send budget across all limits
    fn min_send_budget(&self) -> u64 {
        // Local budget: how much more we can have in flight
        let local_available = self.max_inflight_bytes.saturating_sub(self.inflight_bytes);

        // Remote budget: how much more the peer will accept
        let remote_available = self
            .remote_max_data
            .as_u64()
            .saturating_sub(self.next_offset.as_u64());

        local_available.min(remote_available)
    }

    /// Fragment and send data
    ///
    /// Returns the number of bytes sent.
    fn send_data<S>(&mut self, buf: &mut S, is_fin: bool) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        let mtu = self.packet_size as usize;
        let mut written = 0;
        let data_addr = self.path_secret_entry.data_addr();
        let max_segments = self.gso.max_segments();

        // Track batches and segments per batch
        let mut batches = List::new();
        let mut current_builder = crate::datagram::batch::Builder::new(None, data_addr);

        // Special case: if is_fin and buffer is empty, send empty FIN packet
        let mut need_fin_packet = is_fin && buf.buffer_is_empty();

        // Fragment data into MTU-sized chunks while we have budget
        loop {
            // Exit conditions
            if !need_fin_packet && buf.buffer_is_empty() {
                break;
            }

            let available = self.min_send_budget();
            if !need_fin_packet && available == 0 {
                break;
            }

            let chunk_len = if need_fin_packet {
                0 // Empty FIN packet
            } else {
                mtu.min(buf.buffered_len()).min(available as usize)
            };

            // Read chunk from application buffer
            let mut payload = ByteVec::new();
            if chunk_len > 0 {
                let mut writer = payload.with_write_limit(chunk_len);
                buf.infallible_copy_into(&mut writer);
            }

            let payload_len = payload.len();
            let offset = self.next_offset;
            let is_last_chunk = buf.buffer_is_empty();
            let include_fin = is_fin && is_last_chunk;

            // Create FlowData datagram
            let queue_pair = QueuePair {
                source_queue_id: self.control_rx.queue_id(),
                dest_queue_id: self
                    .control_rx
                    .remote_queue_id()
                    .expect("remote_queue_id must be set when Open"),
            };

            let datagram = PartialDatagram::new_datagram(
                RoutingInfo::FlowData {
                    source_sender_id: VarInt::MAX,
                    queue_pair,
                    stream_id: self.stream_id,
                    offset,
                    is_fin: include_fin,
                },
                ByteVec::new(),
                payload,
                self.path_secret_entry.clone(),
                Some(self.completion_rx.sender()),
            );

            // Try to add to current batch, respecting GSO max_segments
            if current_builder.len() >= max_segments {
                // Batch is full, finish it and start a new one
                if !current_builder.is_empty() {
                    batches.push_back(current_builder.finish().into());
                }
                current_builder = crate::datagram::batch::Builder::new(None, data_addr);
            }

            match current_builder.try_push(datagram.into()) {
                Ok(()) => {}
                Err(datagram_entry) => {
                    // Batch builder is full, finish it and start a new one
                    batches.push_back(current_builder.finish().into());
                    current_builder = crate::datagram::batch::Builder::new(None, data_addr);

                    // Try again with new builder
                    current_builder
                        .try_push(datagram_entry)
                        .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "batch full"))?;
                }
            }

            // Update state
            self.next_offset += payload_len;
            self.inflight_bytes += payload_len as u64;
            written += payload_len as usize;

            trace!(
                stream_id = self.stream_id.as_u64(),
                offset = offset.as_u64(),
                payload_len,
                is_fin = include_fin,
                "Sending FlowData"
            );

            // Clear need_fin_packet flag after sending it
            need_fin_packet = false;
        }

        // Push final batch if not empty
        if !current_builder.is_empty() {
            batches.push_back(current_builder.finish().into());
        }

        // Submit all batches
        if !batches.is_empty() {
            self.wheel_tx
                .send_batch(batches)
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "wheel channel closed"))?;
        }

        Ok(written)
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        // If we're panicking, we need to:
        // 1. Send FlowReset to peer so they know we crashed
        // 2. Cancel all pending transmissions so they don't get sent
        if std::thread::panicking() {
            // Cancel pending transmissions so they don't get sent
            self.0.completion_rx.cancel();

            let error_code = crate::stream2::endpoint::reset_error::ABNORMAL_TERMINATION;
            let _ = self
                .0
                .send_reset_packet(error_code, crate::packet::datagram::ResetTarget::Both);
            debug!(
                stream_id = self.0.stream_id.as_u64(),
                "Writer dropped during panic - sent FlowReset and cancelled transmissions"
            );
        } else {
            // Normal drop - graceful shutdown
            let _ = self.shutdown();
        }
    }
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
        // No-op to match TCP semantics
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
