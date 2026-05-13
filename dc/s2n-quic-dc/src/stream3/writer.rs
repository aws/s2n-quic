// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0

//! Stream3 Writer: Fragmentation and flow control
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
//!    `should_transmit` and `receiver_alive` flags are cleared, and a FlowReset with
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
// * MTU estimation is overly conservative. MAX_FLOW_DATA_HEADER_OVERHEAD assumes worst-case
//   VarInt sizes for all fields (8 bytes each), but many fields have known values at frame
//   construction time (stream_id, queue_ids, offset). We should compute the actual header
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
//   early data with FlowInit, completion failure handling, panic-drop behavior, and
//   multi-stream contention on shared pipeline resources.

use crate::{
    byte_vec::ByteVec,
    intrusive_queue::{Entry, Queue},
    packet::{
        control,
        datagram::{partial::MAX_FLOW_DATA_HEADER_OVERHEAD, QueuePair, ResetTarget},
    },
    path::secret::map::Entry as PathSecretEntry,
    stream3::{
        endpoint::{
            msg,
            reset_error::{self, ResetError},
        },
        frame::{self, Frame, Header, HomogeneousBatch, SubmissionSender, DEFAULT_TTL},
    },
};
use crate::datagram::batch::Priority;
use s2n_quic_core::{
    buffer::{self, writer::Storage},
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

use super::coop::{self, Coop, HasCoop};

pub struct Writer(Box<Inner>);

struct Inner {
    /// Channel to submit frames to the wheel
    frame_tx: SubmissionSender,
    /// Receiver for completion notifications from the pipeline
    completion_rx: frame::CompletionReceiver,
    /// Control-side channel for receiving MAX_DATA frames
    control_rx: msg::queue::Control,
    /// Path secret entry providing MTU and crypto material
    path_secret_entry: Arc<PathSecretEntry>,
    /// Cached packet size (MTU minus header overhead) for fragmentation
    packet_size: u16,
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
    /// Cooperative yield budget
    coop: Coop,
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
        on_send_flow_init(Init => FlowInitSent);
        on_flow_established(FlowInitSent => Open);
        on_send_fin(FlowInitSent | Open => FinSent);
        on_shutdown(Init | FlowInitSent | Open | FinSent => Shutdown);
    }
}

impl Writer {
    pub(crate) fn new_client(
        frame_tx: SubmissionSender,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        acceptor_id: VarInt,
        control_rx: msg::queue::Control,
    ) -> Self {
        let completion_rx = frame::completion_channel();
        let parameters = path_secret_entry.parameters();
        let mtu = parameters.max_datagram_size();
        let packet_size = mtu.saturating_sub(MAX_FLOW_DATA_HEADER_OVERHEAD);
        let max_inflight_bytes = parameters.local_send_max_data.as_u64();
        let remote_max_data = VarInt::ZERO;

        Self(Box::new(Inner {
            frame_tx,
            completion_rx,
            control_rx,
            path_secret_entry,
            packet_size,
            stream_id,
            acceptor_id,
            next_offset: VarInt::ZERO,
            inflight_bytes: 0,
            max_inflight_bytes,
            remote_max_data,
            status: Status::Init,
            reset_error_code: None,
            coop: Coop::default(),
        }))
    }

    pub(crate) fn new_server(
        frame_tx: SubmissionSender,
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        control_rx: msg::queue::Control,
    ) -> Self {
        let completion_rx = frame::completion_channel();
        let parameters = path_secret_entry.parameters();
        let mtu = parameters.max_datagram_size();
        let packet_size = mtu.saturating_sub(MAX_FLOW_DATA_HEADER_OVERHEAD);
        let max_inflight_bytes = parameters.local_send_max_data.as_u64();
        let initial_remote_max_data = parameters.remote_max_data;

        Self(Box::new(Inner {
            frame_tx,
            completion_rx,
            control_rx,
            path_secret_entry,
            packet_size,
            stream_id,
            acceptor_id: VarInt::ZERO,
            next_offset: VarInt::ZERO,
            inflight_bytes: 0,
            max_inflight_bytes,
            remote_max_data: initial_remote_max_data,
            status: Status::Open,
            reset_error_code: None,
            coop: Coop::default(),
        }))
    }

    /// Writes the provided buffer to the stream
    ///
    /// Note that the stream may not write all of the provided data. If the application wants to flush
    /// the entire buffer, prefer [`Self::write_all_from`] instead.
    pub async fn write_from<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        core::future::poll_fn(|cx| self.poll_write_from(cx, buf, false)).await
    }

    /// Write all data from a buffer
    ///
    /// This method blocks until all of the data is written or the stream returns an error.
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
    ///
    /// Note that the stream may not write all of the provided data. This method signals to the transport
    /// that the provided length is the final amount to send. If the application wants to flush
    /// the entire buffer, prefer [`Self::write_all_from_fin`] instead.
    pub async fn write_from_fin<S>(&mut self, buf: &mut S) -> io::Result<usize>
    where
        S: buffer::reader::storage::Infallible,
    {
        core::future::poll_fn(|cx| self.poll_write_from(cx, buf, true)).await
    }

    /// Write all data from a buffer and send FIN
    ///
    /// This method blocks until all of the data is written or the stream returns an error.
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

    pub fn shutdown(&mut self) -> io::Result<()> {
        self.0.shutdown()
    }

    pub(crate) fn force_shutdown(&mut self) {
        self.0.completion_rx.cancel();
        self.0.status.on_shutdown().ok();
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

        self.poll_completions(cx)?;
        let _ = self.poll_remote_budget(cx)?;

        if self.status.is_init() {
            let (written, is_fin) = self.send_flow_init_with_early_data(buf, is_fin)?;

            if written > 0 || is_fin {
                return Poll::Ready(Ok(written));
            }

            return Poll::Pending;
        }

        if self.status.is_flow_init_sent() {
            trace!(
                stream_id = self.stream_id.as_u64(),
                "Writer blocked in FlowInitSent - waiting for remote MAX_DATA"
            );
            return Poll::Pending;
        }

        let available = self.min_send_budget();
        if available == 0 && !is_fin {
            return Poll::Pending;
        }

        let written = self.send_data(buf, is_fin)?;

        Poll::Ready(Ok(written))
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
        let Some(remote_queue_id) = self.control_rx.remote_queue_id() else {
            debug!(
                stream_id = self.stream_id.as_u64(),
                "Cannot send reset before flow established"
            );
            return Ok(());
        };

        let frame = Frame {
            source_sender_id: VarInt::MAX,
            header: Header::FlowReset {
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
            "Sent FlowReset"
        );

        Ok(())
    }

    fn send_fin_packet(&mut self) -> io::Result<()> {
        if self.status.is_init() {
            self.send_flow_init_with_early_data(&mut buffer::reader::storage::Empty, true)?;
        } else if self.status.is_open() {
            let queue_pair = QueuePair {
                source_queue_id: self.control_rx.queue_id(),
                dest_queue_id: self
                    .control_rx
                    .remote_queue_id()
                    .expect("remote_queue_id must be set when Open"),
            };

            let frame = Frame {
                source_sender_id: VarInt::MAX,
                header: Header::FlowData {
                    queue_pair,
                    stream_id: self.stream_id,
                    offset: self.next_offset,
                    is_fin: true,
                },
                payload: ByteVec::new(),
                path_secret_entry: self.path_secret_entry.clone(),
                completion: Some(self.completion_rx.sender()),
                status: frame::TransmissionStatus::default(),
                ttl: DEFAULT_TTL,
                transmission_time: None,
            };

            self.send_frame(frame)?;

            debug!(stream_id = self.stream_id.as_u64(), "Sent FIN");
            self.status.on_send_fin().unwrap();
        }

        Ok(())
    }

    fn poll_completions(&mut self, cx: &mut Context) -> io::Result<()> {
        use crate::stream3::frame::{FailureReason, TransmissionStatus};

        match self.completion_rx.poll_swap(cx) {
            Poll::Ready(Some(queue)) => {
                let mut freed_bytes = 0u64;
                let mut failure = None;

                for completed in queue.iter() {
                    match completed.status {
                        TransmissionStatus::Acknowledged => {
                            freed_bytes += completed.payload.len() as u64;
                        }
                        TransmissionStatus::Failed(reason) => {
                            failure.get_or_insert(reason);
                            freed_bytes += completed.payload.len() as u64;

                            debug!(
                                stream_id = self.stream_id.as_u64(),
                                ?reason,
                                "Transmission failed"
                            );
                        }
                        TransmissionStatus::Pending => {
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
                            let error_code = reset_error::RETRANSMISSIONS_EXHAUSTED;
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
                    stream_id = self.stream_id.as_u64(),
                    status = ?self.status,
                    msg_count = queue.len(),
                    "poll_remote_budget received messages"
                );
                for msg in queue {
                    match msg.into_inner() {
                        msg::Control::Frames { mut payload } => {
                            if self.handle_control_frames(&mut *payload).is_err() {
                                let error_code = reset_error::FRAME_DECODE_ERROR;
                                self.reset_error_code = Some(error_code);
                                self.status.on_shutdown().ok();

                                let _ = self.send_reset_frame(error_code, ResetTarget::Both);

                                let reset_error: ResetError = error_code.into();
                                return Poll::Ready(Err(io::Error::new(
                                    io::ErrorKind::InvalidData,
                                    reset_error,
                                )));
                            }

                            if self.status.on_flow_established().is_ok() {
                                debug_assert!(self.control_rx.remote_queue_id().is_some());
                                debug!(stream_id = self.stream_id.as_u64(), "Flow established");
                            }
                        }
                        msg::Control::Reset { error_code } => {
                            self.reset_error_code = Some(error_code);
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
                trace!(
                    stream_id = self.stream_id.as_u64(),
                    status = ?self.status,
                    "poll_remote_budget pending - no control messages"
                );
                Poll::Pending
            }
        }
    }

    fn handle_control_frames(&mut self, payload: &mut [u8]) -> Result<(), s2n_codec::DecoderError> {
        use s2n_quic_core::frame::{FrameMut, MaxData};

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

    fn send_flow_init_with_early_data<S>(
        &mut self,
        buf: &mut S,
        is_fin: bool,
    ) -> io::Result<(usize, bool)>
    where
        S: buffer::reader::storage::Infallible,
    {
        let (payload, bytes_read, actual_fin) = self.prepare_early_data(buf, is_fin)?;

        let frame = Frame {
            source_sender_id: VarInt::MAX,
            header: Header::FlowInit {
                source_queue_id: self.control_rx.queue_id(),
                dest_acceptor_id: self.acceptor_id,
                attempt_id: VarInt::MAX,
                stream_id: self.stream_id,
                is_fin: actual_fin,
            },
            payload,
            path_secret_entry: self.path_secret_entry.clone(),
            completion: Some(self.completion_rx.sender()),
            status: frame::TransmissionStatus::default(),
            ttl: DEFAULT_TTL,
            transmission_time: None,
        };

        self.send_frame(frame)?;

        self.status.on_send_flow_init().unwrap();

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

            let queue_pair = QueuePair {
                source_queue_id: self.control_rx.queue_id(),
                dest_queue_id: self
                    .control_rx
                    .remote_queue_id()
                    .expect("remote_queue_id must be set when Open"),
            };

            let frame = Frame {
                source_sender_id: VarInt::MAX,
                header: Header::FlowData {
                    queue_pair,
                    stream_id: self.stream_id,
                    offset,
                    is_fin: include_fin,
                },
                payload,
                path_secret_entry: self.path_secret_entry.clone(),
                completion: Some(self.completion_rx.sender()),
                status: frame::TransmissionStatus::default(),
                ttl: DEFAULT_TTL,
                transmission_time: None,
            };

            frames.push_back(frame.into());

            self.advance_offset(payload_len)?;
            written += payload_len;

            trace!(
                stream_id = self.stream_id.as_u64(),
                offset = offset.as_u64(),
                payload_len,
                is_fin = include_fin,
                "Sending FlowData"
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
                priority: Priority::FlowData,
            })
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "frame channel closed"))
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        debug!(
            stream_id = self.0.stream_id.as_u64(),
            status = ?self.0.status,
            next_offset = self.0.next_offset.as_u64(),
            inflight_bytes = self.0.inflight_bytes,
            remote_max_data = self.0.remote_max_data.as_u64(),
            "Writer dropping"
        );

        if std::thread::panicking() {
            self.0.completion_rx.cancel();

            let error_code = reset_error::ABNORMAL_TERMINATION;
            let _ = self.0.send_reset_frame(error_code, ResetTarget::Both);
            debug!(
                stream_id = self.0.stream_id.as_u64(),
                "Writer dropped during panic - sent FlowReset and cancelled transmissions"
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
mod tests {
    use super::*;
    use crate::flow;
    use bytes::Bytes;

    fn new_test_inner() -> (Inner, crate::stream3::frame::SubmissionReceiver) {
        let (frame_tx, frame_rx) = crate::stream3::frame::submission_channel(1);

        let path_secret_entry = PathSecretEntry::fake("127.0.0.1:8080".parse().unwrap(), None);
        let stream_id = VarInt::from_u8(42);
        let handle = flow::Handle::client(stream_id, path_secret_entry.clone());
        let mut allocator = msg::queue::Allocator::new();
        let (control_rx, _stream_rx) = allocator.alloc_or_grow(handle, Some(VarInt::from_u8(7)));

        let inner = Inner {
            frame_tx,
            completion_rx: frame::completion_channel(),
            control_rx,
            path_secret_entry,
            packet_size: 1200,
            stream_id,
            acceptor_id: VarInt::ZERO,
            next_offset: VarInt::ZERO,
            inflight_bytes: 0,
            max_inflight_bytes: 4096,
            remote_max_data: VarInt::from_u16(4096),
            status: Status::Open,
            reset_error_code: None,
            coop: Coop::default(),
        };

        (inner, frame_rx)
    }

    fn completed_frame(
        path_secret_entry: Arc<PathSecretEntry>,
        stream_id: VarInt,
        payload_len: usize,
        status: frame::TransmissionStatus,
    ) -> Frame {
        let mut payload = ByteVec::new();
        if payload_len > 0 {
            payload.push_back(Bytes::from(vec![0; payload_len]));
        }

        Frame {
            source_sender_id: VarInt::MAX,
            header: Header::FlowData {
                queue_pair: QueuePair {
                    source_queue_id: VarInt::from_u8(1),
                    dest_queue_id: VarInt::from_u8(2),
                },
                stream_id,
                offset: VarInt::ZERO,
                is_fin: false,
            },
            payload,
            path_secret_entry,
            completion: None,
            status,
            ttl: DEFAULT_TTL,
            transmission_time: None,
        }
    }

    fn send_completions(inner: &Inner, completions: impl IntoIterator<Item = Frame>) {
        let mut queue = Queue::new();
        for completion in completions {
            queue.push_back(completion.into());
        }
        inner.completion_rx.sender().send_batch(queue).unwrap();
    }

    fn noop_cx() -> core::task::Context<'static> {
        let waker = Box::leak(Box::new(s2n_quic_core::task::waker::noop()));
        core::task::Context::from_waker(waker)
    }

    #[test]
    fn poll_completions_prefers_first_failure_and_skips_later_reset() {
        let (mut inner, mut frame_rx) = new_test_inner();
        inner.inflight_bytes = 23;

        send_completions(
            &inner,
            [
                completed_frame(
                    inner.path_secret_entry.clone(),
                    inner.stream_id,
                    5,
                    frame::TransmissionStatus::Acknowledged,
                ),
                completed_frame(
                    inner.path_secret_entry.clone(),
                    inner.stream_id,
                    7,
                    frame::TransmissionStatus::Failed(frame::FailureReason::UnknownPathSecret),
                ),
                completed_frame(
                    inner.path_secret_entry.clone(),
                    inner.stream_id,
                    11,
                    frame::TransmissionStatus::Failed(frame::FailureReason::TransmissionError),
                ),
            ],
        );

        let mut cx = noop_cx();
        let err = inner.poll_completions(&mut cx).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::ConnectionRefused);
        assert_eq!(inner.inflight_bytes, 0);
        assert!(inner.status.is_shutdown());
        let mut staging = crate::stream3::frame::PriorityStorage::default();
        assert!(matches!(
            frame_rx.poll_swap(&mut cx, &mut staging),
            Poll::Pending
        ));
    }

    #[test]
    fn poll_completions_keeps_first_transmission_error() {
        let (mut inner, mut frame_rx) = new_test_inner();
        inner.inflight_bytes = 18;

        send_completions(
            &inner,
            [
                completed_frame(
                    inner.path_secret_entry.clone(),
                    inner.stream_id,
                    7,
                    frame::TransmissionStatus::Failed(frame::FailureReason::TransmissionError),
                ),
                completed_frame(
                    inner.path_secret_entry.clone(),
                    inner.stream_id,
                    11,
                    frame::TransmissionStatus::Failed(frame::FailureReason::PeerDead),
                ),
            ],
        );

        let mut cx = noop_cx();
        let err = inner.poll_completions(&mut cx).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::BrokenPipe);
        assert_eq!(inner.inflight_bytes, 0);
        assert!(inner.status.is_shutdown());

        let mut staging = crate::stream3::frame::PriorityStorage::default();
        match frame_rx.poll_swap(&mut cx, &mut staging) {
            Poll::Ready(Some(())) => {}
            other => panic!("expected reset frame, got {other:?}"),
        }

        let sent = staging.iter().collect::<Vec<_>>();
        assert_eq!(sent.len(), 1);
        assert!(matches!(
            sent[0].header,
            Header::FlowReset {
                error_code,
                reset_target: ResetTarget::Both,
            ..
            } if error_code == reset_error::RETRANSMISSIONS_EXHAUSTED
        ));
    }

    #[test]
    fn prepare_early_data_returns_error_before_consuming_on_offset_overflow() {
        let (mut inner, _) = new_test_inner();
        inner.next_offset = VarInt::MAX;

        let mut buf = &b"x"[..];
        let err = inner.prepare_early_data(&mut buf, false).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(buf, b"x");
        assert_eq!(inner.inflight_bytes, 0);
        assert_eq!(inner.next_offset, VarInt::MAX);
    }

    #[test]
    fn send_data_allows_fin_at_varint_max() {
        let (mut inner, mut frame_rx) = new_test_inner();
        inner.next_offset = VarInt::MAX;

        let mut buf = buffer::reader::storage::Empty;
        let written = inner.send_data(&mut buf, true).unwrap();

        assert_eq!(written, 0);
        assert!(inner.status.is_fin_sent());
        assert_eq!(inner.next_offset, VarInt::MAX);

        let mut cx = noop_cx();
        let mut staging = crate::stream3::frame::PriorityStorage::default();
        match frame_rx.poll_swap(&mut cx, &mut staging) {
            Poll::Ready(Some(())) => {}
            other => panic!("expected FIN frame, got {other:?}"),
        }

        let sent = staging.iter().collect::<Vec<_>>();
        assert_eq!(sent.len(), 1);
        assert!(matches!(
            sent[0].header,
            Header::FlowData {
                offset,
                is_fin: true,
                ..
            } if offset == VarInt::MAX
        ));
    }

    #[test]
    fn send_data_caps_payload_at_varint_max() {
        let (mut inner, mut frame_rx) = new_test_inner();
        inner.next_offset = VarInt::MAX - VarInt::from_u8(1);
        inner.remote_max_data = VarInt::MAX;

        let mut buf = &b"xy"[..];
        let written = inner.send_data(&mut buf, false).unwrap();

        assert_eq!(written, 1);
        assert_eq!(buf, b"y");
        assert_eq!(inner.inflight_bytes, 1);
        assert_eq!(inner.next_offset, VarInt::MAX);

        let mut cx = noop_cx();
        let mut staging = crate::stream3::frame::PriorityStorage::default();
        match frame_rx.poll_swap(&mut cx, &mut staging) {
            Poll::Ready(Some(())) => {}
            other => panic!("expected data frame, got {other:?}"),
        }

        let sent = staging.iter().collect::<Vec<_>>();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].payload.len(), 1);
    }

    #[test]
    fn send_data_returns_error_before_consuming_when_offset_is_max() {
        let (mut inner, mut frame_rx) = new_test_inner();
        inner.next_offset = VarInt::MAX;
        inner.remote_max_data = VarInt::MAX;

        let mut buf = &b"x"[..];
        let err = inner.send_data(&mut buf, false).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(buf, b"x");
        assert_eq!(inner.next_offset, VarInt::MAX);
        assert_eq!(inner.inflight_bytes, 0);

        let mut cx = noop_cx();
        let mut staging = crate::stream3::frame::PriorityStorage::default();
        assert!(matches!(
            frame_rx.poll_swap(&mut cx, &mut staging),
            Poll::Pending
        ));
    }
}
